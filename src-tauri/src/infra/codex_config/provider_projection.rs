use crate::shared::error::AppResult;
use toml_edit::{DocumentMut, Item, TableLike, Value};

pub(crate) const AIO_PROVIDER_KEY: &str = "aio";
pub(crate) const OPENAI_PROVIDER_KEY: &str = "OpenAI";

const MANAGED_PROVIDER_FIELDS: &[&str] = &["name", "base_url", "wire_api", "requires_openai_auth"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CodexManagedProviderKey {
    Aio,
    OpenAi,
}

impl CodexManagedProviderKey {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Aio => AIO_PROVIDER_KEY,
            Self::OpenAi => OPENAI_PROVIDER_KEY,
        }
    }

    fn alternate(self) -> &'static str {
        match self {
            Self::Aio => OPENAI_PROVIDER_KEY,
            Self::OpenAi => AIO_PROVIDER_KEY,
        }
    }
}

pub(crate) fn desired_provider_key_from_config(
    config: &[u8],
) -> AppResult<CodexManagedProviderKey> {
    let doc = parse_document(config)?;
    Ok(desired_provider_key_from_document(&doc))
}

pub(crate) fn reconcile_provider_identity(
    config: &[u8],
    desired: CodexManagedProviderKey,
    expected_base_url: Option<&str>,
) -> AppResult<Vec<u8>> {
    let mut doc = parse_document(config)?;
    reconcile_document_provider_identity(&mut doc, desired, expected_base_url)?;
    Ok(document_bytes(doc))
}

pub(crate) fn project_active_provider(
    config: &[u8],
    base_url: &str,
    previous_managed_base_url: Option<&str>,
) -> AppResult<Vec<u8>> {
    let mut doc = parse_document(config)?;
    let desired = desired_provider_key_from_document(&doc);
    reconcile_document_provider_identity_with_owned_base(
        &mut doc,
        desired,
        Some(base_url),
        previous_managed_base_url,
        true,
    )?;

    let provider = provider_item_mut(&mut doc, desired.as_str())?
        .ok_or_else(|| provider_conflict("missing managed provider table"))?;
    let provider = provider
        .as_table_like_mut()
        .ok_or_else(|| provider_conflict("managed provider is not a table"))?;
    replace_value_preserving_decor(provider, "name", Value::from(desired.as_str()));
    replace_value_preserving_decor(provider, "base_url", Value::from(base_url));
    replace_value_preserving_decor(provider, "wire_api", Value::from("responses"));
    replace_value_preserving_decor(provider, "requires_openai_auth", Value::from(true));

    Ok(document_bytes(doc))
}

pub(crate) fn is_managed_projection_applied(config: &[u8], base_url: &str) -> bool {
    let Ok(doc) = parse_document(config) else {
        return false;
    };
    let desired = desired_provider_key_from_document(&doc);
    if doc.get("model_provider").and_then(Item::as_str) != Some(desired.as_str()) {
        return false;
    }

    let Some(providers) = doc.get("model_providers").and_then(Item::as_table_like) else {
        return false;
    };
    if providers.contains_key(desired.alternate()) {
        return false;
    }
    let Some(provider) = providers
        .get(desired.as_str())
        .and_then(Item::as_table_like)
    else {
        return false;
    };

    provider.get("name").and_then(Item::as_str) == Some(desired.as_str())
        && provider
            .get("base_url")
            .and_then(Item::as_str)
            .is_some_and(|actual| normalized_url(actual) == normalized_url(base_url))
        && provider.get("wire_api").and_then(Item::as_str) == Some("responses")
        && provider.get("requires_openai_auth").and_then(Item::as_bool) == Some(true)
}

pub(crate) fn has_managed_provider_identity(config: &[u8]) -> bool {
    let Ok(doc) = parse_document(config) else {
        return false;
    };
    let desired = desired_provider_key_from_document(&doc);
    doc.get("model_provider").and_then(Item::as_str) == Some(desired.as_str())
        && doc
            .get("model_providers")
            .and_then(Item::as_table_like)
            .is_some_and(|providers| providers.contains_key(desired.as_str()))
}

pub(crate) fn merge_raw_user_changes(
    baseline: &[u8],
    expected_projection: &[u8],
    current_live: &[u8],
    submitted: &[u8],
) -> AppResult<Vec<u8>> {
    let baseline_doc = parse_document(baseline)?;
    let expected_doc = parse_document(expected_projection)?;
    let current_doc = parse_document(current_live)?;
    let submitted_doc = parse_document(submitted)?;

    let mut merged = Item::Table(baseline_doc.as_table().clone());
    let expected = Item::Table(expected_doc.as_table().clone());
    let current = Item::Table(current_doc.as_table().clone());
    let submitted = Item::Table(submitted_doc.as_table().clone());
    validate_owned_edits(Some(&current), Some(&submitted), &mut Vec::new())?;
    merge_user_item(
        &mut merged,
        Some(&expected),
        Some(&submitted),
        &mut Vec::new(),
    )?;

    let table = merged
        .into_table()
        .map_err(|_| "CODEX_PROXY_OWNED_FIELD_EDIT: invalid merged config root".to_string())?;
    Ok(document_bytes(DocumentMut::from(table)))
}

pub(crate) fn restore_managed_provider_projection(
    current: &[u8],
    baseline: &[u8],
) -> AppResult<Vec<u8>> {
    let mut current_doc = parse_document(current)?;
    let baseline_doc = parse_document(baseline)?;

    match baseline_doc.get("model_provider") {
        Some(original) => {
            current_doc.insert("model_provider", original.clone());
        }
        None => {
            current_doc.remove("model_provider");
        }
    }

    let baseline_providers = baseline_doc
        .get("model_providers")
        .and_then(Item::as_table_like);
    ensure_providers_table(&mut current_doc)?;
    let current_providers = current_doc
        .get_mut("model_providers")
        .and_then(Item::as_table_like_mut)
        .ok_or_else(|| provider_conflict("model_providers is not a table"))?;

    for key in [AIO_PROVIDER_KEY, OPENAI_PROVIDER_KEY] {
        let active = current_providers.remove(key);
        let original = baseline_providers.and_then(|providers| providers.get(key));
        let Some(original) = original else {
            continue;
        };
        let mut restored = original.clone();
        if let Some(active) = active.as_ref() {
            merge_current_user_provider_fields(&mut restored, active, true);
        }
        current_providers.insert(key, restored);
    }

    if current_providers.is_empty() && baseline_providers.is_none() {
        current_doc.remove("model_providers");
    }
    Ok(document_bytes(current_doc))
}

fn parse_document(config: &[u8]) -> AppResult<DocumentMut> {
    if config.is_empty() {
        return Ok(DocumentMut::new());
    }
    let text = std::str::from_utf8(config)
        .map_err(|_| "CLI_PROXY_INVALID_TOML: config.toml must be valid UTF-8".to_string())?;
    text.parse::<DocumentMut>()
        .map_err(|err| format!("CLI_PROXY_INVALID_TOML: failed to parse config.toml: {err}").into())
}

fn desired_provider_key_from_document(doc: &DocumentMut) -> CodexManagedProviderKey {
    if doc
        .get("features")
        .and_then(Item::as_table_like)
        .and_then(|features| features.get("remote_compaction"))
        .and_then(Item::as_bool)
        == Some(true)
    {
        CodexManagedProviderKey::OpenAi
    } else {
        CodexManagedProviderKey::Aio
    }
}

fn reconcile_document_provider_identity(
    doc: &mut DocumentMut,
    desired: CodexManagedProviderKey,
    expected_base_url: Option<&str>,
) -> AppResult<()> {
    reconcile_document_provider_identity_with_owned_base(
        doc,
        desired,
        expected_base_url,
        None,
        false,
    )
}

fn reconcile_document_provider_identity_with_owned_base(
    doc: &mut DocumentMut,
    desired: CodexManagedProviderKey,
    expected_base_url: Option<&str>,
    previous_managed_base_url: Option<&str>,
    allow_single_provider_overlay: bool,
) -> AppResult<()> {
    let target_key = desired.as_str();
    let source_key = desired.alternate();
    let previous_root = doc
        .get("model_provider")
        .and_then(Item::as_str)
        .map(str::to_string);

    ensure_providers_table(doc)?;
    let providers = doc
        .get_mut("model_providers")
        .and_then(Item::as_table_like_mut)
        .ok_or_else(|| provider_conflict("model_providers is not a table"))?;
    let source = providers.remove(source_key);
    let target = providers.remove(target_key);

    let mut reconciled = match (source, target) {
        (Some(source), Some(mut target)) => {
            validate_provider_compatibility(
                &source,
                &target,
                source_key,
                target_key,
                expected_base_url,
                previous_managed_base_url,
            )?;
            merge_provider_items(&mut target, &source, target_key, true)?;
            target
        }
        (Some(source), None) => {
            if !allow_single_provider_overlay {
                validate_owned_base_url(
                    &source,
                    source_key,
                    expected_base_url,
                    previous_managed_base_url,
                )?;
            }
            source
        }
        (None, Some(target)) => {
            // Active projection is derived from the manifest baseline, so a single
            // provider can be overlaid regardless of its direct URL and restored later.
            let can_reuse = if allow_single_provider_overlay {
                true
            } else if expected_base_url.is_some() {
                provider_base_url(&target).is_some_and(|actual| {
                    url_matches(actual, expected_base_url)
                        || url_matches(actual, previous_managed_base_url)
                })
            } else {
                previous_root.as_deref() == Some(target_key)
            };
            if !can_reuse {
                return Err(provider_conflict(
                    "target provider exists but is not a proven AIO projection",
                ));
            }
            if !allow_single_provider_overlay {
                validate_owned_base_url(
                    &target,
                    target_key,
                    expected_base_url,
                    previous_managed_base_url,
                )?;
            }
            target
        }
        (None, None) => Item::Table(toml_edit::Table::new()),
    };

    let provider = reconciled
        .as_table_like_mut()
        .ok_or_else(|| provider_conflict("managed provider is not a table"))?;
    replace_value_preserving_decor(provider, "name", Value::from(target_key));
    providers.insert(target_key, reconciled);
    replace_value_preserving_decor(
        doc.as_table_mut(),
        "model_provider",
        Value::from(target_key),
    );
    Ok(())
}

fn replace_value_preserving_decor(table: &mut dyn TableLike, key: &str, value: Value) {
    if let Some(item) = table.get_mut(key) {
        let decor = item.as_value().map(|existing| existing.decor().clone());
        *item = Item::Value(value);
        if let (Some(decor), Some(next)) = (decor, item.as_value_mut()) {
            *next.decor_mut() = decor;
        }
    } else {
        table.insert(key, Item::Value(value));
    }
}

fn ensure_providers_table(doc: &mut DocumentMut) -> AppResult<()> {
    if !doc.contains_key("model_providers") {
        doc.insert("model_providers", Item::Table(toml_edit::Table::new()));
    }
    if doc
        .get("model_providers")
        .and_then(Item::as_table_like)
        .is_none()
    {
        return Err(provider_conflict("model_providers is not a table"));
    }
    Ok(())
}

fn provider_item_mut<'a>(doc: &'a mut DocumentMut, key: &str) -> AppResult<Option<&'a mut Item>> {
    ensure_providers_table(doc)?;
    Ok(doc
        .get_mut("model_providers")
        .and_then(Item::as_table_like_mut)
        .and_then(|providers| providers.get_mut(key)))
}

fn validate_provider_compatibility(
    source: &Item,
    target: &Item,
    source_key: &str,
    target_key: &str,
    expected_base_url: Option<&str>,
    previous_managed_base_url: Option<&str>,
) -> AppResult<()> {
    let source_table = source.as_table_like().ok_or_else(|| {
        provider_conflict(&format!("model_providers.{source_key} is not a table"))
    })?;
    let target_table = target.as_table_like().ok_or_else(|| {
        provider_conflict(&format!("model_providers.{target_key} is not a table"))
    })?;

    validate_owned_base_url(
        source,
        source_key,
        expected_base_url,
        previous_managed_base_url,
    )?;
    validate_owned_base_url(
        target,
        target_key,
        expected_base_url,
        previous_managed_base_url,
    )?;
    for field in MANAGED_PROVIDER_FIELDS {
        if *field == "name" {
            continue;
        }
        let Some(source_value) = source_table.get(field) else {
            continue;
        };
        let Some(target_value) = target_table.get(field) else {
            continue;
        };
        let compatible = if *field == "base_url" {
            match (source_value.as_str(), target_value.as_str()) {
                (Some(source), Some(target)) => normalized_url(source) == normalized_url(target),
                _ => false,
            }
        } else {
            items_semantically_equal(source_value, target_value)
        };
        if !compatible {
            return Err(provider_conflict(&format!(
                "model_providers.{target_key}.{field} conflicts"
            )));
        }
    }
    Ok(())
}

fn validate_owned_base_url(
    provider: &Item,
    key: &str,
    expected_base_url: Option<&str>,
    previous_managed_base_url: Option<&str>,
) -> AppResult<()> {
    if expected_base_url.is_none() {
        return Ok(());
    }
    if let Some(actual) = provider_base_url(provider) {
        if !url_matches(actual, expected_base_url)
            && !url_matches(actual, previous_managed_base_url)
        {
            return Err(provider_conflict(&format!(
                "model_providers.{key}.base_url conflicts"
            )));
        }
    }
    Ok(())
}

fn provider_base_url(provider: &Item) -> Option<&str> {
    provider
        .as_table_like()
        .and_then(|table| table.get("base_url"))
        .and_then(Item::as_str)
}

fn merge_provider_items(
    target: &mut Item,
    source: &Item,
    path: &str,
    provider_root: bool,
) -> AppResult<()> {
    let Some(target_table) = target.as_table_like_mut() else {
        if items_semantically_equal(target, source) {
            return Ok(());
        }
        return Err(provider_conflict(&format!(
            "model_providers.{path} conflicts"
        )));
    };
    let Some(source_table) = source.as_table_like() else {
        return Err(provider_conflict(&format!(
            "model_providers.{path} conflicts"
        )));
    };

    for (key, source_item) in source_table.iter() {
        if provider_root && MANAGED_PROVIDER_FIELDS.contains(&key) {
            if key != "name" && !target_table.contains_key(key) {
                target_table.insert(key, source_item.clone());
            }
            continue;
        }
        let child_path = format!("{path}.{key}");
        match target_table.get_mut(key) {
            Some(target_item) => {
                merge_provider_items(target_item, source_item, &child_path, false)?;
            }
            None => {
                target_table.insert(key, source_item.clone());
            }
        }
    }
    Ok(())
}

fn merge_user_item(
    baseline: &mut Item,
    expected: Option<&Item>,
    submitted: Option<&Item>,
    path: &mut Vec<String>,
) -> AppResult<()> {
    if is_proxy_owned_path(path) {
        return Ok(());
    }
    if optional_items_semantically_equal(expected, submitted) {
        return Ok(());
    }

    match (expected, submitted) {
        (_, None) => {
            *baseline = Item::None;
            Ok(())
        }
        (None, Some(submitted)) => {
            *baseline = submitted.clone();
            Ok(())
        }
        (Some(expected), Some(submitted)) => {
            let (Some(expected_table), Some(submitted_table)) =
                (expected.as_table_like(), submitted.as_table_like())
            else {
                if is_managed_provider_root(path) {
                    return Err(owned_field_edit(path));
                }
                *baseline = submitted.clone();
                return Ok(());
            };

            if baseline.as_table_like().is_none() {
                *baseline = Item::Table(toml_edit::Table::new());
            }
            let baseline_table = baseline
                .as_table_like_mut()
                .ok_or_else(|| owned_field_edit(path))?;
            let mut keys: Vec<String> = expected_table
                .iter()
                .map(|(key, _)| key.to_string())
                .chain(submitted_table.iter().map(|(key, _)| key.to_string()))
                .collect();
            keys.sort();
            keys.dedup();

            for key in keys {
                path.push(key.clone());
                let expected_child = expected_table.get(&key);
                let submitted_child = submitted_table.get(&key);
                if is_proxy_owned_path(path) {
                    path.pop();
                    continue;
                }
                if submitted_child.is_none() && !is_proxy_owned_path(path) {
                    baseline_table.remove(&key);
                    path.pop();
                    continue;
                }
                if !baseline_table.contains_key(&key) {
                    let seed = if submitted_child.is_some_and(|item| item.as_table_like().is_some())
                    {
                        Item::Table(toml_edit::Table::new())
                    } else {
                        submitted_child.cloned().unwrap_or(Item::None)
                    };
                    baseline_table.insert(&key, seed);
                }
                let baseline_child = baseline_table
                    .get_mut(&key)
                    .ok_or_else(|| owned_field_edit(path))?;
                merge_user_item(baseline_child, expected_child, submitted_child, path)?;
                if baseline_child.is_none() {
                    baseline_table.remove(&key);
                }
                path.pop();
            }
            Ok(())
        }
    }
}

fn validate_owned_edits(
    current: Option<&Item>,
    submitted: Option<&Item>,
    path: &mut Vec<String>,
) -> AppResult<()> {
    if is_proxy_owned_path(path) {
        if !optional_items_semantically_equal(current, submitted) {
            return Err(owned_field_edit(path));
        }
        return Ok(());
    }

    let current_table = current.and_then(Item::as_table_like);
    let submitted_table = submitted.and_then(Item::as_table_like);
    if current_table.is_none() && submitted_table.is_none() {
        return Ok(());
    }
    let mut keys: Vec<String> = current_table
        .into_iter()
        .flat_map(|table| table.iter().map(|(key, _)| key.to_string()))
        .chain(
            submitted_table
                .into_iter()
                .flat_map(|table| table.iter().map(|(key, _)| key.to_string())),
        )
        .collect();
    keys.sort();
    keys.dedup();
    for key in keys {
        path.push(key.clone());
        validate_owned_edits(
            current_table.and_then(|table| table.get(&key)),
            submitted_table.and_then(|table| table.get(&key)),
            path,
        )?;
        path.pop();
    }
    Ok(())
}

fn merge_current_user_provider_fields(target: &mut Item, current: &Item, provider_root: bool) {
    let (Some(target_table), Some(current_table)) =
        (target.as_table_like_mut(), current.as_table_like())
    else {
        return;
    };
    for (key, current_item) in current_table.iter() {
        if provider_root && MANAGED_PROVIDER_FIELDS.contains(&key) {
            continue;
        }
        match target_table.get_mut(key) {
            Some(target_item)
                if target_item.as_table_like().is_some()
                    && current_item.as_table_like().is_some() =>
            {
                merge_current_user_provider_fields(target_item, current_item, false);
            }
            Some(target_item) => *target_item = current_item.clone(),
            None => {
                target_table.insert(key, current_item.clone());
            }
        }
    }
}

fn is_proxy_owned_path(path: &[String]) -> bool {
    matches!(
        path,
        [root]
            if matches!(
                root.as_str(),
                "model_provider" | "preferred_auth_method" | "model_catalog_json"
            )
    ) || matches!(path, [table, key] if table == "windows" && key == "sandbox")
        || matches!(
            path,
            [providers, provider, field]
                if providers == "model_providers"
                    && matches!(provider.as_str(), AIO_PROVIDER_KEY | OPENAI_PROVIDER_KEY)
                    && MANAGED_PROVIDER_FIELDS.contains(&field.as_str())
        )
}

fn is_managed_provider_root(path: &[String]) -> bool {
    matches!(
        path,
        [providers, provider]
            if providers == "model_providers"
                && matches!(provider.as_str(), AIO_PROVIDER_KEY | OPENAI_PROVIDER_KEY)
    )
}

fn optional_items_semantically_equal(left: Option<&Item>, right: Option<&Item>) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some(left), Some(right)) => items_semantically_equal(left, right),
        _ => false,
    }
}

fn items_semantically_equal(left: &Item, right: &Item) -> bool {
    fn normalize(item: &Item) -> Option<toml::Value> {
        let mut doc = DocumentMut::new();
        doc.insert("value", item.clone());
        toml::from_str::<toml::Value>(&doc.to_string())
            .ok()
            .and_then(|root| root.get("value").cloned())
    }
    normalize(left) == normalize(right)
}

fn normalized_url(value: &str) -> &str {
    value.trim().trim_end_matches('/')
}

fn url_matches(actual: &str, expected: Option<&str>) -> bool {
    expected.is_some_and(|expected| normalized_url(actual) == normalized_url(expected))
}

fn document_bytes(doc: DocumentMut) -> Vec<u8> {
    let mut text = doc.to_string();
    if !text.ends_with('\n') {
        text.push('\n');
    }
    text.into_bytes()
}

fn provider_conflict(detail: &str) -> crate::shared::error::AppError {
    format!(
        "CODEX_REMOTE_COMPACTION_PROVIDER_CONFLICT: {detail}; rename or resolve the existing provider first"
    )
    .into()
}

fn owned_field_edit(path: &[String]) -> crate::shared::error::AppError {
    let path = if path.is_empty() {
        "config root".to_string()
    } else {
        path.join(".")
    };
    format!(
        "CODEX_PROXY_OWNED_FIELD_EDIT: {path} is managed while the Codex route is enabled; disable the route before editing it"
    )
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desired_key_uses_only_exact_features_boolean() {
        assert_eq!(
            desired_provider_key_from_config(b"[features]\nremote_compaction = true\n").unwrap(),
            CodexManagedProviderKey::OpenAi
        );
        assert_eq!(
            desired_provider_key_from_config(
                b"note = \"remote_compaction = true\"\n[other]\nremote_compaction = true\n"
            )
            .unwrap(),
            CodexManagedProviderKey::Aio
        );
    }

    #[test]
    fn reconcile_renames_quoted_provider_with_nested_data() {
        let input = br#"model_provider = "aio"

[model_providers.'aio']
name = "aio"
base_url = "http://127.0.0.1:37123/v1"
custom = "keep"

[model_providers.'aio'.headers]
team = "alpha"
"#;
        let output =
            reconcile_provider_identity(input, CodexManagedProviderKey::OpenAi, None).unwrap();
        let text = String::from_utf8(output).unwrap();
        assert!(text.contains("model_provider = \"OpenAI\""), "{text}");
        assert!(text.contains("[model_providers.OpenAI]"), "{text}");
        assert!(text.contains("[model_providers.OpenAI.headers]"), "{text}");
        assert!(text.contains("custom = \"keep\""), "{text}");
    }

    #[test]
    fn reconcile_deduplicates_equivalent_provider_subtrees() {
        let input = br#"model_provider = "aio"

[model_providers.aio]
name = "aio"
base_url = "http://127.0.0.1:37123/v1"
wire_api = "responses"
requires_openai_auth = true
custom = "same"

[model_providers.OpenAI]
name = "OpenAI"
base_url = "http://127.0.0.1:37123/v1/"
wire_api = "responses"
requires_openai_auth = true
custom = "same"
"#;
        let output = reconcile_provider_identity(
            input,
            CodexManagedProviderKey::OpenAi,
            Some("http://127.0.0.1:37123/v1"),
        )
        .unwrap();
        let doc = parse_document(&output).unwrap();
        let providers = doc["model_providers"].as_table_like().unwrap();
        assert!(providers.contains_key("OpenAI"));
        assert!(!providers.contains_key("aio"));
    }

    #[test]
    fn reconcile_rejects_conflicting_existing_target_without_leaking_url() {
        let input = br#"model_provider = "aio"

[model_providers.aio]
name = "aio"
base_url = "http://127.0.0.1:37123/v1"

[model_providers.OpenAI]
name = "OpenAI"
base_url = "https://private.example/v1"
"#;
        let error = reconcile_provider_identity(
            input,
            CodexManagedProviderKey::OpenAi,
            Some("http://127.0.0.1:37123/v1"),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("CODEX_REMOTE_COMPACTION_PROVIDER_CONFLICT"));
        assert!(error.contains("base_url"));
        assert!(!error.contains("private.example"));
    }

    #[test]
    fn active_projection_and_status_share_remote_provider_rule() {
        let input = b"[features]\nremote_compaction = true\n";
        let output = project_active_provider(input, "http://127.0.0.1:37123/v1", None).unwrap();
        assert!(is_managed_projection_applied(
            &output,
            "http://127.0.0.1:37123/v1"
        ));
        assert!(!is_managed_projection_applied(
            &output,
            "http://127.0.0.1:37124/v1"
        ));
    }

    #[test]
    fn active_projection_overlays_single_direct_provider_and_preserves_unknown_fields() {
        let input = br#"model_provider = "aio"

[model_providers.aio]
name = "aio"
base_url = "https://private.example/v1"
wire_api = "responses"
requires_openai_auth = true
custom = "keep"

[model_providers.aio.headers]
team = "alpha"

[features]
remote_compaction = true
"#;
        let output = project_active_provider(input, "http://127.0.0.1:37123/v1", None).unwrap();
        let doc = parse_document(&output).unwrap();
        assert_eq!(doc["model_provider"].as_str(), Some("OpenAI"));
        assert_eq!(
            doc["model_providers"]["OpenAI"]["base_url"].as_str(),
            Some("http://127.0.0.1:37123/v1")
        );
        assert_eq!(
            doc["model_providers"]["OpenAI"]["custom"].as_str(),
            Some("keep")
        );
        assert_eq!(
            doc["model_providers"]["OpenAI"]["headers"]["team"].as_str(),
            Some("alpha")
        );
    }

    #[test]
    fn active_projection_accepts_manifest_owned_previous_address_on_rebind() {
        let input = br#"model_provider = "OpenAI"

[model_providers.OpenAI]
name = "OpenAI"
base_url = "http://127.0.0.1:37122/v1"
wire_api = "responses"
requires_openai_auth = true

[features]
remote_compaction = true
"#;
        let output = project_active_provider(
            input,
            "http://127.0.0.1:37123/v1",
            Some("http://127.0.0.1:37122/v1"),
        )
        .unwrap();
        assert!(is_managed_projection_applied(
            &output,
            "http://127.0.0.1:37123/v1"
        ));
    }

    #[test]
    fn reconcile_copies_one_sided_managed_fields_without_losing_user_data() {
        let input = br#"model_provider = "aio"
[model_providers.aio]
name = "aio"
base_url = "http://127.0.0.1:37123/v1"
wire_api = "responses"
requires_openai_auth = true
source_only = "keep"

[model_providers.OpenAI]
name = "OpenAI"
target_only = "keep-too"
"#;
        let output = reconcile_provider_identity(
            input,
            CodexManagedProviderKey::OpenAi,
            Some("http://127.0.0.1:37123/v1"),
        )
        .unwrap();
        let doc = parse_document(&output).unwrap();
        let provider = doc["model_providers"]["OpenAI"].as_table_like().unwrap();
        assert_eq!(
            provider.get("base_url").and_then(Item::as_str),
            Some("http://127.0.0.1:37123/v1")
        );
        assert_eq!(
            provider.get("wire_api").and_then(Item::as_str),
            Some("responses")
        );
        assert_eq!(
            provider.get("requires_openai_auth").and_then(Item::as_bool),
            Some(true)
        );
        assert_eq!(
            provider.get("source_only").and_then(Item::as_str),
            Some("keep")
        );
        assert_eq!(
            provider.get("target_only").and_then(Item::as_str),
            Some("keep-too")
        );
    }

    #[test]
    fn reconcile_supports_dotted_inline_and_double_quoted_provider_shapes() {
        let cases: &[&[u8]] = &[
            br#"model_provider = "aio"
model_providers.aio.name = "aio"
model_providers.aio.base_url = "https://direct.example/v1"
model_providers.aio.headers.team = "alpha"
"#,
            br#"model_provider = "aio"
model_providers = { aio = { name = "aio", base_url = "https://direct.example/v1", headers = { team = "alpha" } } }
"#,
            br#"# keep root comment
model_provider = "aio"

[model_providers."aio"] # keep provider comment
name = "aio"
base_url = "https://direct.example/v1"
# keep user comment
custom = "value"

[model_providers."aio".headers]
team = "alpha"
"#,
        ];

        for (index, input) in cases.iter().enumerate() {
            let output =
                reconcile_provider_identity(input, CodexManagedProviderKey::OpenAi, None).unwrap();
            let text = std::str::from_utf8(&output).unwrap();
            let doc = parse_document(&output).unwrap();
            let providers = doc["model_providers"].as_table_like().unwrap();
            assert_eq!(providers.len(), 1, "{text}");
            assert!(!providers.contains_key("aio"), "{text}");
            let provider = providers
                .get("OpenAI")
                .and_then(Item::as_table_like)
                .unwrap();
            assert_eq!(
                provider.get("base_url").and_then(Item::as_str),
                Some("https://direct.example/v1")
            );
            assert_eq!(
                provider
                    .get("headers")
                    .and_then(Item::as_table_like)
                    .and_then(|headers| headers.get("team"))
                    .and_then(Item::as_str),
                Some("alpha")
            );
            if index == 2 {
                assert!(text.contains("# keep root comment"), "{text}");
                assert!(text.contains("# keep provider comment"), "{text}");
                assert!(text.contains("# keep user comment"), "{text}");
            }
        }
    }

    #[test]
    fn raw_three_way_merge_preserves_baseline_and_accepts_user_fields() {
        let baseline = br#"model = "before"
model_provider = "direct"

[model_providers.direct]
name = "direct"
base_url = "https://example.invalid/v1"
"#;
        let live = br#"model = "before"
model_provider = "aio"

[model_providers.aio]
name = "aio"
base_url = "http://127.0.0.1:37123/v1"
wire_api = "responses"
requires_openai_auth = true
"#;
        let submitted = br#"model = "after"
model_provider = "aio"

[model_providers.aio]
name = "aio"
base_url = "http://127.0.0.1:37123/v1"
wire_api = "responses"
requires_openai_auth = true
custom = "keep"
"#;
        let live_doc = parse_document(live).unwrap();
        let submitted_doc = parse_document(submitted).unwrap();
        assert!(live_doc["model_providers"]["aio"].as_table_like().is_some());
        assert!(submitted_doc["model_providers"]["aio"]
            .as_table_like()
            .is_some());
        let output = merge_raw_user_changes(baseline, live, live, submitted).unwrap();
        let text = String::from_utf8(output).unwrap();
        assert!(text.contains("model = \"after\""), "{text}");
        assert!(text.contains("model_provider = \"direct\""), "{text}");
        assert!(text.contains("[model_providers.direct]"), "{text}");
        assert!(text.contains("custom = \"keep\""), "{text}");
        assert!(!text.contains("127.0.0.1"), "{text}");
    }

    #[test]
    fn raw_three_way_merge_rejects_proxy_owned_field_edits() {
        let baseline = b"model = \"before\"\n";
        let live = br#"model = "before"
model_provider = "aio"
[model_providers.aio]
name = "aio"
base_url = "http://127.0.0.1:37123/v1"
wire_api = "responses"
requires_openai_auth = true
"#;
        let submitted = br#"model = "before"
model_provider = "aio"
[model_providers.aio]
name = "aio"
base_url = "https://user-change.invalid/v1"
wire_api = "responses"
requires_openai_auth = true
"#;
        let error = merge_raw_user_changes(baseline, live, live, submitted)
            .unwrap_err()
            .to_string();
        assert!(error.contains("CODEX_PROXY_OWNED_FIELD_EDIT"));
        assert!(error.contains("base_url"));
        assert!(!error.contains("user-change.invalid"));
    }

    #[test]
    fn raw_three_way_merge_rejects_bulk_owned_provider_add_or_delete() {
        let baseline = b"model = \"before\"\n";
        let live = br#"model = "before"
model_provider = "aio"
[model_providers.aio]
name = "aio"
base_url = "http://127.0.0.1:37123/v1"
wire_api = "responses"
requires_openai_auth = true
"#;
        let deleted = b"model = \"before\"\n";
        let error = merge_raw_user_changes(baseline, live, live, deleted)
            .unwrap_err()
            .to_string();
        assert!(error.contains("CODEX_PROXY_OWNED_FIELD_EDIT"), "{error}");

        let added = br#"model = "before"
model_provider = "aio"
[model_providers.aio]
name = "aio"
base_url = "http://127.0.0.1:37123/v1"
wire_api = "responses"
requires_openai_auth = true
[model_providers.OpenAI]
name = "OpenAI"
base_url = "https://user.invalid/v1"
"#;
        let error = merge_raw_user_changes(baseline, live, live, added)
            .unwrap_err()
            .to_string();
        assert!(error.contains("CODEX_PROXY_OWNED_FIELD_EDIT"), "{error}");
        assert!(!error.contains("user.invalid"), "{error}");
    }

    #[test]
    fn restore_projection_removes_active_only_openai_and_restores_baseline_provider() {
        let baseline = br#"model_provider = "aio"
[model_providers.aio]
name = "aio"
base_url = "https://direct.example/v1"
custom = "before"
"#;
        let current = br#"model_provider = "OpenAI"
[model_providers.OpenAI]
name = "OpenAI"
base_url = "http://127.0.0.1:37123/v1"
wire_api = "responses"
requires_openai_auth = true
"#;
        let output = restore_managed_provider_projection(current, baseline).unwrap();
        let text = String::from_utf8(output).unwrap();
        assert!(text.contains("model_provider = \"aio\""), "{text}");
        assert!(text.contains("[model_providers.aio]"), "{text}");
        assert!(text.contains("https://direct.example/v1"), "{text}");
        assert!(!text.contains("[model_providers.OpenAI]"), "{text}");
        assert!(!text.contains("127.0.0.1"), "{text}");
    }
}
