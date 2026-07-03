//! Local continuity cache for bridged OpenAI Responses requests.

use serde_json::Value;
use std::collections::HashMap;
#[cfg(test)]
use std::sync::{Mutex, MutexGuard};
use std::sync::{OnceLock, RwLock};
use std::time::{Duration, Instant};

const CACHE_TTL: Duration = Duration::from_secs(10 * 60);
const CACHE_MAX_ENTRIES: usize = 2_000;
const CACHE_MAX_ITEMS_PER_ENTRY: usize = 200;

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub(crate) struct ResponsesCacheKey {
    namespace: String,
    response_id: String,
}

impl ResponsesCacheKey {
    pub(crate) fn new(
        namespace: impl Into<String>,
        response_id: impl Into<String>,
    ) -> Option<Self> {
        let namespace = namespace.into();
        let response_id = response_id.into();
        if namespace.trim().is_empty() || response_id.trim().is_empty() {
            return None;
        }
        Some(Self {
            namespace,
            response_id,
        })
    }
}

#[derive(Debug, Clone)]
struct CacheEntry {
    created_at: Instant,
    items: Vec<Value>,
}

fn cache() -> &'static RwLock<HashMap<ResponsesCacheKey, CacheEntry>> {
    static CACHE: OnceLock<RwLock<HashMap<ResponsesCacheKey, CacheEntry>>> = OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(HashMap::new()))
}

#[cfg(test)]
pub(crate) fn test_guard() -> MutexGuard<'static, ()> {
    static TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    TEST_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub(crate) fn get(key: &ResponsesCacheKey) -> Option<Vec<Value>> {
    let now = Instant::now();
    {
        let guard = cache().read().ok()?;
        let entry = guard.get(key)?;
        if now.duration_since(entry.created_at) <= CACHE_TTL {
            return Some(entry.items.clone());
        }
    }
    if let Ok(mut guard) = cache().write() {
        guard.remove(key);
    }
    None
}

pub(crate) fn set(key: ResponsesCacheKey, mut items: Vec<Value>) {
    if items.is_empty() {
        return;
    }
    if items.len() > CACHE_MAX_ITEMS_PER_ENTRY {
        items = items.split_off(items.len() - CACHE_MAX_ITEMS_PER_ENTRY);
    }
    let Ok(mut guard) = cache().write() else {
        return;
    };
    prune_expired_locked(&mut guard, Instant::now());
    if guard.len() >= CACHE_MAX_ENTRIES {
        let remove_count = (CACHE_MAX_ENTRIES / 10).max(1);
        let keys = guard.keys().take(remove_count).cloned().collect::<Vec<_>>();
        for key in keys {
            guard.remove(&key);
        }
    }
    guard.insert(
        key,
        CacheEntry {
            created_at: Instant::now(),
            items,
        },
    );
}

fn prune_expired_locked(cache: &mut HashMap<ResponsesCacheKey, CacheEntry>, now: Instant) {
    cache.retain(|_, entry| now.duration_since(entry.created_at) <= CACHE_TTL);
}

pub(crate) fn namespace(
    bridge_type: &str,
    source_provider_id: i64,
    session_id: Option<&str>,
    trace_id: &str,
) -> String {
    let boundary = session_id
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(trace_id);
    format!("{bridge_type}:source={source_provider_id}:session={boundary}")
}

pub(crate) fn cache_completed_response(
    namespace: &str,
    expanded_input: &[Value],
    response: &Value,
) {
    let Some(response_id) = response.get("id").and_then(Value::as_str) else {
        return;
    };
    let Some(output) = response.get("output").and_then(Value::as_array) else {
        return;
    };
    if !output.iter().any(is_tool_call_context_item) {
        return;
    }

    let mut replay_items = Vec::new();
    for item in expanded_input {
        if let Some(item) = replayable_input_item(item) {
            replay_items.push(item);
        }
    }
    for item in output {
        if let Some(item) = replayable_output_item(item) {
            replay_items.push(item);
        }
    }
    if let Some(key) = ResponsesCacheKey::new(namespace, response_id) {
        set(key, replay_items);
    }
}

pub(crate) fn replayable_input_item(item: &Value) -> Option<Value> {
    if item.get("type").and_then(Value::as_str) == Some("reasoning")
        && item.get("encrypted_content").is_some()
    {
        return None;
    }
    strip_item_id(item)
}

pub(crate) fn replayable_output_item(item: &Value) -> Option<Value> {
    if !is_tool_call_context_item(item) {
        return None;
    }
    strip_item_id(item)
}

pub(crate) fn strip_item_id(item: &Value) -> Option<Value> {
    let mut item = item.clone();
    if let Some(obj) = item.as_object_mut() {
        obj.remove("id");
    }
    Some(item)
}

pub(crate) fn is_tool_call_context_item(item: &Value) -> bool {
    matches!(
        item.get("type").and_then(Value::as_str),
        Some(
            "function_call"
                | "tool_call"
                | "local_shell_call"
                | "tool_search_call"
                | "custom_tool_call"
                | "mcp_tool_call"
        )
    )
}

pub(crate) fn is_tool_output_item(item: &Value) -> bool {
    matches!(
        item.get("type").and_then(Value::as_str),
        Some(
            "function_call_output"
                | "tool_call_output"
                | "local_shell_call_output"
                | "tool_search_call_output"
                | "tool_search_output"
                | "custom_tool_call_output"
                | "mcp_tool_call_output"
        )
    )
}

#[cfg(test)]
pub(crate) fn clear_for_tests() {
    if let Ok(mut guard) = cache().write() {
        guard.clear();
    }
}

#[cfg(test)]
pub(crate) fn force_insert_for_tests(key: ResponsesCacheKey, items: Vec<Value>, age: Duration) {
    if let Ok(mut guard) = cache().write() {
        guard.insert(
            key,
            CacheEntry {
                created_at: Instant::now()
                    .checked_sub(age)
                    .expect("test cache age should be within Instant range"),
                items,
            },
        );
    }
}

#[cfg(test)]
pub(crate) fn len_for_tests() -> usize {
    cache().read().map(|guard| guard.len()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn cache_completed_response_stores_replayable_tool_context_by_namespace() {
        let _guard = test_guard();
        clear_for_tests();
        let input = vec![json!({
            "id": "msg_1",
            "type": "message",
            "role": "user",
            "content": [{"type": "input_text", "text": "use a tool"}]
        })];
        let response = json!({
            "id": "resp_1",
            "output": [{
                "id": "fc_1",
                "type": "function_call",
                "call_id": "call_1",
                "name": "lookup",
                "arguments": "{}"
            }]
        });

        cache_completed_response("bridge:source=1:session=a", &input, &response);

        let key = ResponsesCacheKey::new("bridge:source=1:session=a", "resp_1").unwrap();
        let cached = get(&key).expect("cached replay items");
        assert_eq!(cached.len(), 2);
        assert!(cached[0].get("id").is_none());
        assert!(cached[1].get("id").is_none());
        assert_eq!(cached[1]["type"], "function_call");

        let other_namespace =
            ResponsesCacheKey::new("bridge:source=2:session=a", "resp_1").unwrap();
        assert!(get(&other_namespace).is_none());
    }

    #[test]
    fn cache_completed_response_ignores_plain_text_responses() {
        let _guard = test_guard();
        clear_for_tests();
        cache_completed_response(
            "bridge:source=1:session=a",
            &[json!({"type": "message", "role": "user", "content": []})],
            &json!({
                "id": "resp_text",
                "output": [{
                    "type": "message",
                    "role": "assistant",
                    "content": [{"type": "output_text", "text": "hello"}]
                }]
            }),
        );

        assert_eq!(len_for_tests(), 0);
    }

    #[test]
    fn expired_entries_are_removed_on_read() {
        let _guard = test_guard();
        clear_for_tests();
        let key = ResponsesCacheKey::new("bridge:source=1:session=a", "resp_old").unwrap();
        force_insert_for_tests(
            key.clone(),
            vec![json!({"type": "function_call", "call_id": "call_1"})],
            CACHE_TTL + Duration::from_secs(1),
        );

        assert!(get(&key).is_none());
        assert_eq!(len_for_tests(), 0);
    }
}
