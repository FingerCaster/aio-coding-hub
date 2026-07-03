use serde::Serialize;
use std::collections::HashMap;
use std::sync::RwLock;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ActiveRequestStart {
    pub(crate) trace_id: String,
    pub(crate) cli_key: String,
    pub(crate) method: String,
    pub(crate) path: String,
    pub(crate) query: Option<String>,
    pub(crate) session_id: Option<String>,
    pub(crate) requested_model: Option<String>,
    pub(crate) created_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, specta::Type, PartialEq, Eq)]
pub(crate) struct ActiveRequestSnapshotItem {
    pub trace_id: String,
    pub cli_key: String,
    pub method: String,
    pub path: String,
    pub query: Option<String>,
    pub session_id: Option<String>,
    pub requested_model: Option<String>,
    pub created_at_ms: i64,
    pub last_activity_ms: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActiveRequestFinishReason {
    Completed,
    Failed,
    ClientAborted,
    GatewayStopped,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ActiveRequestEntry {
    start: ActiveRequestStart,
    last_activity_ms: i64,
}

#[derive(Debug, Default)]
pub(crate) struct ActiveRequestRegistry {
    entries: RwLock<HashMap<String, ActiveRequestEntry>>,
}

impl ActiveRequestRegistry {
    pub(crate) fn register(&self, start: ActiveRequestStart) {
        let last_activity_ms = start.created_at_ms.max(0);
        if let Ok(mut entries) = self.entries.write() {
            entries.insert(
                start.trace_id.clone(),
                ActiveRequestEntry {
                    start,
                    last_activity_ms,
                },
            );
        }
    }

    pub(crate) fn touch(&self, trace_id: &str, last_activity_ms: i64) {
        if let Ok(mut entries) = self.entries.write() {
            if let Some(entry) = entries.get_mut(trace_id) {
                entry.last_activity_ms = entry.last_activity_ms.max(last_activity_ms.max(0));
            }
        }
    }

    pub(crate) fn finish(
        &self,
        trace_id: &str,
        _reason: ActiveRequestFinishReason,
    ) -> Option<ActiveRequestSnapshotItem> {
        self.entries
            .write()
            .ok()
            .and_then(|mut entries| entries.remove(trace_id))
            .map(ActiveRequestEntry::into_snapshot)
    }

    pub(crate) fn finish_all(
        &self,
        _reason: ActiveRequestFinishReason,
    ) -> Vec<ActiveRequestSnapshotItem> {
        let Ok(mut entries) = self.entries.write() else {
            return Vec::new();
        };
        let mut rows: Vec<_> = entries
            .drain()
            .map(|(_, entry)| entry.into_snapshot())
            .collect();
        sort_snapshot_items(&mut rows);
        rows
    }

    pub(crate) fn snapshot(&self) -> Vec<ActiveRequestSnapshotItem> {
        let Ok(entries) = self.entries.read() else {
            return Vec::new();
        };
        let mut rows: Vec<_> = entries
            .values()
            .cloned()
            .map(ActiveRequestEntry::into_snapshot)
            .collect();
        sort_snapshot_items(&mut rows);
        rows
    }
}

impl ActiveRequestEntry {
    fn into_snapshot(self) -> ActiveRequestSnapshotItem {
        ActiveRequestSnapshotItem {
            trace_id: self.start.trace_id,
            cli_key: self.start.cli_key,
            method: self.start.method,
            path: self.start.path,
            query: self.start.query,
            session_id: self.start.session_id,
            requested_model: self.start.requested_model,
            created_at_ms: self.start.created_at_ms,
            last_activity_ms: self.last_activity_ms,
        }
    }
}

fn sort_snapshot_items(rows: &mut [ActiveRequestSnapshotItem]) {
    rows.sort_by(|a, b| {
        b.created_at_ms
            .cmp(&a.created_at_ms)
            .then_with(|| b.trace_id.cmp(&a.trace_id))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn active_request_start(trace_id: &str) -> ActiveRequestStart {
        ActiveRequestStart {
            trace_id: trace_id.to_string(),
            cli_key: "claude".to_string(),
            method: "POST".to_string(),
            path: "/v1/messages".to_string(),
            query: None,
            session_id: None,
            requested_model: Some("claude-sonnet-4".to_string()),
            created_at_ms: 1_000,
        }
    }

    #[test]
    fn registry_register_touch_finish_lifecycle() {
        let registry = ActiveRequestRegistry::default();
        registry.register(active_request_start("trace-active"));

        assert_eq!(registry.snapshot().len(), 1);
        registry.touch("trace-active", 2_000);
        assert_eq!(registry.snapshot()[0].last_activity_ms, 2_000);

        assert!(registry
            .finish("trace-active", ActiveRequestFinishReason::Completed)
            .is_some());
        assert!(registry.snapshot().is_empty());
    }

    #[test]
    fn registry_finish_is_idempotent() {
        let registry = ActiveRequestRegistry::default();
        registry.register(active_request_start("trace-once"));

        assert!(registry
            .finish("trace-once", ActiveRequestFinishReason::Completed)
            .is_some());
        assert!(registry
            .finish("trace-once", ActiveRequestFinishReason::Completed)
            .is_none());
    }

    #[test]
    fn registry_finish_all_clears_entries() {
        let registry = ActiveRequestRegistry::default();
        registry.register(active_request_start("trace-a"));
        registry.register(active_request_start("trace-b"));

        let removed = registry.finish_all(ActiveRequestFinishReason::GatewayStopped);

        assert_eq!(removed.len(), 2);
        assert!(registry.snapshot().is_empty());
    }
}
