//! One-time provenance for the single trusted CX2CC gateway reentry.

use axum::http::Method;
use rand::RngCore as _;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub(in crate::gateway) const INTERNAL_REENTRY_HEADER: &str = "x-aio-internal-reentry-nonce";

const DEFAULT_NONCE_TTL: Duration = Duration::from_secs(10);
const DEFAULT_NONCE_CAPACITY: usize = 1024;
const NONCE_BYTES: usize = 32;
const NONCE_GENERATION_ATTEMPTS: usize = 4;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::gateway) struct TrustedInternalReentry {
    pub(in crate::gateway) bridge_provider_id: i64,
    pub(in crate::gateway) origin_trace_id: String,
}

#[derive(Debug)]
struct PendingInternalReentry {
    bridge_provider_id: i64,
    origin_trace_id: String,
    issued_at: Instant,
}

#[derive(Debug)]
pub(in crate::gateway) struct InternalReentryRegistry {
    pending: Mutex<HashMap<String, PendingInternalReentry>>,
    ttl: Duration,
    capacity: usize,
}

impl Default for InternalReentryRegistry {
    fn default() -> Self {
        Self::new(DEFAULT_NONCE_TTL, DEFAULT_NONCE_CAPACITY)
    }
}

impl InternalReentryRegistry {
    fn new(ttl: Duration, capacity: usize) -> Self {
        Self {
            pending: Mutex::new(HashMap::new()),
            ttl,
            capacity: capacity.max(1),
        }
    }

    pub(in crate::gateway) fn issue(
        &self,
        bridge_provider_id: i64,
        origin_trace_id: &str,
    ) -> Option<String> {
        self.issue_at(bridge_provider_id, origin_trace_id, Instant::now())
    }

    fn issue_at(
        &self,
        bridge_provider_id: i64,
        origin_trace_id: &str,
        now: Instant,
    ) -> Option<String> {
        let mut pending = self
            .pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        self.prune_expired(&mut pending, now);
        while pending.len() >= self.capacity {
            let Some(oldest) = pending
                .iter()
                .min_by_key(|(_, entry)| entry.issued_at)
                .map(|(nonce, _)| nonce.clone())
            else {
                break;
            };
            pending.remove(&oldest);
        }

        for _ in 0..NONCE_GENERATION_ATTEMPTS {
            let nonce = random_nonce();
            if pending.contains_key(&nonce) {
                continue;
            }
            pending.insert(
                nonce.clone(),
                PendingInternalReentry {
                    bridge_provider_id,
                    origin_trace_id: origin_trace_id.to_string(),
                    issued_at: now,
                },
            );
            return Some(nonce);
        }
        None
    }

    pub(in crate::gateway) fn consume(
        &self,
        nonce: &str,
        cli_key: &str,
        method: &Method,
        path: &str,
        query: Option<&str>,
    ) -> Option<TrustedInternalReentry> {
        self.consume_at(nonce, cli_key, method, path, query, Instant::now())
    }

    fn consume_at(
        &self,
        nonce: &str,
        cli_key: &str,
        method: &Method,
        path: &str,
        query: Option<&str>,
        now: Instant,
    ) -> Option<TrustedInternalReentry> {
        if nonce.len() != NONCE_BYTES * 2 || !nonce.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return None;
        }
        let mut pending = self
            .pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let entry = pending.remove(nonce)?;
        if now.saturating_duration_since(entry.issued_at) >= self.ttl
            || cli_key != "codex"
            || *method != Method::POST
            || path != "/v1/responses"
            || query.is_some()
        {
            return None;
        }

        Some(TrustedInternalReentry {
            bridge_provider_id: entry.bridge_provider_id,
            origin_trace_id: entry.origin_trace_id,
        })
    }

    fn prune_expired(&self, pending: &mut HashMap<String, PendingInternalReentry>, now: Instant) {
        pending.retain(|_, entry| now.saturating_duration_since(entry.issued_at) < self.ttl);
    }
}

fn random_nonce() -> String {
    let mut bytes = [0_u8; NONCE_BYTES];
    rand::thread_rng().fill_bytes(&mut bytes);
    let mut nonce = String::with_capacity(NONCE_BYTES * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(nonce, "{byte:02x}");
    }
    nonce
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry() -> InternalReentryRegistry {
        InternalReentryRegistry::new(Duration::from_secs(2), 8)
    }

    #[test]
    fn exact_capability_is_consumed_once() {
        let registry = registry();
        let nonce = registry.issue(42, "trace-outer").expect("nonce");

        let trusted = registry
            .consume(&nonce, "codex", &Method::POST, "/v1/responses", None)
            .expect("trusted reentry");
        assert_eq!(trusted.bridge_provider_id, 42);
        assert_eq!(trusted.origin_trace_id, "trace-outer");
        assert!(registry
            .consume(&nonce, "codex", &Method::POST, "/v1/responses", None,)
            .is_none());
    }

    #[test]
    fn forged_and_expired_capabilities_fail_closed() {
        let registry = registry();
        assert!(registry
            .consume("not-issued", "codex", &Method::POST, "/v1/responses", None,)
            .is_none());

        let issued_at = Instant::now();
        let nonce = registry
            .issue_at(7, "trace-expired", issued_at)
            .expect("nonce");
        assert!(registry
            .consume_at(
                &nonce,
                "codex",
                &Method::POST,
                "/v1/responses",
                None,
                issued_at + Duration::from_secs(3),
            )
            .is_none());
    }

    #[test]
    fn wrong_contract_burns_the_capability() {
        let registry = registry();
        let rejected = [
            ("claude", Method::POST, "/v1/responses", None),
            ("codex", Method::GET, "/v1/responses", None),
            ("codex", Method::POST, "/responses", None),
            ("codex", Method::POST, "/v1/responses", Some("probe=1")),
        ];

        for (cli_key, method, path, query) in rejected {
            let nonce = registry.issue(9, "trace-wrong").expect("nonce");
            assert!(registry
                .consume(&nonce, cli_key, &method, path, query)
                .is_none());
            assert!(registry
                .consume(&nonce, "codex", &Method::POST, "/v1/responses", None,)
                .is_none());
        }
    }

    #[test]
    fn registry_evicts_the_oldest_nonce_at_capacity() {
        let registry = InternalReentryRegistry::new(Duration::from_secs(10), 2);
        let start = Instant::now();
        let first = registry.issue_at(1, "first", start).expect("first");
        let second = registry
            .issue_at(2, "second", start + Duration::from_millis(1))
            .expect("second");
        let third = registry
            .issue_at(3, "third", start + Duration::from_millis(2))
            .expect("third");

        assert!(registry
            .consume_at(
                &first,
                "codex",
                &Method::POST,
                "/v1/responses",
                None,
                start + Duration::from_millis(3),
            )
            .is_none());
        for nonce in [second, third] {
            assert!(registry
                .consume_at(
                    &nonce,
                    "codex",
                    &Method::POST,
                    "/v1/responses",
                    None,
                    start + Duration::from_millis(3),
                )
                .is_some());
        }
    }
}
