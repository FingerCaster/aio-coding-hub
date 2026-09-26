//! Strict, bounded native parsing. Parser diagnostics never include source text.
use crate::domain::native_cli::{
    validate_native_key, validate_provider, NativeClient, NativeFormat,
};
use crate::shared::error::{AppError, AppResult};
use serde::de::{DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Number, Value};
use sha2::{Digest, Sha256};
use std::cell::Cell;
use std::fmt;

pub(crate) const MAX_DOCUMENT_BYTES: usize = 4 * 1024 * 1024;

pub(crate) fn digest_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub(crate) fn node_digest(node: &Value) -> String {
    // Stable even when a transitive dependency enables serde_json/preserve_order.
    fn canonical(value: &Value) -> Value {
        match value {
            Value::Object(map) => {
                let mut keys: Vec<_> = map.keys().collect();
                keys.sort();
                Value::Object(
                    keys.into_iter()
                        .map(|key| (key.clone(), canonical(&map[key])))
                        .collect(),
                )
            }
            Value::Array(values) => Value::Array(values.iter().map(canonical).collect()),
            value => value.clone(),
        }
    }
    digest_bytes(&serde_json::to_vec(&canonical(node)).expect("JSON values serialize"))
}

fn invalid() -> AppError {
    AppError::new(
        "NATIVE_PARSE_INVALID",
        "Native model document is invalid or has ambiguous syntax; original file was not changed",
    )
}

pub(crate) fn parse(client: NativeClient, format: NativeFormat, bytes: &[u8]) -> AppResult<Value> {
    let root = parse_mapping(format, bytes)?;
    validate_document(client, &root)?;
    Ok(root)
}

/// Strict mapping parser shared with settings/frontmatter; no provider semantics.
pub(crate) fn parse_mapping(format: NativeFormat, bytes: &[u8]) -> AppResult<Value> {
    if bytes.len() > MAX_DOCUMENT_BYTES {
        return Err(AppError::new(
            "NATIVE_FILE_TOO_LARGE",
            "Native model document exceeds 4 MiB",
        ));
    }
    let text = std::str::from_utf8(bytes).map_err(|_| invalid())?;
    let text = text.strip_prefix('﻿').unwrap_or(text);
    let count = Cell::new(0);
    let seed = StrictValue {
        depth: 0,
        count: &count,
    };
    let root = match format {
        NativeFormat::Jsonc | NativeFormat::LegacyJson => {
            let stripped = strip_json_comments(text)?;
            let mut parser = serde_json::Deserializer::from_slice(&stripped);
            let value = seed.deserialize(&mut parser).map_err(|_| invalid())?;
            parser.end().map_err(|_| invalid())?;
            value
        }
        NativeFormat::Yaml => {
            reject_yaml_references(text)?;
            let mut docs = serde_yaml::Deserializer::from_str(text);
            let first = docs.next().ok_or_else(invalid)?;
            let value = seed.deserialize(first).map_err(|_| invalid())?;
            if docs.next().is_some() {
                return Err(invalid());
            }
            value
        }
    };
    if !root.is_object() {
        return Err(invalid());
    }
    Ok(root)
}

pub(crate) fn validate_document(client: NativeClient, root: &Value) -> AppResult<()> {
    if !root.is_object() {
        return Err(invalid());
    }
    match root.get("providers") {
        None if client == NativeClient::Omp => Ok(()),
        Some(Value::Object(providers)) => {
            if providers.len() > 4096 {
                return Err(invalid());
            }
            for (key, node) in providers {
                validate_native_key(key)?;
                validate_provider(client, node)?;
            }
            Ok(())
        }
        _ => Err(invalid()),
    }
}

pub(crate) fn serialize(
    client: NativeClient,
    format: NativeFormat,
    root: &Value,
) -> AppResult<Vec<u8>> {
    validate_document(client, root)?;
    let text = match format {
        NativeFormat::Jsonc => serde_json::to_string_pretty(root).map_err(|_| invalid())? + "\n",
        NativeFormat::Yaml => serde_yaml::to_string(root).map_err(|_| invalid())?,
        NativeFormat::LegacyJson => {
            return Err(AppError::new(
                "NATIVE_LEGACY_READ_ONLY",
                "Migrate OMP legacy JSON with the native CLI before editing",
            ))
        }
    };
    let reparsed = parse(client, format, text.as_bytes())?;
    if reparsed != *root {
        return Err(invalid());
    }
    Ok(text.into_bytes())
}

fn strip_json_comments(text: &str) -> AppResult<Vec<u8>> {
    let mut out = text.as_bytes().to_vec();
    let mut i = 0;
    let mut quoted = false;
    while i < out.len() {
        if quoted {
            if out[i] == b'\\' {
                i += 2;
                continue;
            }
            if out[i] == b'"' {
                quoted = false;
            }
            i += 1;
            continue;
        }
        if out[i] == b'"' {
            quoted = true;
            i += 1;
            continue;
        }
        if out[i] == b'/' && out.get(i + 1) == Some(&b'/') {
            while i < out.len() && out[i] != b'\n' {
                out[i] = b' ';
                i += 1;
            }
        } else if out[i] == b'/' && out.get(i + 1) == Some(&b'*') {
            // Pi's native helper only supports // comments, not JSON5 blocks.
            return Err(invalid());
        } else {
            i += 1;
        }
    }
    // Native stripJsonComments removes trailing commas after removing comments.
    // Keep string contents untouched, including apparent delimiters and URLs.
    i = 0;
    quoted = false;
    while i < out.len() {
        if quoted {
            if out[i] == b'\\' {
                i += 2;
                continue;
            }
            if out[i] == b'"' {
                quoted = false;
            }
        } else if out[i] == b'"' {
            quoted = true;
        } else if out[i] == b',' {
            let mut next = i + 1;
            while next < out.len() && out[next].is_ascii_whitespace() {
                next += 1;
            }
            if matches!(out.get(next), Some(b'}' | b']')) {
                out[i] = b' ';
            }
        }
        i += 1;
    }
    Ok(out)
}

/// Inspect actual YAML tokens, not quote heuristics: plain scalars may contain
/// quotes and multiline/block strings may contain apparent anchor syntax.
fn reject_yaml_references(text: &str) -> AppResult<()> {
    use yaml_rust2::scanner::{Scanner, TokenType};
    let mut scanner = Scanner::new(text.chars());
    while let Some(token) = scanner.next_token().map_err(|_| invalid())? {
        if matches!(
            token.1,
            TokenType::Anchor(_)
                | TokenType::Alias(_)
                | TokenType::Tag(_, _)
                | TokenType::TagDirective(_, _)
                | TokenType::VersionDirective(_, _)
        ) {
            return Err(AppError::new(
                "NATIVE_YAML_READ_ONLY",
                "YAML anchors, aliases, tags and version directives require native editing",
            ));
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct StrictValue<'a> {
    depth: usize,
    count: &'a Cell<usize>,
}
impl<'de> DeserializeSeed<'de> for StrictValue<'_> {
    type Value = Value;
    fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> Result<Value, D::Error> {
        self.count.set(self.count.get() + 1);
        if self.depth > 64 || self.count.get() > 100_000 {
            return Err(serde::de::Error::custom("document complexity limit"));
        }
        d.deserialize_any(self)
    }
}
impl<'de> Visitor<'de> for StrictValue<'_> {
    type Value = Value;
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("unambiguous JSON-compatible native data")
    }
    fn visit_unit<E: serde::de::Error>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_none<E: serde::de::Error>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_bool<E: serde::de::Error>(self, v: bool) -> Result<Value, E> {
        Ok(Value::Bool(v))
    }
    fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Value, E> {
        Ok(Value::Number(v.into()))
    }
    fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Value, E> {
        Ok(Value::Number(v.into()))
    }
    fn visit_f64<E: serde::de::Error>(self, v: f64) -> Result<Value, E> {
        Number::from_f64(v)
            .map(Value::Number)
            .ok_or_else(|| E::custom("nonfinite value"))
    }
    fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Value, E> {
        Ok(Value::String(v.to_string()))
    }
    fn visit_string<E: serde::de::Error>(self, v: String) -> Result<Value, E> {
        Ok(Value::String(v))
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Value, A::Error> {
        let child = Self {
            depth: self.depth + 1,
            count: self.count,
        };
        let mut values = Vec::new();
        while let Some(value) = seq.next_element_seed(child)? {
            values.push(value);
        }
        Ok(Value::Array(values))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Value, A::Error> {
        let child = Self {
            depth: self.depth + 1,
            count: self.count,
        };
        let mut values = Map::new();
        while let Some(key) = map.next_key_seed(child)? {
            let Value::String(key) = key else {
                return Err(serde::de::Error::custom("non-string key"));
            };
            if key == "<<" || values.contains_key(&key) {
                return Err(serde::de::Error::custom("duplicate or merge key"));
            }
            let value = map.next_value_seed(child)?;
            values.insert(key, value);
        }
        Ok(Value::Object(values))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pi_supports_bom_comments_but_not_json5() {
        let node = parse(NativeClient::Pi, NativeFormat::Jsonc, "﻿{// hi\n\"providers\":{\"builtin\":{\"api\":\"extension\",\"baseUrl\":\"https://example.invalid//path\",\"future\":[1,],},},}// tail".as_bytes()).unwrap();
        assert_eq!(node["providers"]["builtin"]["api"], "extension");
        for text in [
            "{providers:{}}",
            "{'providers':{}}",
            "{/* native does not support blocks */\"providers\":{}}",
            "{\"providers\":{},,}",
            "{\"providers\":{},\"providers\":{}}",
            "/* broken",
            "",
        ] {
            assert!(parse(NativeClient::Pi, NativeFormat::Jsonc, text.as_bytes()).is_err());
        }
    }
    #[test]
    fn yaml_rejects_ambiguous_inputs_without_echoing_secrets() {
        for text in [
            "providers: {}\nproviders: {}",
            "providers:\n  a: &secret {auth: none}\n  b: *secret",
            "providers: {a: {<<: {auth: none}}}",
            "providers: {}\n---\nproviders: {}",
            "providers: {}\nother: {1: x}",
            "providers: {}\nsecret: !!str token",
        ] {
            let err = parse(NativeClient::Omp, NativeFormat::Yaml, text.as_bytes()).unwrap_err();
            assert!(!err.to_string().contains("token"));
        }
    }
    #[test]
    fn yaml_roundtrip_preserves_unknown_maps_and_literal_expressions() {
        let root = parse(NativeClient::Omp, NativeFormat::Yaml, b"providers:\n  openai:\n    apiKey: '!literal command'\n    future: {nested: [x, 42, true]}\nfutureRoot: keep\n").unwrap();
        assert_eq!(
            parse(
                NativeClient::Omp,
                NativeFormat::Yaml,
                &serialize(NativeClient::Omp, NativeFormat::Yaml, &root).unwrap()
            )
            .unwrap(),
            root
        );
    }
    #[test]
    fn yaml_scanner_distinguishes_literals_from_graph_syntax() {
        let text = "providers: {p: {auth: none}}\nnote: |\n  \"unmatched quote with &literal and *literal\n";
        assert!(parse(NativeClient::Omp, NativeFormat::Yaml, text.as_bytes()).is_ok());
        let tricky = "providers: {p: {auth: none}}\nextra: {text: plain \"quote, anchor: &a {value: true}, alias: *a, closing: quote\"}\n";
        assert_eq!(
            parse(NativeClient::Omp, NativeFormat::Yaml, tricky.as_bytes())
                .unwrap_err()
                .code(),
            "NATIVE_YAML_READ_ONLY"
        );
        assert!(parse(
            NativeClient::Omp,
            NativeFormat::Yaml,
            b"%YAML 1.1\n---\nproviders: {}"
        )
        .is_err());
    }
    #[test]
    fn yaml_json_numbers_and_unknown_values_roundtrip_without_coercion() {
        let root = serde_json::json!({"providers":{"p":{"auth":"none","future":{"yes":"yes","null":"null","integer":42,"float":1.5,"text":"001"}}}});
        let encoded = serialize(NativeClient::Omp, NativeFormat::Yaml, &root).unwrap();
        assert_eq!(
            parse(NativeClient::Omp, NativeFormat::Yaml, &encoded).unwrap(),
            root
        );
    }
}
