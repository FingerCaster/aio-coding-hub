//! OMP 18.3.2 settings: explicit editable surface, never an arbitrary config IPC.
use crate::shared::error::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

#[derive(Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OmpSettingField {
    pub key: String,
    pub label: String,
    pub group: String,
    pub kind: String,
    pub default_value: Value,
    pub options: Vec<String>,
    pub min: Option<i32>,
    pub max: Option<i32>,
}

#[derive(Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OmpModelOption {
    pub selector: String,
    pub label: String,
    pub source: String,
    pub thinking_levels: Vec<String>,
}

#[derive(Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OmpAgentSummary {
    pub name: String,
    pub description: String,
    pub source: String,
    pub file_name: Option<String>,
    pub model: Vec<String>,
    pub thinking: Option<String>,
}

#[derive(Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OmpSettingsSnapshot {
    pub target_id: String,
    pub config_path: String,
    pub agents_dir: String,
    pub revision: String,
    pub writable: bool,
    pub warnings: Vec<String>,
    pub fields: Vec<OmpSettingField>,
    /// Only explicitly supported settings; auth and unknown fields stay on disk.
    pub values: BTreeMap<String, Value>,
    pub models: Vec<OmpModelOption>,
    pub agents: Vec<OmpAgentSummary>,
}

#[derive(Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct OmpSettingPatch {
    pub path: Vec<String>,
    /// null removes the explicit value, restoring OMP's own inheritance.
    pub value: Option<Value>,
}

#[derive(Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct OmpSettingsSaveInput {
    pub target_id: String,
    pub expected_revision: String,
    pub patches: Vec<OmpSettingPatch>,
}

#[derive(Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OmpSettingsWriteResult {
    pub target_id: String,
    pub revision: String,
    pub changed: bool,
    pub backup_path: Option<String>,
}

#[derive(Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OmpAgentDocument {
    pub target_id: String,
    pub file_name: String,
    pub revision: String,
    pub content: String,
}

#[derive(Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct OmpAgentSaveInput {
    pub target_id: String,
    pub file_name: String,
    pub expected_revision: String,
    pub content: String,
}

pub(crate) const EFFORTS: &[&str] = &["minimal", "low", "medium", "high", "xhigh", "max"];
pub(crate) const RECORD_KEYS: &[&str] = &[
    "modelRoles",
    "task.agentModelOverrides",
    "task.agentServiceTierOverrides",
    "task.agentPrewalk",
    "task.agentAdvisor",
];

pub(crate) fn fields() -> Vec<OmpSettingField> {
    let mut out = Vec::new();
    let mut add = |key: &str,
                   label: &str,
                   group: &str,
                   kind: &str,
                   default_value: Value,
                   options: &[&str],
                   bounds: Option<(i32, i32)>| {
        out.push(OmpSettingField {
            key: key.into(),
            label: label.into(),
            group: group.into(),
            kind: kind.into(),
            default_value,
            options: options.iter().map(|s| (*s).into()).collect(),
            min: bounds.map(|b| b.0),
            max: bounds.map(|b| b.1),
        });
    };
    add(
        "defaultThinkingLevel",
        "默认思考等级",
        "session",
        "enum",
        json!("high"),
        &["auto", "minimal", "low", "medium", "high", "xhigh", "max"],
        None,
    );
    add(
        "modelRoleStorage",
        "OMP 内模型角色的保存位置",
        "session",
        "enum",
        json!("global"),
        &["global", "project"],
        None,
    );
    add(
        "tools.approvalMode",
        "工具审批模式",
        "session",
        "enum",
        json!("yolo"),
        &["always-ask", "write", "yolo"],
        None,
    );
    add(
        "steeringMode",
        "即时指令处理",
        "session",
        "enum",
        json!("one-at-a-time"),
        &["one-at-a-time", "all"],
        None,
    );
    add(
        "followUpMode",
        "后续消息处理",
        "session",
        "enum",
        json!("one-at-a-time"),
        &["one-at-a-time", "all"],
        None,
    );
    add(
        "compaction.enabled",
        "自动压缩上下文",
        "session",
        "boolean",
        json!(true),
        &[],
        None,
    );
    add(
        "compaction.reserveTokens",
        "压缩预留 Token（空值为原生自动）",
        "session",
        "number",
        Value::Null,
        &[],
        Some((0, 10_000_000)),
    );
    add(
        "compaction.keepRecentTokens",
        "压缩保留最近 Token",
        "session",
        "number",
        json!(20000),
        &[],
        Some((0, 10_000_000)),
    );
    add(
        "task.maxConcurrency",
        "最大并发 Agent（0 不限）",
        "tasks",
        "number",
        json!(32),
        &[],
        Some((0, 1024)),
    );
    add(
        "task.maxRecursionDepth",
        "最大递归层数（-1 不限）",
        "tasks",
        "number",
        json!(2),
        &[],
        Some((-1, 100)),
    );
    add(
        "task.maxRuntimeMs",
        "Agent 最长运行时间（毫秒，0 不限）",
        "tasks",
        "number",
        json!(0),
        &[],
        Some((0, 604_800_000)),
    );
    add(
        "task.agentIdleTtlMs",
        "空闲 Agent 停驻时间（毫秒，0 禁用）",
        "tasks",
        "number",
        json!(420000),
        &[],
        Some((0, 604_800_000)),
    );
    add(
        "task.softRequestBudget",
        "每个 Agent 的请求软预算（0 禁用）",
        "tasks",
        "number",
        json!(200),
        &[],
        Some((0, 1_000_000)),
    );
    add(
        "task.softRequestBudgetNotice",
        "请求预算提醒",
        "tasks",
        "boolean",
        json!(true),
        &[],
        None,
    );
    add(
        "task.enableEffort",
        "允许任务指定思考强度",
        "tasks",
        "boolean",
        json!(false),
        &[],
        None,
    );
    add(
        "task.maxEffort",
        "子任务思考等级上限",
        "tasks",
        "enum",
        json!("max"),
        EFFORTS,
        None,
    );
    add(
        "task.enableLsp",
        "子 Agent 使用 LSP",
        "tasks",
        "boolean",
        json!(false),
        &[],
        None,
    );
    add(
        "task.eager",
        "任务委派倾向",
        "tasks",
        "enum",
        json!("default"),
        &["default", "preferred", "always"],
        None,
    );
    add(
        "task.batch",
        "允许批量委派",
        "tasks",
        "boolean",
        json!(true),
        &[],
        None,
    );
    add(
        "task.prewalk",
        "通用 task 首次编辑时交接给 smol",
        "tasks",
        "boolean",
        json!(false),
        &[],
        None,
    );
    add(
        "task.showResolvedModelBadge",
        "显示实际子 Agent 模型",
        "tasks",
        "boolean",
        json!(false),
        &[],
        None,
    );
    add(
        "task.isolation.enabled",
        "子 Agent 使用隔离工作区",
        "tasks",
        "boolean",
        json!(false),
        &[],
        None,
    );
    add(
        "task.isolation.apply",
        "自动应用隔离工作区变更",
        "tasks",
        "boolean",
        json!(true),
        &[],
        None,
    );
    add(
        "task.isolation.merge",
        "隔离变更合并方式",
        "tasks",
        "enum",
        json!("patch"),
        &["patch", "branch"],
        None,
    );
    add(
        "task.isolation.commits",
        "隔离提交信息",
        "tasks",
        "enum",
        json!("generic"),
        &["generic", "ai"],
        None,
    );
    out
}

pub(crate) fn invalid(message: &str) -> AppError {
    AppError::new("OMP_SETTINGS_INVALID", message)
}

pub(crate) fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name.trim() == name
        && !name.chars().any(char::is_control)
        && !matches!(name, "__proto__" | "prototype" | "constructor")
}

fn selector(value: &Value) -> bool {
    value.as_str().is_some_and(|s| {
        !s.trim().is_empty() && s.len() <= 1024 && !s.chars().any(char::is_control)
    })
}

pub(crate) fn model_selector(value: &Value) -> bool {
    selector(value)
        || value
            .as_array()
            .is_some_and(|a| !a.is_empty() && a.len() <= 32 && a.iter().all(selector))
}

pub(crate) fn validate_patch(patch: &OmpSettingPatch) -> AppResult<()> {
    if patch.path.is_empty() || patch.path.iter().any(|s| !valid_name(s)) {
        return Err(invalid("无效的设置路径"));
    }
    let key = patch.path.join(".");
    if let Some(field) = fields()
        .iter()
        .find(|f| f.key == key && f.key.split('.').eq(patch.path.iter().map(String::as_str)))
    {
        let Some(value) = &patch.value else {
            return Ok(());
        };
        let ok = match field.kind.as_str() {
            "boolean" => value.is_boolean(),
            "enum" => value
                .as_str()
                .is_some_and(|s| field.options.iter().any(|v| v == s)),
            "number" => value.as_i64().is_some_and(|n| {
                n >= i64::from(field.min.unwrap_or(i32::MIN))
                    && n <= i64::from(field.max.unwrap_or(i32::MAX))
            }),
            _ => false,
        };
        return if ok {
            Ok(())
        } else {
            Err(invalid(&format!("{} 的类型或范围无效", field.label)))
        };
    }
    if key == "task.disabledAgents" && patch.path.len() == 2 {
        let ok = patch.value.as_ref().is_none_or(|v| {
            v.as_array().is_some_and(|a| {
                a.len() <= 512 && a.iter().all(|v| v.as_str().is_some_and(valid_name))
            })
        });
        return if ok {
            Ok(())
        } else {
            Err(invalid("禁用 Agent 列表无效"))
        };
    }
    let parent = patch.path[..patch.path.len() - 1].join(".");
    if !RECORD_KEYS.iter().any(|key| {
        *key == parent
            && key.split('.').eq(patch.path[..patch.path.len() - 1]
                .iter()
                .map(String::as_str))
    }) {
        return Err(invalid("该字段不在 OMP 可编辑设置中"));
    }
    let Some(value) = &patch.value else {
        return Ok(());
    };
    let ok = match parent.as_str() {
        "modelRoles" => selector(value) && !value.as_str().unwrap_or("").starts_with('@'),
        "task.agentModelOverrides" => model_selector(value),
        "task.agentPrewalk" | "task.agentAdvisor" => selector(value),
        "task.agentServiceTierOverrides" => value.as_str().is_some_and(|s| {
            [
                "inherit", "none", "auto", "default", "flex", "scale", "priority",
            ]
            .contains(&s)
        }),
        _ => false,
    };
    if ok {
        Ok(())
    } else {
        Err(invalid("模型或 Agent 设置值无效"))
    }
}

pub(crate) fn get_path<'a>(root: &'a Value, path: &[&str]) -> Option<&'a Value> {
    path.iter().try_fold(root, |v, key| v.get(*key))
}

pub(crate) fn project(root: &Value) -> BTreeMap<String, Value> {
    fields()
        .into_iter()
        .map(|f| f.key)
        .chain(RECORD_KEYS.iter().map(|s| (*s).into()))
        .chain(["task.disabledAgents".into()])
        .filter_map(|key| {
            get_path(root, &key.split('.').collect::<Vec<_>>())
                .cloned()
                .map(|v| (key, v))
        })
        .collect()
}

pub(crate) fn patch_value(
    root: &mut Value,
    path: &[String],
    value: &Option<Value>,
) -> AppResult<()> {
    let map = root
        .as_object_mut()
        .ok_or_else(|| invalid("设置的父级不是映射，请在原生配置中修复后重试"))?;
    if path.len() == 1 {
        if let Some(value) = value {
            map.insert(path[0].clone(), value.clone());
        } else {
            map.remove(&path[0]);
        }
        return Ok(());
    }
    if !map.contains_key(&path[0]) && value.is_none() {
        return Ok(());
    }
    let child = map.entry(path[0].clone()).or_insert_with(|| json!({}));
    let had_members = child.as_object().is_some_and(|v| !v.is_empty());
    patch_value(child, &path[1..], value)?;
    if had_members && child.as_object().is_some_and(|v| v.is_empty()) {
        map.remove(&path[0]);
    }
    Ok(())
}
