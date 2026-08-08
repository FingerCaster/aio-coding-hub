//! Constrained JavaScript adapter for display-only provider account usage.

use crate::domain::provider_account_usage::{
    ProviderAccountUsageAdapterKind, ProviderAccountUsageCustomConfig,
    ProviderAccountUsageFreshness, ProviderAccountUsageResult, ProviderAccountUsageStatus,
    CUSTOM_ACCOUNT_USAGE_MAX_SCRIPT_BYTES,
};
use rquickjs::context::intrinsic;
use rquickjs::{Context, Function, Object, Runtime, Value as JsValue};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;
use std::io::{self, Read, Write};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

const JS_MEMORY_LIMIT: usize = 8 * 1024 * 1024;
const JS_STACK_LIMIT: usize = 256 * 1024;
const JS_EXECUTION_LIMIT: Duration = Duration::from_millis(100);
const MAX_REQUEST_HEADERS: usize = 32;
const MAX_REQUEST_URL_BYTES: usize = 16 * 1024;
const MAX_HEADER_NAME_BYTES: usize = 128;
const MAX_HEADER_VALUE_BYTES: usize = 8 * 1024;
const MAX_REQUEST_BODY_BYTES: usize = 64 * 1024;
const MAX_RESPONSE_BODY_BYTES: usize = 64 * 1024;
const MAX_OUTPUT_JSON_BYTES: usize = 64 * 1024;
const MAX_CONCURRENT_CUSTOM_REQUESTS: usize = 4;
const SCRIPT_WORKER_FLAG: &str = "--account-usage-script-worker";
const SCRIPT_WORKER_PROTOCOL: &str = "account-usage-script-v1";
const SCRIPT_WORKER_MAX_LINE_BYTES: usize = 512 * 1024;
const SCRIPT_WORKER_REAP_LIMIT: Duration = Duration::from_secs(2);
#[cfg(not(test))]
const SCRIPT_WORKER_STARTUP_LIMIT: Duration = Duration::from_secs(5);
#[cfg(test)]
const SCRIPT_WORKER_STARTUP_LIMIT: Duration = Duration::from_secs(30);
const SCRIPT_API_KEY_PLACEHOLDER: &str = "__AIO_ACCOUNT_USAGE_API_KEY__";
const SCRIPT_BASE_URL_PLACEHOLDER: &str = "__AIO_ACCOUNT_USAGE_BASE_URL__";
static CUSTOM_REQUEST_PERMITS: tokio::sync::Semaphore =
    tokio::sync::Semaphore::const_new(MAX_CONCURRENT_CUSTOM_REQUESTS);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ScriptContext {
    api_key: String,
    base_url: String,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ScriptFunction {
    Request,
    Parse,
}

impl ScriptFunction {
    fn name(self) -> &'static str {
        match self {
            Self::Request => "request",
            Self::Parse => "parse",
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ScriptWorkerRequest {
    source: String,
    function: ScriptFunction,
    context: Option<ScriptContext>,
    first_argument: Option<Value>,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum ScriptWorkerResponse {
    Success { value: Value },
    Error { error: CustomScriptError },
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ScriptWorkerReady {
    protocol: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CustomAccountUsageRequestPlan {
    pub url: String,
    pub method: String,
    #[serde(default)]
    pub headers: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    pub body: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ScriptResponse<'a> {
    status: u16,
    data: &'a Value,
}

pub(crate) async fn execute_custom_account_usage(
    config: &ProviderAccountUsageCustomConfig,
    base_url: &str,
    api_key: &str,
    fetched_at: i64,
) -> ProviderAccountUsageResult {
    match execute_custom_account_usage_inner(config, base_url, api_key, fetched_at).await {
        Ok(result) => result,
        Err(error) => failed_result(fetched_at, error),
    }
}

async fn execute_custom_account_usage_inner(
    config: &ProviderAccountUsageCustomConfig,
    base_url: &str,
    api_key: &str,
    fetched_at: i64,
) -> Result<ProviderAccountUsageResult, CustomScriptError> {
    let _permit = CUSTOM_REQUEST_PERMITS
        .try_acquire()
        .map_err(|_| CustomScriptError::Busy)?;
    let context = ScriptContext {
        api_key: SCRIPT_API_KEY_PLACEHOLDER.to_string(),
        base_url: SCRIPT_BASE_URL_PLACEHOLDER.to_string(),
    };
    let request_plan = evaluate_script_function_in_worker_process(
        config.script.clone(),
        ScriptFunction::Request,
        Some(context),
        None,
    )
    .await?;
    let request_plan: CustomAccountUsageRequestPlan =
        serde_json::from_value(request_plan).map_err(|_| CustomScriptError::InvalidRequest)?;
    let request_plan = materialize_request_plan(request_plan, base_url, api_key)?;
    let request = validate_and_build_request(config, base_url, request_plan)?;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(config.timeout_seconds))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(format!(
            "aio-coding-hub-custom-account-usage/{}",
            env!("CARGO_PKG_VERSION")
        ))
        .build()
        .map_err(|_| CustomScriptError::Http)?;
    let response = client
        .execute(request)
        .await
        .map_err(|_| CustomScriptError::Http)?;
    let status = response.status();
    if status.is_redirection() {
        return Err(CustomScriptError::Redirect);
    }
    if !status.is_success() {
        return Err(
            if matches!(
                status,
                reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN
            ) {
                CustomScriptError::Auth
            } else {
                CustomScriptError::HttpStatus
            },
        );
    }
    let body = read_response_body_with_limit(response).await?;
    let mut data = parse_response_json(&body)?;
    let materialized_base_url = base_url.trim_end_matches('/');
    redact_sensitive_json(&mut data, &[api_key, base_url, materialized_base_url]);
    let response = ScriptResponse {
        status: status.as_u16(),
        data: &data,
    };
    let response = serde_json::to_value(response).map_err(|_| CustomScriptError::Runtime)?;
    let output = evaluate_script_function_in_worker_process(
        config.script.clone(),
        ScriptFunction::Parse,
        None,
        Some(response),
    )
    .await?;
    normalize_custom_result(
        output,
        fetched_at,
        &[api_key, base_url, materialized_base_url],
    )
}

async fn read_response_body_with_limit(
    mut response: reqwest::Response,
) -> Result<Vec<u8>, CustomScriptError> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RESPONSE_BODY_BYTES as u64)
    {
        return Err(CustomScriptError::ResponseTooLarge);
    }

    let capacity = response
        .content_length()
        .and_then(|length| usize::try_from(length).ok())
        .unwrap_or_default()
        .min(MAX_RESPONSE_BODY_BYTES);
    let mut body = Vec::with_capacity(capacity);
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| CustomScriptError::ResponseTooLarge)?
    {
        if body
            .len()
            .checked_add(chunk.len())
            .is_none_or(|length| length > MAX_RESPONSE_BODY_BYTES)
        {
            return Err(CustomScriptError::ResponseTooLarge);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn parse_response_json(body: &[u8]) -> Result<Value, CustomScriptError> {
    serde_json::from_slice(body).map_err(|_| CustomScriptError::InvalidJson)
}

async fn evaluate_script_function_in_worker_process(
    source: String,
    function: ScriptFunction,
    context: Option<ScriptContext>,
    first_argument: Option<Value>,
) -> Result<Value, CustomScriptError> {
    let request = ScriptWorkerRequest {
        source,
        function,
        context,
        first_argument,
    };
    let request = serde_json::to_vec(&request).map_err(|_| CustomScriptError::Runtime)?;
    if request
        .len()
        .checked_add(1)
        .is_none_or(|length| length > SCRIPT_WORKER_MAX_LINE_BYTES)
    {
        return Err(CustomScriptError::Runtime);
    }

    let mut worker = ScriptWorkerChild::start().await?;
    let write_result =
        tokio::time::timeout(SCRIPT_WORKER_STARTUP_LIMIT, worker.write_request(&request)).await;
    if !matches!(write_result, Ok(Ok(()))) {
        worker.kill_and_wait().await;
        return Err(CustomScriptError::Runtime);
    }

    let response = tokio::time::timeout(JS_EXECUTION_LIMIT, worker.read_response()).await;
    let result = match response {
        Ok(Ok(ScriptWorkerResponse::Success { value })) => Ok(value),
        Ok(Ok(ScriptWorkerResponse::Error { error })) => Err(error),
        Ok(Err(error)) => Err(error),
        Err(_) => Err(CustomScriptError::Timeout),
    };
    worker.kill_and_wait().await;
    result
}

struct ScriptWorkerChild {
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    stdout: Option<BufReader<ChildStdout>>,
}

impl ScriptWorkerChild {
    async fn start() -> Result<Self, CustomScriptError> {
        let program = std::env::current_exe().map_err(|_| CustomScriptError::Runtime)?;
        let mut command = Command::new(program);
        command.args(script_worker_args());
        command.env_clear();
        preserve_script_worker_environment(&mut command);
        command.stdin(Stdio::piped());
        command.stdout(Stdio::piped());
        command.stderr(Stdio::null());
        command.kill_on_drop(true);
        #[cfg(windows)]
        {
            command.creation_flags(0x08000000);
        }

        let mut child = command.spawn().map_err(|_| CustomScriptError::Runtime)?;
        let stdin = match child.stdin.take() {
            Some(stdin) => stdin,
            None => {
                kill_and_wait_child(&mut child).await;
                return Err(CustomScriptError::Runtime);
            }
        };
        let stdout = match child.stdout.take() {
            Some(stdout) => stdout,
            None => {
                kill_and_wait_child(&mut child).await;
                return Err(CustomScriptError::Runtime);
            }
        };
        let mut worker = Self {
            child: Some(child),
            stdin: Some(stdin),
            stdout: Some(BufReader::new(stdout)),
        };
        let ready = tokio::time::timeout(SCRIPT_WORKER_STARTUP_LIMIT, worker.read_ready()).await;
        if !matches!(ready, Ok(Ok(()))) {
            worker.kill_and_wait().await;
            return Err(CustomScriptError::Runtime);
        }
        Ok(worker)
    }

    async fn read_ready(&mut self) -> Result<(), CustomScriptError> {
        loop {
            let line = self.read_line().await?;
            let ready = serde_json::from_slice::<ScriptWorkerReady>(&line);
            if ready
                .as_ref()
                .is_ok_and(|ready| ready.protocol == SCRIPT_WORKER_PROTOCOL)
            {
                return Ok(());
            }
            if !cfg!(test) {
                return Err(CustomScriptError::Runtime);
            }
        }
    }

    async fn write_request(&mut self, request: &[u8]) -> Result<(), CustomScriptError> {
        let stdin = self.stdin.as_mut().ok_or(CustomScriptError::Runtime)?;
        stdin
            .write_all(request)
            .await
            .map_err(|_| CustomScriptError::Runtime)?;
        stdin
            .write_all(b"\n")
            .await
            .map_err(|_| CustomScriptError::Runtime)?;
        stdin
            .flush()
            .await
            .map_err(|_| CustomScriptError::Runtime)?;
        self.stdin.take();
        Ok(())
    }

    async fn read_response(&mut self) -> Result<ScriptWorkerResponse, CustomScriptError> {
        loop {
            let line = self.read_line().await?;
            match serde_json::from_slice(&line) {
                Ok(response) => return Ok(response),
                Err(_) if cfg!(test) => continue,
                Err(_) => return Err(CustomScriptError::Runtime),
            }
        }
    }

    async fn read_line(&mut self) -> Result<Vec<u8>, CustomScriptError> {
        let stdout = self.stdout.as_mut().ok_or(CustomScriptError::Runtime)?;
        read_bounded_worker_line(stdout).await
    }

    async fn kill_and_wait(&mut self) {
        self.stdin.take();
        self.stdout.take();
        if let Some(child) = self.child.as_mut() {
            kill_and_wait_child(child).await;
        }
        self.child.take();
    }
}

impl Drop for ScriptWorkerChild {
    fn drop(&mut self) {
        self.stdin.take();
        self.stdout.take();
        if let Some(mut child) = self.child.take() {
            let _ = child.start_kill();
            if let Ok(handle) = tokio::runtime::Handle::try_current() {
                handle.spawn(async move {
                    let _ = child.wait().await;
                });
            }
        }
    }
}

async fn kill_and_wait_child(child: &mut Child) {
    match child.try_wait() {
        Ok(Some(_)) => {}
        Ok(None) | Err(_) => {
            let _ = child.start_kill();
            let _ = tokio::time::timeout(SCRIPT_WORKER_REAP_LIMIT, child.wait()).await;
        }
    }
}

async fn read_bounded_worker_line(
    reader: &mut BufReader<ChildStdout>,
) -> Result<Vec<u8>, CustomScriptError> {
    let mut line = Vec::new();
    let mut byte = [0_u8; 1];
    loop {
        let read = reader
            .read(&mut byte)
            .await
            .map_err(|_| CustomScriptError::Runtime)?;
        if read == 0 {
            return if line.is_empty() {
                Err(CustomScriptError::Runtime)
            } else {
                Ok(line)
            };
        }
        if byte[0] == b'\n' {
            return Ok(line);
        }
        line.push(byte[0]);
        if line.len() > SCRIPT_WORKER_MAX_LINE_BYTES {
            return Err(CustomScriptError::Runtime);
        }
    }
}

fn script_worker_args() -> Vec<String> {
    #[cfg(not(test))]
    {
        vec![SCRIPT_WORKER_FLAG.to_string()]
    }
    #[cfg(test)]
    {
        vec![
            "--exact".to_string(),
            "domain::provider_account_usage_script::account_usage_script_worker_process_entry_for_tests"
                .to_string(),
            "--nocapture".to_string(),
            "--".to_string(),
            SCRIPT_WORKER_FLAG.to_string(),
        ]
    }
}

fn preserve_script_worker_environment(command: &mut Command) {
    #[cfg(windows)]
    const ENV_ALLOWLIST: &[&str] = &["SystemRoot", "WINDIR", "TEMP", "TMP"];
    #[cfg(target_os = "macos")]
    const ENV_ALLOWLIST: &[&str] = &["TMPDIR"];
    #[cfg(all(unix, not(target_os = "macos")))]
    const ENV_ALLOWLIST: &[&str] = &["TMPDIR", "APPDIR", "APPIMAGE", "ARGV0"];
    for key in ENV_ALLOWLIST {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
}

pub(crate) fn run_account_usage_script_worker() {
    if run_account_usage_script_worker_inner().is_err() {
        std::process::exit(1);
    }
}

#[cfg(test)]
#[test]
fn account_usage_script_worker_process_entry_for_tests() {
    if !std::env::args().any(|arg| arg == SCRIPT_WORKER_FLAG) {
        return;
    }
    run_account_usage_script_worker();
}

fn run_account_usage_script_worker_inner() -> Result<(), CustomScriptError> {
    write_worker_message(&ScriptWorkerReady {
        protocol: SCRIPT_WORKER_PROTOCOL.to_string(),
    })?;
    let request = read_bounded_worker_stdin_line()?;
    let request: ScriptWorkerRequest =
        serde_json::from_slice(&request).map_err(|_| CustomScriptError::Runtime)?;
    let result = if request.source.len() > CUSTOM_ACCOUNT_USAGE_MAX_SCRIPT_BYTES {
        Err(CustomScriptError::Runtime)
    } else {
        match (
            request.function,
            request.context.as_ref(),
            request.first_argument.as_ref(),
        ) {
            (ScriptFunction::Request, Some(context), None) => evaluate_script_function(
                &request.source,
                request.function.name(),
                Some(context),
                None,
            ),
            (ScriptFunction::Parse, None, Some(first_argument)) => evaluate_script_function(
                &request.source,
                request.function.name(),
                None,
                Some(first_argument),
            ),
            _ => Err(CustomScriptError::Runtime),
        }
    };
    let response = match result {
        Ok(value) => ScriptWorkerResponse::Success { value },
        Err(error) => ScriptWorkerResponse::Error { error },
    };
    write_worker_message(&response)
}

fn read_bounded_worker_stdin_line() -> Result<Vec<u8>, CustomScriptError> {
    let stdin = io::stdin();
    let mut reader = stdin.lock();
    let mut line = Vec::new();
    let mut byte = [0_u8; 1];
    loop {
        let read = reader
            .read(&mut byte)
            .map_err(|_| CustomScriptError::Runtime)?;
        if read == 0 {
            return if line.is_empty() {
                Err(CustomScriptError::Runtime)
            } else {
                Ok(line)
            };
        }
        if byte[0] == b'\n' {
            return Ok(line);
        }
        line.push(byte[0]);
        if line.len() > SCRIPT_WORKER_MAX_LINE_BYTES {
            return Err(CustomScriptError::Runtime);
        }
    }
}

fn write_worker_message(message: &impl Serialize) -> Result<(), CustomScriptError> {
    let message = serde_json::to_vec(message).map_err(|_| CustomScriptError::Runtime)?;
    if message
        .len()
        .checked_add(1)
        .is_none_or(|length| length > SCRIPT_WORKER_MAX_LINE_BYTES)
    {
        return Err(CustomScriptError::Runtime);
    }
    let stdout = io::stdout();
    let mut writer = stdout.lock();
    writer
        .write_all(&message)
        .map_err(|_| CustomScriptError::Runtime)?;
    writer
        .write_all(b"\n")
        .map_err(|_| CustomScriptError::Runtime)?;
    writer.flush().map_err(|_| CustomScriptError::Runtime)
}

fn evaluate_script_function(
    source: &str,
    function_name: &str,
    context: Option<&ScriptContext>,
    first_argument: Option<&Value>,
) -> Result<Value, CustomScriptError> {
    let runtime = Runtime::new().map_err(|_| CustomScriptError::Runtime)?;
    runtime.set_memory_limit(JS_MEMORY_LIMIT);
    runtime.set_max_stack_size(JS_STACK_LIMIT);
    let deadline = Instant::now() + JS_EXECUTION_LIMIT;
    let interrupted = Arc::new(AtomicBool::new(false));
    let interrupted_handler = Arc::clone(&interrupted);
    runtime.set_interrupt_handler(Some(Box::new(move || {
        let timed_out = Instant::now() >= deadline;
        if timed_out {
            interrupted_handler.store(true, Ordering::Relaxed);
        }
        timed_out
    })));
    let context_handle = Context::builder()
        .with::<intrinsic::Eval>()
        .with::<intrinsic::Json>()
        .with::<intrinsic::RegExpCompiler>()
        .with::<intrinsic::RegExp>()
        .build(&runtime)
        .map_err(|_| CustomScriptError::Runtime)?;

    let context_json = context
        .map(serde_json::to_string)
        .transpose()
        .map_err(|_| CustomScriptError::Runtime)?;
    let first_argument_json = first_argument
        .map(serde_json::to_string)
        .transpose()
        .map_err(|_| CustomScriptError::Runtime)?;
    let result = context_handle.with(|ctx| {
        let globals = ctx.globals();
        for blocked in [
            "eval",
            "fetch",
            "process",
            "require",
            "Deno",
            "Bun",
            "Tauri",
            "__TAURI__",
            "WebSocket",
            "setTimeout",
            "setInterval",
        ] {
            globals
                .remove(blocked)
                .map_err(|_| CustomScriptError::Runtime)?;
        }
        let object: Object = globals
            .get("Object")
            .map_err(|_| CustomScriptError::Runtime)?;
        let freeze: Function = object
            .get("freeze")
            .map_err(|_| CustomScriptError::Runtime)?;
        let reject_non_finite: Function = ctx
            .eval(
                r#"(() => {
                  const numberValueOf = Function.prototype.call.bind(Number.prototype.valueOf);
                  return (_key, value) => {
                    if (typeof value === "number") {
                      if (value - value !== 0) throw null;
                      return value;
                    }
                    if (value !== null && typeof value === "object") {
                      try {
                        const unboxed = numberValueOf(value);
                        if (unboxed - unboxed !== 0) throw null;
                      } catch (error) {
                        if (error === null) throw null;
                      }
                    }
                    return value;
                  };
                })()"#,
            )
            .map_err(|_| CustomScriptError::Runtime)?;
        let script_object: Object = ctx.eval(source).map_err(|_| CustomScriptError::Script)?;
        let function: Function = script_object
            .get(function_name)
            .map_err(|_| CustomScriptError::MissingFunction)?;
        let parsed_context: Option<JsValue> = context_json
            .as_deref()
            .map(|context_json| {
                let context = ctx
                    .json_parse(context_json.as_bytes().to_vec())
                    .map_err(|_| CustomScriptError::Runtime)?;
                freeze
                    .call((context,))
                    .map_err(|_| CustomScriptError::Runtime)
            })
            .transpose()?;
        let parsed_argument = first_argument_json
            .as_deref()
            .map(|first_argument_json| {
                ctx.json_parse(first_argument_json.as_bytes().to_vec())
                    .map_err(|_| CustomScriptError::Runtime)
            })
            .transpose()?;
        let result: JsValue = match (parsed_argument, parsed_context) {
            (Some(first_argument), Some(context)) => function
                .call((first_argument, context))
                .map_err(|_| CustomScriptError::Script)?,
            (Some(first_argument), None) => function
                .call((first_argument,))
                .map_err(|_| CustomScriptError::Script)?,
            (None, Some(context)) => function
                .call((context,))
                .map_err(|_| CustomScriptError::Script)?,
            (None, None) => function.call(()).map_err(|_| CustomScriptError::Script)?,
        };
        let serialized = ctx
            .json_stringify_replacer(result, reject_non_finite)
            .map_err(|_| CustomScriptError::InvalidOutput)?;
        let serialized = serialized
            .ok_or(CustomScriptError::InvalidOutput)?
            .to_string()
            .map_err(|_| CustomScriptError::InvalidOutput)?;
        if serialized.len() > MAX_OUTPUT_JSON_BYTES {
            return Err(CustomScriptError::InvalidOutput);
        }
        serde_json::from_str(&serialized).map_err(|_| CustomScriptError::InvalidOutput)
    });
    if interrupted.load(Ordering::Relaxed) {
        return Err(CustomScriptError::Timeout);
    }
    result
}

fn materialize_request_plan(
    mut request: CustomAccountUsageRequestPlan,
    base_url: &str,
    api_key: &str,
) -> Result<CustomAccountUsageRequestPlan, CustomScriptError> {
    let base_url = base_url.trim_end_matches('/');
    request.url = materialize_request_value(
        &request.url,
        base_url,
        api_key,
        MAX_REQUEST_URL_BYTES,
        CustomScriptError::InvalidUrl,
    )?;
    for value in request.headers.values_mut() {
        *value = materialize_request_value(
            value,
            base_url,
            api_key,
            MAX_HEADER_VALUE_BYTES,
            CustomScriptError::InvalidHeaders,
        )?;
    }
    if let Some(body) = request.body.as_mut() {
        *body = materialize_request_value(
            body,
            base_url,
            api_key,
            MAX_REQUEST_BODY_BYTES,
            CustomScriptError::RequestTooLarge,
        )?;
    }
    Ok(request)
}

fn materialize_request_value(
    value: &str,
    base_url: &str,
    api_key: &str,
    max_bytes: usize,
    limit_error: CustomScriptError,
) -> Result<String, CustomScriptError> {
    let mut output = String::with_capacity(value.len().min(max_bytes));
    let mut remaining = value;
    while let Some((index, placeholder, replacement)) =
        next_request_placeholder(remaining, base_url, api_key)
    {
        push_bounded_request_value(&mut output, &remaining[..index], max_bytes, limit_error)?;
        push_bounded_request_value(&mut output, replacement, max_bytes, limit_error)?;
        remaining = &remaining[index + placeholder.len()..];
    }
    push_bounded_request_value(&mut output, remaining, max_bytes, limit_error)?;
    Ok(output)
}

fn push_bounded_request_value(
    output: &mut String,
    value: &str,
    max_bytes: usize,
    limit_error: CustomScriptError,
) -> Result<(), CustomScriptError> {
    if output
        .len()
        .checked_add(value.len())
        .is_none_or(|length| length > max_bytes)
    {
        return Err(limit_error);
    }
    output.push_str(value);
    Ok(())
}

fn next_request_placeholder<'a>(
    value: &'a str,
    base_url: &'a str,
    api_key: &'a str,
) -> Option<(usize, &'static str, &'a str)> {
    let base = value
        .find(SCRIPT_BASE_URL_PLACEHOLDER)
        .map(|index| (index, SCRIPT_BASE_URL_PLACEHOLDER, base_url));
    let key = value
        .find(SCRIPT_API_KEY_PLACEHOLDER)
        .map(|index| (index, SCRIPT_API_KEY_PLACEHOLDER, api_key));
    match (base, key) {
        (Some(base), Some(key)) => Some(if base.0 <= key.0 { base } else { key }),
        (Some(base), None) => Some(base),
        (None, Some(key)) => Some(key),
        (None, None) => None,
    }
}

fn redact_sensitive_json(value: &mut Value, sensitive_values: &[&str]) {
    match value {
        Value::String(text) => {
            *text = redact_sensitive_text(text, sensitive_values);
        }
        Value::Number(number) => {
            let rendered = number.to_string();
            if sensitive_values
                .iter()
                .any(|sensitive| !sensitive.is_empty() && *sensitive == rendered)
            {
                *value = Value::String("[REDACTED]".to_string());
            }
        }
        Value::Array(values) => {
            for value in values {
                redact_sensitive_json(value, sensitive_values);
            }
        }
        Value::Object(object) => {
            let values = std::mem::take(object);
            for (key, mut value) in values {
                redact_sensitive_json(&mut value, sensitive_values);
                object.insert(redact_sensitive_text(&key, sensitive_values), value);
            }
        }
        Value::Null | Value::Bool(_) => {}
    }
}

fn redact_sensitive_text(value: &str, sensitive_values: &[&str]) -> String {
    sensitive_values
        .iter()
        .filter(|sensitive| !sensitive.is_empty())
        .fold(value.to_string(), |redacted, sensitive| {
            redacted.replace(sensitive, "[REDACTED]")
        })
}

fn validate_and_build_request(
    config: &ProviderAccountUsageCustomConfig,
    base_url: &str,
    request: CustomAccountUsageRequestPlan,
) -> Result<reqwest::Request, CustomScriptError> {
    let url = reqwest::Url::parse(request.url.trim()).map_err(|_| CustomScriptError::InvalidUrl)?;
    if url.as_str().len() > MAX_REQUEST_URL_BYTES {
        return Err(CustomScriptError::InvalidUrl);
    }
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(CustomScriptError::InvalidUrl);
    }
    let allowed = crate::domain::provider_account_usage::custom_account_usage_network_origins(
        base_url,
        &config.allowed_origins,
    )
    .map_err(|_| CustomScriptError::InvalidBaseUrl)?
    .into_iter()
    .collect::<BTreeSet<_>>();
    if !allowed.contains(&url.origin().ascii_serialization()) {
        return Err(CustomScriptError::OriginForbidden);
    }

    let method = match request.method.trim().to_ascii_uppercase().as_str() {
        "GET" => reqwest::Method::GET,
        "POST" => reqwest::Method::POST,
        _ => return Err(CustomScriptError::InvalidMethod),
    };
    if request.headers.len() > MAX_REQUEST_HEADERS {
        return Err(CustomScriptError::InvalidHeaders);
    }
    let mut builder = reqwest::Client::new().request(method, url);
    for (name, value) in request.headers {
        if name.len() > MAX_HEADER_NAME_BYTES || value.len() > MAX_HEADER_VALUE_BYTES {
            return Err(CustomScriptError::InvalidHeaders);
        }
        let name = reqwest::header::HeaderName::from_bytes(name.as_bytes())
            .map_err(|_| CustomScriptError::InvalidHeaders)?;
        if is_forbidden_request_header(&name) {
            return Err(CustomScriptError::InvalidHeaders);
        }
        let value = reqwest::header::HeaderValue::from_str(&value)
            .map_err(|_| CustomScriptError::InvalidHeaders)?;
        builder = builder.header(name, value);
    }
    if let Some(body) = request.body {
        if body.len() > MAX_REQUEST_BODY_BYTES {
            return Err(CustomScriptError::RequestTooLarge);
        }
        builder = builder.body(body);
    }
    builder
        .build()
        .map_err(|_| CustomScriptError::InvalidRequest)
}

fn is_forbidden_request_header(name: &reqwest::header::HeaderName) -> bool {
    matches!(
        name.as_str(),
        "connection"
            | "cookie"
            | "content-length"
            | "host"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "proxy-connection"
            | "set-cookie"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
    )
}

fn normalize_custom_result(
    output: Value,
    fetched_at: i64,
    sensitive_values: &[&str],
) -> Result<ProviderAccountUsageResult, CustomScriptError> {
    let object = output.as_object().ok_or(CustomScriptError::InvalidOutput)?;
    let status = match object.get("status").and_then(Value::as_str) {
        Some("available") => ProviderAccountUsageStatus::Available,
        Some("zero_balance") => ProviderAccountUsageStatus::ZeroBalance,
        Some("expired") => ProviderAccountUsageStatus::Expired,
        Some("auth_failed") => ProviderAccountUsageStatus::AuthFailed,
        Some("query_failed") => ProviderAccountUsageStatus::QueryFailed,
        Some("configuration_required") => ProviderAccountUsageStatus::ConfigurationRequired,
        _ => return Err(CustomScriptError::InvalidOutput),
    };
    if matches!(
        status,
        ProviderAccountUsageStatus::AuthFailed
            | ProviderAccountUsageStatus::QueryFailed
            | ProviderAccountUsageStatus::ConfigurationRequired
    ) {
        let mut result = ProviderAccountUsageResult::fetched(
            ProviderAccountUsageAdapterKind::Custom,
            status,
            fetched_at,
        );
        result.message = Some(
            match status {
                ProviderAccountUsageStatus::AuthFailed => "自定义账户用量接口认证失败",
                ProviderAccountUsageStatus::QueryFailed => "自定义账户用量查询失败",
                ProviderAccountUsageStatus::ConfigurationRequired => "自定义账户用量配置不完整",
                _ => unreachable!("only failed custom account usage statuses reach this branch"),
            }
            .to_string(),
        );
        return Ok(result);
    }
    let message =
        optional_text(object.get("message"), sensitive_values)?.filter(|value| !value.is_empty());
    let mut result = ProviderAccountUsageResult {
        adapter_kind: Some(ProviderAccountUsageAdapterKind::Custom),
        status,
        freshness: ProviderAccountUsageFreshness::Fresh,
        plan_name: optional_text(object.get("planName"), sensitive_values)?,
        balance: optional_number(object.get("balance"))?,
        plan_remaining: optional_number(object.get("planRemaining"))?,
        used: optional_number(object.get("used"))?,
        total: optional_number(object.get("total"))?,
        unit: optional_text(object.get("unit"), sensitive_values)?,
        unit_note: optional_text(object.get("unitNote"), sensitive_values)?,
        daily_used: optional_number(object.get("dailyUsed"))?,
        daily_total: optional_number(object.get("dailyTotal"))?,
        weekly_used: optional_number(object.get("weeklyUsed"))?,
        weekly_total: optional_number(object.get("weeklyTotal"))?,
        monthly_used: optional_number(object.get("monthlyUsed"))?,
        monthly_total: optional_number(object.get("monthlyTotal"))?,
        expires_at: optional_integer(object.get("expiresAt"))?,
        last_fetched_at: Some(fetched_at),
        message,
    };
    if result
        .message
        .as_ref()
        .is_some_and(|value| value.is_empty())
    {
        result.message = None;
    }
    Ok(result)
}

fn optional_number(value: Option<&Value>) -> Result<Option<f64>, CustomScriptError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_f64()
            .filter(|value| value.is_finite())
            .map(Some)
            .ok_or(CustomScriptError::InvalidOutput),
    }
}

fn optional_integer(value: Option<&Value>) -> Result<Option<i64>, CustomScriptError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_i64()
            .map(Some)
            .ok_or(CustomScriptError::InvalidOutput),
    }
}

fn optional_text(
    value: Option<&Value>,
    sensitive_values: &[&str],
) -> Result<Option<String>, CustomScriptError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(value) => {
            let value = value.as_str().ok_or(CustomScriptError::InvalidOutput)?;
            if value.chars().count() > 96 {
                return Err(CustomScriptError::InvalidOutput);
            }
            Ok(Some(redact_sensitive_text(value, sensitive_values)))
        }
    }
}

fn failed_result(fetched_at: i64, error: CustomScriptError) -> ProviderAccountUsageResult {
    let status = if error == CustomScriptError::Auth {
        ProviderAccountUsageStatus::AuthFailed
    } else {
        ProviderAccountUsageStatus::QueryFailed
    };
    let mut result = ProviderAccountUsageResult::fetched(
        ProviderAccountUsageAdapterKind::Custom,
        status,
        fetched_at,
    );
    result.message = Some(error.message().to_string());
    result
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum CustomScriptError {
    Busy,
    Runtime,
    Script,
    MissingFunction,
    Timeout,
    InvalidRequest,
    InvalidUrl,
    InvalidBaseUrl,
    OriginForbidden,
    InvalidMethod,
    InvalidHeaders,
    RequestTooLarge,
    Http,
    Redirect,
    HttpStatus,
    Auth,
    ResponseTooLarge,
    InvalidJson,
    InvalidOutput,
}

impl CustomScriptError {
    fn message(self) -> &'static str {
        match self {
            Self::Busy => "自定义账户用量查询并发数已达上限",
            Self::Runtime | Self::Script => "自定义账户用量脚本执行失败",
            Self::MissingFunction => "自定义账户用量脚本缺少 request 或 parse 函数",
            Self::Timeout => "自定义账户用量脚本执行超时",
            Self::InvalidRequest => "自定义账户用量脚本返回了无效请求",
            Self::InvalidUrl => "自定义账户用量脚本返回了无效 HTTPS URL",
            Self::InvalidBaseUrl => "供应商 Base URL 必须是有效 HTTPS URL",
            Self::OriginForbidden => "自定义账户用量请求目标未获得允许",
            Self::InvalidMethod => "自定义账户用量请求仅支持 GET 或 POST",
            Self::InvalidHeaders => "自定义账户用量请求头无效或超过限制",
            Self::RequestTooLarge => "自定义账户用量请求体超过限制",
            Self::Http => "自定义账户用量请求失败",
            Self::Redirect => "自定义账户用量请求不允许重定向",
            Self::HttpStatus => "自定义账户用量接口返回非成功状态",
            Self::Auth => "自定义账户用量接口认证失败",
            Self::ResponseTooLarge => "自定义账户用量响应超过限制或读取失败",
            Self::InvalidJson => "自定义账户用量接口返回了无效 JSON",
            Self::InvalidOutput => "自定义账户用量脚本返回了无效结果",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn config(script: &str) -> ProviderAccountUsageCustomConfig {
        ProviderAccountUsageCustomConfig {
            script: script.to_string(),
            allowed_origins: vec!["https://usage.example.test".to_string()],
            timeout_seconds: 5,
            enabled: true,
            permission_base_origin: Some("https://api.example.test".to_string()),
            permission_fingerprint: None,
        }
    }

    async fn response_from_raw(raw_response: &'static [u8]) -> reqwest::Response {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("bind test server");
        let address = listener.local_addr().expect("test server address");
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept request");
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request).await;
            stream
                .write_all(raw_response)
                .await
                .expect("write response");
        });

        let response = reqwest::Client::builder()
            .no_proxy()
            .build()
            .expect("test client")
            .get(format!("http://{address}"))
            .send()
            .await
            .expect("send test request");
        server.await.expect("test server task");
        response
    }

    #[test]
    fn evaluates_request_without_host_capabilities() {
        let script = r#"({
          request: (ctx) => ({
            url: ctx.baseUrl.replace(/\/$/, "") + "/v1/usage",
            method: "GET",
            headers: { Authorization: `Bearer ${ctx.apiKey}` },
            body: typeof fetch + ":" + typeof process + ":" + typeof require + ":" + typeof eval
          }),
          parse: () => ({ status: "available" })
        })"#;
        let context = ScriptContext {
            api_key: SCRIPT_API_KEY_PLACEHOLDER.to_string(),
            base_url: SCRIPT_BASE_URL_PLACEHOLDER.to_string(),
        };
        let value = evaluate_script_function(script, "request", Some(&context), None).unwrap();
        assert_eq!(value["body"], "undefined:undefined:undefined:undefined");
        assert_eq!(
            value["headers"]["Authorization"],
            format!("Bearer {SCRIPT_API_KEY_PLACEHOLDER}")
        );
        assert!(!value.to_string().contains("SYNTHETIC_SECRET"));

        let request: CustomAccountUsageRequestPlan = serde_json::from_value(value).unwrap();
        let request =
            materialize_request_plan(request, "https://usage.example.test///", "SYNTHETIC_SECRET")
                .unwrap();
        assert_eq!(request.url, "https://usage.example.test/v1/usage");
        assert_eq!(
            request.headers.get("Authorization").map(String::as_str),
            Some("Bearer SYNTHETIC_SECRET")
        );

        let collision = materialize_request_value(
            SCRIPT_BASE_URL_PLACEHOLDER,
            &format!("https://usage.example.test/{SCRIPT_API_KEY_PLACEHOLDER}"),
            "SYNTHETIC_SECRET",
            MAX_REQUEST_URL_BYTES,
            CustomScriptError::InvalidUrl,
        )
        .unwrap();
        assert_eq!(
            collision,
            format!("https://usage.example.test/{SCRIPT_API_KEY_PLACEHOLDER}")
        );
    }

    #[test]
    fn interrupts_infinite_script() {
        let context = ScriptContext {
            api_key: "SYNTHETIC_SECRET".to_string(),
            base_url: "https://api.example.test/v1".to_string(),
        };
        let error = evaluate_script_function(
            "({ request: () => { while (true) {} }, parse: () => ({}) })",
            "request",
            Some(&context),
            None,
        )
        .unwrap_err();
        assert_eq!(error, CustomScriptError::Timeout);
    }

    #[tokio::test]
    async fn worker_hard_kills_uninterruptible_native_array_loop() {
        let response = serde_json::json!({"status": 200, "data": {}});
        let attack_config = config(
            "({ request: () => Array.prototype.sort.call({ length: Number.MAX_SAFE_INTEGER }), parse: () => ({}) })",
        );
        for _ in 0..MAX_CONCURRENT_CUSTOM_REQUESTS {
            let attack = execute_custom_account_usage_inner(
                &attack_config,
                "https://api.example.test/v1",
                "SYNTHETIC_SECRET",
                100,
            );
            let error = tokio::time::timeout(Duration::from_secs(10), attack)
                .await
                .expect("the worker process must be reclaimable")
                .expect_err("the native array loop must not complete successfully");
            assert_eq!(error, CustomScriptError::Timeout);
        }

        let sorted = tokio::time::timeout(
            Duration::from_secs(10),
            evaluate_script_function_in_worker_process(
                "({ parse: () => [3, 1, 2].sort((left, right) => left - right) })".to_string(),
                ScriptFunction::Parse,
                None,
                Some(response),
            ),
        )
        .await
        .expect("a replacement worker must start after the timed-out worker")
        .expect("normal small-array calculations must remain available");
        assert_eq!(sorted, serde_json::json!([1, 2, 3]));

        let payload_length = MAX_OUTPUT_JSON_BYTES - 2;
        let large_output = tokio::time::timeout(
            Duration::from_secs(10),
            evaluate_script_function_in_worker_process(
                format!("({{ parse: () => 'x'.repeat({payload_length}) }})"),
                ScriptFunction::Parse,
                None,
                Some(serde_json::json!({"status": 200, "data": {}})),
            ),
        )
        .await
        .expect("the maximum normal output must fit within the worker deadline")
        .expect("the maximum normal output must remain valid");
        assert_eq!(large_output.as_str().map(str::len), Some(payload_length));

        let recovery_config = config(
            "({ request: () => ({ url: 'http://invalid.example.test', method: 'GET' }), parse: () => ({}) })",
        );
        let recovery_error = execute_custom_account_usage_inner(
            &recovery_config,
            "https://api.example.test/v1",
            "SYNTHETIC_SECRET",
            101,
        )
        .await
        .expect_err("the recovery request is intentionally invalid");
        assert_eq!(recovery_error, CustomScriptError::InvalidUrl);
    }

    #[test]
    fn request_validation_enforces_https_origin_and_headers() {
        let allowed = config("({})");
        let request = CustomAccountUsageRequestPlan {
            url: "https://usage.example.test/account".to_string(),
            method: "POST".to_string(),
            headers: [("Authorization".to_string(), "Bearer x".to_string())]
                .into_iter()
                .collect(),
            body: Some("{}".to_string()),
        };
        let built =
            validate_and_build_request(&allowed, "https://api.example.test/v1", request).unwrap();
        assert_eq!(built.method(), reqwest::Method::POST);
        assert_eq!(
            built.url().origin().ascii_serialization(),
            "https://usage.example.test"
        );

        let forbidden = CustomAccountUsageRequestPlan {
            url: "https://other.example.test/account".to_string(),
            method: "GET".to_string(),
            headers: Default::default(),
            body: None,
        };
        assert_eq!(
            validate_and_build_request(&allowed, "https://api.example.test/v1", forbidden,)
                .unwrap_err(),
            CustomScriptError::OriginForbidden
        );

        for header in [
            "Connection",
            "Cookie",
            "Content-Length",
            "Host",
            "Keep-Alive",
            "Proxy-Authenticate",
            "Proxy-Authorization",
            "Proxy-Connection",
            "Set-Cookie",
            "TE",
            "Trailer",
            "Transfer-Encoding",
            "Upgrade",
        ] {
            let request = CustomAccountUsageRequestPlan {
                url: "https://usage.example.test/account".to_string(),
                method: "GET".to_string(),
                headers: [(header.to_string(), "synthetic".to_string())]
                    .into_iter()
                    .collect(),
                body: None,
            };
            assert_eq!(
                validate_and_build_request(&allowed, "https://api.example.test/v1", request)
                    .unwrap_err(),
                CustomScriptError::InvalidHeaders,
                "header {header} must be rejected"
            );
        }
    }

    #[test]
    fn request_placeholder_expansion_is_bounded_before_allocation() {
        let repeated_placeholders = SCRIPT_API_KEY_PLACEHOLDER.repeat(3);
        let oversized_key = "x".repeat(MAX_REQUEST_BODY_BYTES / 2);
        let error = materialize_request_value(
            &repeated_placeholders,
            "https://api.example.test",
            &oversized_key,
            MAX_REQUEST_BODY_BYTES,
            CustomScriptError::RequestTooLarge,
        )
        .unwrap_err();
        assert_eq!(error, CustomScriptError::RequestTooLarge);
    }

    #[test]
    fn request_validation_rejects_percent_encoded_url_over_wire_limit() {
        let raw_url = format!(
            "https://usage.example.test/{}",
            "界".repeat(MAX_REQUEST_URL_BYTES / 4)
        );
        assert!(raw_url.len() < MAX_REQUEST_URL_BYTES);
        let parsed = reqwest::Url::parse(&raw_url).expect("valid Unicode URL");
        assert!(parsed.as_str().len() > MAX_REQUEST_URL_BYTES);

        let request = CustomAccountUsageRequestPlan {
            url: raw_url,
            method: "GET".to_string(),
            headers: Default::default(),
            body: None,
        };
        assert_eq!(
            validate_and_build_request(&config("({})"), "https://api.example.test/v1", request,)
                .unwrap_err(),
            CustomScriptError::InvalidUrl
        );
    }

    #[test]
    fn parse_output_is_strict_and_normalized() {
        let script = r#"({
          request: () => ({}),
          parse: (response) => ({
            status: "available",
            balance: response.data.balance,
            used: 4,
            total: 10,
            unit: "USD",
            expiresAt: 123
          })
        })"#;
        let response = serde_json::to_value(ScriptResponse {
            status: 200,
            data: &serde_json::json!({"balance": 6}),
        })
        .unwrap();
        let output = evaluate_script_function(script, "parse", None, Some(&response)).unwrap();
        let result = normalize_custom_result(output, 100, &[]).unwrap();
        assert_eq!(result.status, ProviderAccountUsageStatus::Available);
        assert_eq!(result.balance, Some(6.0));
        assert_eq!(result.expires_at, Some(123));
    }

    #[test]
    fn parse_has_no_credential_context_and_rejects_non_finite_numbers() {
        let response = serde_json::json!({"status": 200, "data": {}});
        let no_context = evaluate_script_function(
            "({ parse: (_response, ctx) => ({ status: 'available', message: typeof ctx }) })",
            "parse",
            None,
            Some(&response),
        )
        .unwrap();
        assert_eq!(no_context["message"], "undefined");

        let non_finite = evaluate_script_function(
            "({ parse: () => ({ status: 'available', balance: Number.POSITIVE_INFINITY }) })",
            "parse",
            None,
            Some(&response),
        )
        .unwrap_err();
        assert_eq!(non_finite, CustomScriptError::InvalidOutput);
    }

    #[test]
    fn script_cannot_replace_json_parse_for_backend_arguments() {
        let script = r#"(() => {
          JSON.parse = () => ({
            apiKey: "FORGED_KEY",
            baseUrl: "https://forged.example.test",
            status: 599,
            data: { balance: 999 }
          });
          return {
            request: (ctx) => ({
              url: ctx.baseUrl + "/v1/usage",
              method: "POST",
              headers: { Authorization: `Bearer ${ctx.apiKey}` },
              body: ctx.apiKey
            }),
            parse: (response) => ({
              status: "available",
              balance: response.data.balance
            })
          };
        })()"#;
        let context = ScriptContext {
            api_key: SCRIPT_API_KEY_PLACEHOLDER.to_string(),
            base_url: SCRIPT_BASE_URL_PLACEHOLDER.to_string(),
        };

        let request = evaluate_script_function(script, "request", Some(&context), None)
            .expect("backend context must use the intrinsic JSON parser");
        assert_eq!(
            request["url"],
            format!("{SCRIPT_BASE_URL_PLACEHOLDER}/v1/usage")
        );
        assert_eq!(request["body"], SCRIPT_API_KEY_PLACEHOLDER);
        assert_eq!(
            request["headers"]["Authorization"],
            format!("Bearer {SCRIPT_API_KEY_PLACEHOLDER}")
        );

        let response = serde_json::json!({"status": 200, "data": {"balance": 7}});
        let output = evaluate_script_function(script, "parse", None, Some(&response))
            .expect("backend response must use the intrinsic JSON parser");
        assert_eq!(output["balance"], 7);
    }

    #[test]
    fn script_cannot_replace_json_stringify_for_backend_output() {
        let script = r#"(() => {
          JSON.stringify = () => '{"status":"query_failed"}';
          return {
            parse: () => ({ status: "available", balance: 7 })
          };
        })()"#;
        let response = serde_json::json!({"status": 200, "data": {}});

        let output = evaluate_script_function(script, "parse", None, Some(&response))
            .expect("backend output must use the intrinsic JSON serializer");
        assert_eq!(output["status"], "available");
        assert_eq!(output["balance"], 7);
    }

    #[test]
    fn replacing_number_is_finite_cannot_hide_non_finite_output() {
        let response = serde_json::json!({"status": 200, "data": {}});
        for expression in ["0 / 0", "1 / 0", "-1 / 0"] {
            let script = format!(
                r#"({{
                  parse: () => {{
                    Number.isFinite = () => true;
                    return {{ status: "available", balance: {expression} }};
                  }}
                }})"#
            );

            let error = evaluate_script_function(&script, "parse", None, Some(&response))
                .expect_err("NaN and positive/negative infinity must fail closed");
            assert_eq!(
                error,
                CustomScriptError::InvalidOutput,
                "expression {expression} must be rejected"
            );
        }
    }

    #[test]
    fn boxed_non_finite_numbers_are_rejected_before_json_coerces_them_to_null() {
        let response = serde_json::json!({"status": 200, "data": {}});
        for expression in [
            "new Number(0 / 0)",
            "new Number(1 / 0)",
            "new Number(-1 / 0)",
        ] {
            let script = format!(
                r#"({{
                  parse: () => {{
                    Number.prototype.valueOf = () => 0;
                    return {{ status: "available", balance: {expression} }};
                  }}
                }})"#
            );

            let error = evaluate_script_function(&script, "parse", None, Some(&response))
                .expect_err("boxed NaN and positive/negative infinity must fail closed");
            assert_eq!(
                error,
                CustomScriptError::InvalidOutput,
                "expression {expression} must be rejected"
            );
        }
    }

    #[test]
    fn failed_script_status_uses_stable_local_message_without_partial_values() {
        let result = normalize_custom_result(
            serde_json::json!({
                "status": "query_failed",
                "balance": 10,
                "used": 2,
                "total": 12,
                "unit": "USD",
                "message": { "upstream": "SYNTHETIC_SECRET" }
            }),
            100,
            &["SYNTHETIC_SECRET"],
        )
        .unwrap();
        assert_eq!(result.status, ProviderAccountUsageStatus::QueryFailed);
        assert_eq!(result.balance, None);
        assert_eq!(result.used, None);
        assert_eq!(result.total, None);
        assert_eq!(result.unit, None);
        assert_eq!(result.message.as_deref(), Some("自定义账户用量查询失败"));
        assert!(!result
            .message
            .as_deref()
            .unwrap_or_default()
            .contains("SYNTHETIC_SECRET"));
    }

    #[test]
    fn failed_script_status_messages_are_local_and_status_specific() {
        for (script_status, expected_status, expected_message) in [
            (
                "auth_failed",
                ProviderAccountUsageStatus::AuthFailed,
                "自定义账户用量接口认证失败",
            ),
            (
                "query_failed",
                ProviderAccountUsageStatus::QueryFailed,
                "自定义账户用量查询失败",
            ),
            (
                "configuration_required",
                ProviderAccountUsageStatus::ConfigurationRequired,
                "自定义账户用量配置不完整",
            ),
        ] {
            let result = normalize_custom_result(
                serde_json::json!({
                    "status": script_status,
                    "message": "upstream detail"
                }),
                100,
                &[],
            )
            .unwrap();
            assert_eq!(result.status, expected_status);
            assert_eq!(result.message.as_deref(), Some(expected_message));
        }
    }

    #[tokio::test]
    async fn response_json_rejects_invalid_utf8_instead_of_replacing_it() {
        let response = response_from_raw(
            b"HTTP/1.1 200 OK\r\nContent-Length: 9\r\nConnection: close\r\n\r\n{\"x\":\"\xff\"}",
        )
        .await;
        let body = read_response_body_with_limit(response)
            .await
            .expect("bounded body");
        assert_eq!(body, b"{\"x\":\"\xff\"}");
        let error = parse_response_json(&body).unwrap_err();
        assert_eq!(error, CustomScriptError::InvalidJson);
    }

    #[test]
    fn response_and_normalized_text_redact_exact_credentials() {
        let api_key = "SYNTHETIC_SECRET";
        let base_url = "https://api.example.test/v1";
        let mut response = serde_json::json!({
            "echo": format!("Bearer {api_key}"),
            "url": format!("{base_url}/usage"),
            "nested": [{api_key: base_url}]
        });
        redact_sensitive_json(&mut response, &[api_key, base_url]);
        let response_text = response.to_string();
        assert!(!response_text.contains(api_key));
        assert!(!response_text.contains(base_url));

        let result = normalize_custom_result(
            serde_json::json!({
                "status": "available",
                "planName": format!("plan-{api_key}"),
                "message": format!("from {base_url}")
            }),
            100,
            &[api_key, base_url],
        )
        .unwrap();
        assert_eq!(result.plan_name.as_deref(), Some("plan-[REDACTED]"));
        assert_eq!(result.message.as_deref(), Some("from [REDACTED]"));
    }
}
