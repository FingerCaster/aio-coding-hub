//! Unified streaming translation wrapper.
//!
//! `BridgeStream<S>` wraps an upstream byte stream and translates SSE events
//! through the Outbound → IR → Inbound pipeline.  When `active` is false the
//! stream is a zero-cost passthrough.

use super::traits::{BridgeContext, BridgeError};
use crate::gateway::proxy::sse::{find_sse_event_end, parse_sse_frame};
use axum::body::Bytes;
use futures_core::Stream;
use serde_json::Value;
use std::collections::VecDeque;
use std::pin::Pin;
use std::task::{Context, Poll};

const MAX_BRIDGE_SSE_FRAME_BUFFER_BYTES: usize = 1024 * 1024;
const BRIDGE_SSE_FRAME_TOO_LARGE: &[u8] = concat!(
    "event: error\n",
    "data: {\"type\":\"error\",\"error\":{\"type\":\"invalid_request_error\",\"message\":\"bridge_sse_frame_too_large\"}}\n\n"
)
.as_bytes();

/// Generic stream wrapper that translates upstream SSE events via IR.
pub(crate) struct BridgeStream<S>
where
    S: Stream<Item = Result<Bytes, reqwest::Error>> + Unpin,
{
    upstream: S,
    active: bool,
    translator: Option<StreamTranslatorOwned>,
    ctx: BridgeContext,
    /// Buffered output frames ready to be yielded.
    buffer: VecDeque<Bytes>,
    /// Accumulator for partial SSE lines from the upstream.
    line_buf: Vec<u8>,
    terminated: bool,
}

/// Owned version of StreamTranslator that doesn't borrow from Bridge.
///
/// Because the Bridge is consumed when creating the stream pipeline, we need
/// to own the Inbound/Outbound trait objects directly.
pub(crate) struct StreamTranslatorOwned {
    pub inbound: Box<dyn super::traits::Inbound>,
    pub outbound: Box<dyn super::traits::Outbound>,
    pub state: super::traits::StreamState,
}

impl StreamTranslatorOwned {
    /// Translate a single upstream SSE event into client-facing SSE bytes.
    pub fn translate_event(
        &mut self,
        event_type: &str,
        data: &Value,
        ctx: &BridgeContext,
    ) -> Result<Vec<Bytes>, BridgeError> {
        self.state.enable_reasoning_to_thinking = ctx.cx2cc_settings.enable_reasoning_to_thinking;
        let ir_chunks = self
            .outbound
            .sse_event_to_ir(event_type, data, &mut self.state)?;
        let mut output = Vec::new();
        for chunk in &ir_chunks {
            let mut frames = self.inbound.ir_chunk_to_sse(chunk, ctx)?;
            output.append(&mut frames);
        }
        Ok(output)
    }
}

impl<S> BridgeStream<S>
where
    S: Stream<Item = Result<Bytes, reqwest::Error>> + Unpin,
{
    /// Convenience constructor for CX2CC translation.
    ///
    /// When `active` is false the stream is a zero-cost passthrough.
    /// When `active` is true a fresh CX2CC bridge translator is created.
    pub fn for_cx2cc(
        upstream: S,
        active: bool,
        requested_model: Option<String>,
        cx2cc_settings: crate::gateway::proxy::cx2cc::settings::Cx2ccSettings,
    ) -> Self {
        Self::for_bridge_type(
            upstream,
            active.then_some("cx2cc"),
            requested_model,
            cx2cc_settings,
        )
    }

    pub fn for_bridge_type(
        upstream: S,
        bridge_type: Option<&str>,
        requested_model: Option<String>,
        cx2cc_settings: crate::gateway::proxy::cx2cc::settings::Cx2ccSettings,
    ) -> Self {
        let Some(bridge_type) = bridge_type else {
            let dummy_ctx = BridgeContext {
                claude_models: crate::domain::providers::ClaudeModels::default(),
                cx2cc_settings: crate::gateway::proxy::cx2cc::settings::Cx2ccSettings::default(),
                requested_model: None,
                mapped_model: None,
                stream_requested: false,
                is_chatgpt_backend: false,
            };
            return Self::new(upstream, false, None, dummy_ctx);
        };

        let bridge = match super::registry::get_bridge(bridge_type) {
            Some(b) => b,
            None => {
                tracing::error!(
                    bridge_type,
                    "bridge not found in registry; failing stream closed"
                );
                let ctx = BridgeContext {
                    claude_models: crate::domain::providers::ClaudeModels::default(),
                    cx2cc_settings,
                    requested_model,
                    mapped_model: None,
                    stream_requested: true,
                    is_chatgpt_backend: false,
                };
                let mut stream = Self::new(upstream, true, None, ctx);
                stream.terminate_registry_miss(bridge_type);
                return stream;
            }
        };
        let translator = StreamTranslatorOwned {
            inbound: bridge.inbound,
            outbound: bridge.outbound,
            state: super::traits::StreamState::default(),
        };
        let ctx = BridgeContext {
            claude_models: crate::domain::providers::ClaudeModels::default(),
            cx2cc_settings,
            requested_model,
            mapped_model: None,
            stream_requested: true,
            is_chatgpt_backend: false,
        };
        Self::new(upstream, true, Some(translator), ctx)
    }

    /// Create a new bridge stream.
    ///
    /// When `active` is false, the stream simply forwards upstream bytes
    /// without any processing.
    pub fn new(
        upstream: S,
        active: bool,
        translator: Option<StreamTranslatorOwned>,
        ctx: BridgeContext,
    ) -> Self {
        Self {
            upstream,
            active,
            translator,
            ctx,
            buffer: VecDeque::new(),
            line_buf: Vec::new(),
            terminated: false,
        }
    }

    fn terminate_oversized_frame(&mut self) {
        tracing::warn!(
            max_bytes = MAX_BRIDGE_SSE_FRAME_BUFFER_BYTES,
            "bridge stream SSE frame exceeded maximum buffered size"
        );
        self.line_buf.clear();
        self.buffer
            .push_back(Bytes::from_static(BRIDGE_SSE_FRAME_TOO_LARGE));
        self.terminated = true;
    }

    fn terminate_registry_miss(&mut self, bridge_type: &str) {
        let payload = serde_json::json!({
            "type": "error",
            "error": {
                "type": "invalid_request_error",
                "code": crate::gateway::proxy::GatewayErrorCode::BridgeUnsupportedFeature.as_str(),
                "message": format!("bridge is not registered: {bridge_type}")
            }
        });
        let data = serde_json::to_string(&payload).unwrap_or_else(|_| {
            "{\"type\":\"error\",\"error\":{\"type\":\"invalid_request_error\",\"message\":\"bridge is not registered\"}}".to_string()
        });
        self.line_buf.clear();
        self.buffer
            .push_back(Bytes::from(format!("event: error\ndata: {data}\n\n")));
        self.terminated = true;
    }

    fn terminate_translation_error(&mut self, err: BridgeError) {
        let payload = serde_json::json!({
            "type": "error",
            "error": {
                "type": "invalid_request_error",
                "code": crate::gateway::proxy::GatewayErrorCode::BridgeUnsupportedFeature.as_str(),
                "message": format!("bridge stream translation failed: {err}")
            }
        });
        let data = serde_json::to_string(&payload).unwrap_or_else(|_| {
            "{\"type\":\"error\",\"error\":{\"type\":\"invalid_request_error\",\"message\":\"bridge stream translation failed\"}}".to_string()
        });
        self.line_buf.clear();
        self.buffer
            .push_back(Bytes::from(format!("event: error\ndata: {data}\n\n")));
        self.terminated = true;
    }

    /// Process a raw byte chunk from upstream: split into SSE frames, translate
    /// each, and push the results into `self.buffer`.
    fn process_chunk(&mut self, bytes: &[u8]) {
        if self.translator.is_none() || self.terminated {
            return;
        }

        let mut remaining = bytes;
        while !remaining.is_empty() && !self.terminated {
            let available = MAX_BRIDGE_SSE_FRAME_BUFFER_BYTES.saturating_sub(self.line_buf.len());
            if available == 0 {
                self.terminate_oversized_frame();
                return;
            }

            let take = remaining.len().min(available);
            self.line_buf.extend_from_slice(&remaining[..take]);
            remaining = &remaining[take..];
            self.process_complete_frames();

            if !remaining.is_empty() && self.line_buf.len() >= MAX_BRIDGE_SSE_FRAME_BUFFER_BYTES {
                self.terminate_oversized_frame();
                return;
            }
        }

        if self.line_buf.len() >= MAX_BRIDGE_SSE_FRAME_BUFFER_BYTES {
            self.terminate_oversized_frame();
        }
    }

    fn process_complete_frames(&mut self) {
        // SSE frames are delimited by `\n\n` or `\r\n\r\n`.
        while let Some(end) = find_sse_event_end(&self.line_buf) {
            let frame_bytes: Vec<u8> = self.line_buf.drain(..end).collect();
            let frame_str = match std::str::from_utf8(&frame_bytes) {
                Ok(s) => s,
                Err(_) => continue,
            };

            if let Some((event_type, data)) = parse_sse_frame(frame_str) {
                let Some(translator) = self.translator.as_mut() else {
                    return;
                };
                match translator.translate_event(&event_type, &data, &self.ctx) {
                    Ok(frames) => {
                        for f in frames {
                            self.buffer.push_back(f);
                        }
                    }
                    Err(e) => {
                        tracing::warn!("bridge stream translation error: {e}");
                        self.terminate_translation_error(e);
                        return;
                    }
                }
            }
        }
    }
}

impl<S> Stream for BridgeStream<S>
where
    S: Stream<Item = Result<Bytes, reqwest::Error>> + Unpin,
{
    type Item = Result<Bytes, reqwest::Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();

        if !this.active {
            return Pin::new(&mut this.upstream).poll_next(cx);
        }

        loop {
            // Yield buffered frames first.
            if let Some(frame) = this.buffer.pop_front() {
                return Poll::Ready(Some(Ok(frame)));
            }

            if this.terminated {
                return Poll::Ready(None);
            }

            // Poll upstream for more data.
            match Pin::new(&mut this.upstream).poll_next(cx) {
                Poll::Ready(Some(Ok(bytes))) => {
                    this.process_chunk(&bytes);
                    // Loop back to check buffer — avoids spurious wakeup.
                }
                Poll::Ready(Some(Err(e))) => return Poll::Ready(Some(Err(e))),
                Poll::Ready(None) => return Poll::Ready(None),
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    struct MockStream {
        items: VecDeque<Result<Bytes, reqwest::Error>>,
    }

    impl MockStream {
        fn new(items: Vec<Result<Bytes, reqwest::Error>>) -> Self {
            Self {
                items: items.into_iter().collect(),
            }
        }
    }

    impl Stream for MockStream {
        type Item = Result<Bytes, reqwest::Error>;

        fn poll_next(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
            Poll::Ready(self.items.pop_front())
        }
    }

    #[test]
    fn bridge_stream_emits_error_and_stops_when_frame_buffer_exceeds_limit() {
        let oversized = Bytes::from(vec![b'a'; MAX_BRIDGE_SSE_FRAME_BUFFER_BYTES + 1]);
        let mut stream = BridgeStream::for_cx2cc(
            MockStream::new(vec![Ok(oversized)]),
            true,
            None,
            crate::gateway::proxy::cx2cc::settings::Cx2ccSettings::default(),
        );
        let waker = std::task::Waker::noop();
        let mut cx = Context::from_waker(waker);

        let first = Pin::new(&mut stream).poll_next(&mut cx);
        let Poll::Ready(Some(Ok(frame))) = first else {
            panic!("expected bridge error frame, got {first:?}");
        };
        let text = std::str::from_utf8(frame.as_ref()).expect("utf-8 error frame");
        assert!(text.contains("event: error"));
        assert!(text.contains("bridge_sse_frame_too_large"));

        assert!(matches!(
            Pin::new(&mut stream).poll_next(&mut cx),
            Poll::Ready(None)
        ));
    }

    #[test]
    fn bridge_stream_emits_error_instead_of_passthrough_for_unknown_bridge_type() {
        let upstream_frame = Bytes::from_static(b"data: should-not-pass-through\n\n");
        let mut stream = BridgeStream::for_bridge_type(
            MockStream::new(vec![Ok(upstream_frame)]),
            Some("missing_bridge"),
            None,
            crate::gateway::proxy::cx2cc::settings::Cx2ccSettings::default(),
        );
        let waker = std::task::Waker::noop();
        let mut cx = Context::from_waker(waker);

        let first = Pin::new(&mut stream).poll_next(&mut cx);
        let Poll::Ready(Some(Ok(frame))) = first else {
            panic!("expected bridge registry error frame, got {first:?}");
        };
        let text = std::str::from_utf8(frame.as_ref()).expect("utf-8 error frame");
        assert!(text.contains("event: error"));
        assert!(text.contains("GW_BRIDGE_UNSUPPORTED_FEATURE"));
        assert!(text.contains("missing_bridge"));
        assert!(!text.contains("should-not-pass-through"));

        assert!(matches!(
            Pin::new(&mut stream).poll_next(&mut cx),
            Poll::Ready(None)
        ));
    }
}
