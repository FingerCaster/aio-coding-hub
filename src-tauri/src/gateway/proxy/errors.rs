//! Usage: Error classification + standardized gateway error responses.

use axum::{
    body::{to_bytes, Bytes},
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;

use super::failover::FailoverDecision;
use super::{ErrorCategory, GatewayErrorCode};
use crate::gateway::events::FailoverAttempt;
use crate::gateway::plugins::context::{GatewayPluginHookName, GatewayResponseHookInput};
use crate::gateway::plugins::pipeline::GatewayPluginPipeline;
use std::sync::Arc;

const MAX_PLUGIN_ERROR_BODY_BYTES: usize = 256 * 1024;
const CLIENT_TRANSIENT_FAILURE_MESSAGE: &str = "upstream transient failure";

#[derive(Debug, Serialize)]
struct GatewayErrorResponse {
    trace_id: String,
    error_code: &'static str,
    message: String,
    attempts: Vec<FailoverAttempt>,
    #[serde(skip_serializing_if = "Option::is_none")]
    retry_after_seconds: Option<u64>,
}

fn sanitize_attempts_for_client(mut attempts: Vec<FailoverAttempt>) -> Vec<FailoverAttempt> {
    for attempt in &mut attempts {
        let capacity_evidence = attempt
            .stream_internal_error
            .as_ref()
            .is_some_and(crate::usage::StreamInternalErrorEvidence::contains_codex_capacity_signal);
        let capacity_reason = attempt
            .reason
            .as_deref()
            .is_some_and(crate::usage::contains_codex_capacity_signal);

        if capacity_evidence || capacity_reason {
            attempt.reason = Some(CLIENT_TRANSIENT_FAILURE_MESSAGE.to_string());
            // The full bounded evidence remains in gateway events and request
            // logs. Client-facing diagnostics must not expose any capacity
            // signal that could make Codex stop instead of retrying.
            attempt.stream_internal_error = None;
        }
    }

    attempts
}

fn sanitize_message_for_client(message: String) -> String {
    if crate::usage::contains_codex_capacity_signal(&message) {
        CLIENT_TRANSIENT_FAILURE_MESSAGE.to_string()
    } else {
        message
    }
}

pub(super) fn classify_reqwest_error(err: &reqwest::Error) -> (ErrorCategory, &'static str) {
    if err.is_timeout() {
        return (
            ErrorCategory::SystemError,
            GatewayErrorCode::UpstreamTimeout.as_str(),
        );
    }
    if err.is_connect() {
        return (
            ErrorCategory::SystemError,
            GatewayErrorCode::UpstreamConnectFailed.as_str(),
        );
    }
    (
        ErrorCategory::SystemError,
        GatewayErrorCode::InternalError.as_str(),
    )
}

pub(super) fn classify_upstream_status(
    status: reqwest::StatusCode,
) -> (ErrorCategory, &'static str, FailoverDecision) {
    if status.is_server_error() {
        return (
            ErrorCategory::ProviderError,
            GatewayErrorCode::Upstream5xx.as_str(),
            FailoverDecision::RetrySameProvider,
        );
    }

    match status.as_u16() {
        401 | 403 => (
            ErrorCategory::ProviderError,
            GatewayErrorCode::Upstream4xx.as_str(),
            FailoverDecision::SwitchProvider,
        ),
        402 => (
            // Payment Required / insufficient balance / subscription required.
            // Align with : treat as provider-side limitation and allow failover.
            ErrorCategory::ProviderError,
            GatewayErrorCode::Upstream4xx.as_str(),
            FailoverDecision::SwitchProvider,
        ),
        404 => (
            // Resource not found is often provider-specific (path/model support mismatch).
            ErrorCategory::ResourceNotFound,
            GatewayErrorCode::Upstream4xx.as_str(),
            FailoverDecision::SwitchProvider,
        ),
        408 | 429 => (
            ErrorCategory::ProviderError,
            GatewayErrorCode::Upstream4xx.as_str(),
            FailoverDecision::RetrySameProvider,
        ),
        _ if status.is_client_error() => (
            // Default: allow retry + failover for upstream 4xx.
            // Non-retryable client input errors are detected separately by scanning upstream error bodies.
            ErrorCategory::ProviderError,
            GatewayErrorCode::Upstream4xx.as_str(),
            FailoverDecision::RetrySameProvider,
        ),
        _ => (
            ErrorCategory::ProviderError,
            GatewayErrorCode::InternalError.as_str(),
            FailoverDecision::Abort,
        ),
    }
}

pub(super) fn error_response(
    status: StatusCode,
    trace_id: String,
    error_code: &'static str,
    message: String,
    attempts: Vec<FailoverAttempt>,
) -> Response {
    error_response_with_retry_after(status, trace_id, error_code, message, attempts, None)
}

pub(super) fn error_response_with_retry_after(
    status: StatusCode,
    trace_id: String,
    error_code: &'static str,
    message: String,
    attempts: Vec<FailoverAttempt>,
    retry_after_seconds: Option<u64>,
) -> Response {
    let payload = GatewayErrorResponse {
        trace_id: trace_id.clone(),
        error_code,
        message: sanitize_message_for_client(message),
        attempts: sanitize_attempts_for_client(attempts),
        retry_after_seconds,
    };

    let mut resp = (status, Json(payload)).into_response();

    if let Ok(v) = HeaderValue::from_str(&trace_id) {
        resp.headers_mut().insert("x-trace-id", v);
    }

    if let Some(seconds) = retry_after_seconds.filter(|v| *v > 0) {
        let value = seconds.to_string();
        if let Ok(v) = HeaderValue::from_str(&value) {
            resp.headers_mut().insert(header::RETRY_AFTER, v);
        }
    }

    resp
}

pub(super) async fn apply_gateway_error_hook(
    db: &crate::db::Db,
    pipeline: Arc<GatewayPluginPipeline>,
    trace_id: String,
    response: Response,
) -> Response {
    let status = response.status();
    let mut headers = response.headers().clone();
    let body = match to_bytes(response.into_body(), MAX_PLUGIN_ERROR_BODY_BYTES).await {
        Ok(body) => body,
        Err(err) => {
            tracing::warn!(
                trace_id = %trace_id,
                error = %err,
                "failed to read gateway error response body for plugin hook"
            );
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                trace_id,
                GatewayErrorCode::ResponseBuildError.as_str(),
                "failed to read gateway error response body within plugin error body limit"
                    .to_string(),
                vec![],
            );
        }
    };

    let input = GatewayResponseHookInput {
        hook_name: GatewayPluginHookName::Error,
        trace_id: trace_id.clone(),
        status: status.as_u16(),
        headers: headers.clone(),
        body: body.clone(),
    };

    let output = match pipeline.run_response_hook(input).await {
        Ok(output) => {
            crate::gateway::plugins::audit::persist_gateway_plugin_diagnostics(
                db,
                &trace_id,
                output.audit_events.clone(),
                output.execution_reports.clone(),
            );
            output
        }
        Err(mut err) => {
            crate::gateway::plugins::audit::persist_gateway_plugin_error_audit_events(
                db, &trace_id, &mut err,
            );
            tracing::warn!(
                trace_id = %trace_id,
                error = %err,
                "plugin gateway.error hook failed; keeping original error response"
            );
            let mut builder = Response::builder().status(status);
            for (name, value) in headers.iter() {
                builder = builder.header(name, value);
            }
            return builder
                .body(axum::body::Body::from(body))
                .unwrap_or_else(|_| {
                    error_response(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        trace_id,
                        GatewayErrorCode::ResponseBuildError.as_str(),
                        "failed to rebuild gateway error response".to_string(),
                        vec![],
                    )
                });
        }
    };

    if let Some(blocked) = output.blocked {
        tracing::warn!(
            trace_id = %trace_id,
            status = blocked.status,
            reason = %blocked.reason,
            "plugin blocked gateway error response"
        );
        return error_response(
            StatusCode::from_u16(blocked.status).unwrap_or(StatusCode::BAD_GATEWAY),
            trace_id,
            GatewayErrorCode::InternalError.as_str(),
            blocked.reason,
            vec![],
        );
    }

    headers = output.headers;
    headers.remove(header::CONTENT_LENGTH);
    let mut builder = Response::builder().status(status);
    for (name, value) in headers.iter() {
        builder = builder.header(name, value);
    }
    builder
        .body(axum::body::Body::from(Bytes::copy_from_slice(
            output.body.as_ref(),
        )))
        .unwrap_or_else(|_| {
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                trace_id,
                GatewayErrorCode::ResponseBuildError.as_str(),
                "failed to rebuild gateway error response".to_string(),
                vec![],
            )
        })
}

#[cfg(test)]
mod tests {
    use super::{
        classify_upstream_status, sanitize_attempts_for_client, sanitize_message_for_client,
        FailoverDecision, CLIENT_TRANSIENT_FAILURE_MESSAGE,
    };
    use crate::gateway::events::FailoverAttempt;
    use crate::gateway::proxy::{ErrorCategory, GatewayErrorCode};
    use crate::usage::StreamInternalErrorEvidence;

    fn stream_evidence() -> StreamInternalErrorEvidence {
        StreamInternalErrorEvidence {
            event_type: "response.failed".to_string(),
            error_type: Some("server_error".to_string()),
            error_code: None,
            message: None,
            classification: "retryable".to_string(),
            matched_keyword: None,
            disposition: "retry_exhausted".to_string(),
            truncated: false,
        }
    }

    fn attempt(
        reason: Option<&str>,
        stream_internal_error: Option<StreamInternalErrorEvidence>,
    ) -> FailoverAttempt {
        FailoverAttempt {
            provider_id: 1,
            provider_name: "Provider".to_string(),
            base_url: "https://provider.example".to_string(),
            outcome: "stream_error: code=GW_FAKE_200".to_string(),
            status: Some(502),
            provider_index: Some(1),
            retry_index: Some(1),
            session_reuse: None,
            error_category: Some(ErrorCategory::ProviderError.as_str()),
            error_code: Some(GatewayErrorCode::Fake200.as_str()),
            decision: Some(FailoverDecision::Abort.as_str()),
            reason: reason.map(str::to_string),
            selection_method: None,
            reason_code: None,
            attempt_started_ms: None,
            attempt_duration_ms: None,
            circuit_state_before: None,
            circuit_state_after: None,
            circuit_failure_count: None,
            circuit_failure_threshold: None,
            probe: None,
            probe_trigger: None,
            probe_result: None,
            probe_generation: None,
            circuit_recover_at_unix: None,
            circuit_trigger_error_code: None,
            provider_bridged: Some(false),
            timeout_secs: None,
            stream_internal_error,
            requested_upstream_model: None,
        }
    }

    #[test]
    fn client_attempts_remove_capacity_signals_from_every_evidence_field() {
        let mut cases = Vec::new();

        let mut event_type = stream_evidence();
        event_type.event_type = "SERVER_IS_OVERLOADED".to_string();
        cases.push(event_type);

        let mut error_type = stream_evidence();
        error_type.error_type = Some("slow_down".to_string());
        cases.push(error_type);

        let mut error_code = stream_evidence();
        error_code.error_code = Some("Server_Is_Overloaded".to_string());
        cases.push(error_code);

        let mut message = stream_evidence();
        message.message = Some("Selected model is at capacity. Try another model.".to_string());
        cases.push(message);

        let mut matched_keyword = stream_evidence();
        matched_keyword.matched_keyword = Some("SLOW_DOWN".to_string());
        cases.push(matched_keyword);

        for evidence in cases {
            let sanitized = sanitize_attempts_for_client(vec![attempt(
                Some("upstream returned a terminal stream error"),
                Some(evidence),
            )]);
            assert_eq!(
                sanitized[0].reason.as_deref(),
                Some(CLIENT_TRANSIENT_FAILURE_MESSAGE)
            );
            assert!(sanitized[0].stream_internal_error.is_none());
        }
    }

    #[test]
    fn client_attempts_sanitize_capacity_reason_without_stream_evidence() {
        for reason in [
            "Selected model is at capacity; SERVER_IS_OVERLOADED",
            "upstream capacity failure",
            "provider overloaded",
        ] {
            let sanitized = sanitize_attempts_for_client(vec![attempt(Some(reason), None)]);

            assert_eq!(
                sanitized[0].reason.as_deref(),
                Some(CLIENT_TRANSIENT_FAILURE_MESSAGE)
            );
            assert!(sanitized[0].stream_internal_error.is_none());
        }
    }

    #[test]
    fn client_attempts_preserve_unrelated_verbose_evidence() {
        let mut evidence = stream_evidence();
        evidence.error_code = Some("service_unavailable_error".to_string());
        evidence.message = Some("temporary provider failure".to_string());
        let sanitized = sanitize_attempts_for_client(vec![attempt(
            Some("upstream returned a terminal stream error"),
            Some(evidence.clone()),
        )]);

        assert_eq!(
            sanitized[0].reason.as_deref(),
            Some("upstream returned a terminal stream error")
        );
        assert_eq!(sanitized[0].stream_internal_error, Some(evidence));
    }

    #[test]
    fn client_top_level_message_hides_capacity_signals() {
        for message in [
            "Selected model is at capacity",
            "upstream code=SERVER_IS_OVERLOADED",
            "please SLOW_DOWN",
            "upstream capacity failure",
            "provider overloaded",
        ] {
            assert_eq!(
                sanitize_message_for_client(message.to_string()),
                CLIENT_TRANSIENT_FAILURE_MESSAGE
            );
        }
        assert_eq!(
            sanitize_message_for_client("all providers failed".to_string()),
            "all providers failed"
        );
    }

    #[test]
    fn upstream_402_switches_provider() {
        let (category, code, decision) =
            classify_upstream_status(reqwest::StatusCode::PAYMENT_REQUIRED);
        assert!(matches!(category, ErrorCategory::ProviderError));
        assert_eq!(code, GatewayErrorCode::Upstream4xx.as_str());
        assert!(matches!(decision, FailoverDecision::SwitchProvider));
    }

    #[test]
    fn upstream_404_switches_provider() {
        let (category, code, decision) = classify_upstream_status(reqwest::StatusCode::NOT_FOUND);
        assert!(matches!(category, ErrorCategory::ResourceNotFound));
        assert_eq!(code, GatewayErrorCode::Upstream4xx.as_str());
        assert!(matches!(decision, FailoverDecision::SwitchProvider));
    }

    #[test]
    fn upstream_other_4xx_retries_then_failover() {
        let (category, code, decision) =
            classify_upstream_status(reqwest::StatusCode::UNPROCESSABLE_ENTITY);
        assert!(matches!(category, ErrorCategory::ProviderError));
        assert_eq!(code, GatewayErrorCode::Upstream4xx.as_str());
        assert!(matches!(decision, FailoverDecision::RetrySameProvider));
    }

    #[test]
    fn upstream_5xx_retries_same_provider() {
        let (category, code, decision) =
            classify_upstream_status(reqwest::StatusCode::INTERNAL_SERVER_ERROR);
        assert!(matches!(category, ErrorCategory::ProviderError));
        assert_eq!(code, GatewayErrorCode::Upstream5xx.as_str());
        assert!(matches!(decision, FailoverDecision::RetrySameProvider));

        let (category, code, decision) = classify_upstream_status(reqwest::StatusCode::BAD_GATEWAY);
        assert!(matches!(category, ErrorCategory::ProviderError));
        assert_eq!(code, GatewayErrorCode::Upstream5xx.as_str());
        assert!(matches!(decision, FailoverDecision::RetrySameProvider));

        let (category, code, decision) =
            classify_upstream_status(reqwest::StatusCode::SERVICE_UNAVAILABLE);
        assert!(matches!(category, ErrorCategory::ProviderError));
        assert_eq!(code, GatewayErrorCode::Upstream5xx.as_str());
        assert!(matches!(decision, FailoverDecision::RetrySameProvider));
    }

    #[test]
    fn upstream_401_403_switches_provider() {
        let (category, code, decision) =
            classify_upstream_status(reqwest::StatusCode::UNAUTHORIZED);
        assert!(matches!(category, ErrorCategory::ProviderError));
        assert_eq!(code, GatewayErrorCode::Upstream4xx.as_str());
        assert!(matches!(decision, FailoverDecision::SwitchProvider));

        let (category, code, decision) = classify_upstream_status(reqwest::StatusCode::FORBIDDEN);
        assert!(matches!(category, ErrorCategory::ProviderError));
        assert_eq!(code, GatewayErrorCode::Upstream4xx.as_str());
        assert!(matches!(decision, FailoverDecision::SwitchProvider));
    }

    #[test]
    fn upstream_408_429_retries_same_provider() {
        let (category, code, decision) =
            classify_upstream_status(reqwest::StatusCode::REQUEST_TIMEOUT);
        assert!(matches!(category, ErrorCategory::ProviderError));
        assert_eq!(code, GatewayErrorCode::Upstream4xx.as_str());
        assert!(matches!(decision, FailoverDecision::RetrySameProvider));

        let (category, code, decision) =
            classify_upstream_status(reqwest::StatusCode::TOO_MANY_REQUESTS);
        assert!(matches!(category, ErrorCategory::ProviderError));
        assert_eq!(code, GatewayErrorCode::Upstream4xx.as_str());
        assert!(matches!(decision, FailoverDecision::RetrySameProvider));
    }

    #[test]
    fn upstream_400_retries_same_provider() {
        let (category, code, decision) = classify_upstream_status(reqwest::StatusCode::BAD_REQUEST);
        assert!(matches!(category, ErrorCategory::ProviderError));
        assert_eq!(code, GatewayErrorCode::Upstream4xx.as_str());
        assert!(matches!(decision, FailoverDecision::RetrySameProvider));
    }
}
