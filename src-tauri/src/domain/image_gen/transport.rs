//! Usage: Pure transport layer for the image generation page: path allowlist,
//! scheme checks, private-host rejection, and body-size caps. No provider semantics.

use base64::Engine as _;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE, LOCATION};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::Duration;

const ALLOWED_IMAGE_GEN_PATHS: &[&str] = &["/v1/images/generations", "/v1/images/edits"];
const MAX_RESPONSE_BYTES: usize = 32 * 1024 * 1024;
const MAX_MULTIPART_TOTAL_BYTES: usize = 64 * 1024 * 1024;
const MAX_MULTIPART_FILES: usize = 32;
const MAX_MULTIPART_FIELDS: usize = 64;
const MAX_MULTIPART_FIELD_NAME_BYTES: usize = 128;
const MAX_MULTIPART_FIELD_VALUE_BYTES: usize = 1024 * 1024;
const MAX_MULTIPART_FILENAME_BYTES: usize = 255;
const MAX_MULTIPART_MIME_BYTES: usize = 128;
const MAX_MULTIPART_TEXT_TOTAL_BYTES: usize = 8 * 1024 * 1024;
const DEFAULT_TIMEOUT_SECS: u64 = 600;
const MAX_TIMEOUT_SECS: u64 = 900;
const MAX_ERROR_EXCERPT_CHARS: usize = 512;
const MAX_ERROR_RESPONSE_BYTES: usize = 8 * 1024;
const MAX_IMAGE_REDIRECTS: usize = 5;

#[derive(Debug, Clone, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ImageGenHttpResponse {
    pub status: u16,
    pub body_text: String,
}

#[derive(Debug, Clone, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ImageGenFetchedImage {
    pub mime: String,
    pub data_b64: String,
}

#[derive(Debug, Clone, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ImageGenMultipartFile {
    pub field: String,
    pub filename: String,
    pub mime: String,
    pub data_b64: String,
}

#[derive(Debug)]
pub(super) struct DecodedMultipartFile {
    pub field: String,
    pub filename: String,
    pub mime: String,
    pub bytes: Vec<u8>,
}

pub(super) fn resolve_timeout(timeout_secs: Option<u32>) -> Duration {
    let secs = timeout_secs
        .map(u64::from)
        .unwrap_or(DEFAULT_TIMEOUT_SECS)
        .clamp(1, MAX_TIMEOUT_SECS);
    Duration::from_secs(secs)
}

pub(super) fn validate_request_path(path: &str) -> Result<&str, String> {
    if ALLOWED_IMAGE_GEN_PATHS.contains(&path) {
        Ok(path)
    } else {
        Err(format!("SEC_INVALID_INPUT: path is not allowed: {path}"))
    }
}

/// Joins the stored base_url with an allowlisted path. Scheme must be https;
/// http is only allowed for 127.0.0.1 / localhost (local gateway debugging).
/// A base_url that already ends with `/v1` is deduplicated against the `/v1`
/// path prefix so `https://host/v1` + `/v1/images/generations` does not double up.
pub(super) fn build_request_url(base_url: &str, path: &str) -> Result<reqwest::Url, String> {
    let base_url = base_url.trim().trim_end_matches('/');
    if base_url.is_empty() {
        return Err("SEC_INVALID_INPUT: image gen base_url is not configured".to_string());
    }

    let parsed = reqwest::Url::parse(base_url)
        .map_err(|e| format!("SEC_INVALID_INPUT: invalid base_url: {e}"))?;
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("SEC_INVALID_INPUT: base_url credentials are not allowed".to_string());
    }
    if parsed.query().is_some() || parsed.fragment().is_some() {
        return Err("SEC_INVALID_INPUT: base_url query and fragment are not allowed".to_string());
    }
    match parsed.scheme() {
        "https" => {}
        "http" => {
            let host = parsed.host_str().unwrap_or("");
            if host != "127.0.0.1" && host != "localhost" {
                return Err(
                    "SEC_INVALID_INPUT: base_url must use https (http is only allowed for 127.0.0.1/localhost)"
                        .to_string(),
                );
            }
        }
        other => {
            return Err(format!(
                "SEC_INVALID_INPUT: unsupported base_url scheme: {other}"
            ));
        }
    }

    let joined = if base_url.ends_with("/v1") {
        format!("{base_url}{}", path.trim_start_matches("/v1"))
    } else {
        format!("{base_url}{path}")
    };
    reqwest::Url::parse(&joined).map_err(|e| format!("SEC_INVALID_INPUT: invalid request url: {e}"))
}

fn auth_headers(api_key: &str) -> Result<HeaderMap, String> {
    let api_key = api_key.trim();
    if api_key.is_empty() {
        return Err("SEC_INVALID_INPUT: image gen api_key is not configured".to_string());
    }
    let mut value = HeaderValue::from_str(&format!("Bearer {api_key}"))
        .map_err(|_| "SEC_INVALID_INPUT: api_key contains invalid characters".to_string())?;
    value.set_sensitive(true);
    let mut headers = HeaderMap::new();
    headers.insert(AUTHORIZATION, value);
    Ok(headers)
}

fn truncate_excerpt(text: &str) -> String {
    text.chars().take(MAX_ERROR_EXCERPT_CHARS).collect()
}

async fn read_body_with_limit(
    response: &mut reqwest::Response,
    limit: usize,
) -> Result<Vec<u8>, String> {
    let mut buf = Vec::new();
    loop {
        match response.chunk().await {
            Ok(Some(chunk)) => {
                if buf.len().saturating_add(chunk.len()) > limit {
                    return Err(format!(
                        "HTTP_ERROR: response body exceeds {limit} bytes limit"
                    ));
                }
                buf.extend_from_slice(&chunk);
            }
            Ok(None) => break,
            Err(e) => return Err(safe_reqwest_error("read response body", &e)),
        }
    }
    Ok(buf)
}

async fn read_body_capped(response: &mut reqwest::Response) -> Result<Vec<u8>, String> {
    read_body_with_limit(response, MAX_RESPONSE_BYTES).await
}

async fn read_http_response(
    mut response: reqwest::Response,
) -> Result<ImageGenHttpResponse, String> {
    let status = response.status().as_u16();
    let body = read_body_capped(&mut response).await?;
    Ok(ImageGenHttpResponse {
        status,
        body_text: String::from_utf8_lossy(&body).into_owned(),
    })
}

pub(crate) async fn post_json(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    path: &str,
    body: &serde_json::Value,
    timeout_secs: Option<u32>,
) -> Result<ImageGenHttpResponse, String> {
    let path = validate_request_path(path)?;
    let url = build_request_url(base_url, path)?;
    let headers = auth_headers(api_key)?;
    let body_bytes = serde_json::to_vec(body)
        .map_err(|e| format!("SYSTEM_ERROR: failed to encode body JSON: {e}"))?;

    let response = client
        .post(url)
        .headers(headers)
        .header(CONTENT_TYPE, "application/json")
        .body(body_bytes)
        .timeout(resolve_timeout(timeout_secs))
        .send()
        .await
        .map_err(|e| safe_reqwest_error("send image generation request", &e))?;

    read_http_response(response).await
}

pub(super) fn decode_multipart_files(
    files: &[ImageGenMultipartFile],
) -> Result<Vec<DecodedMultipartFile>, String> {
    validate_multipart_files(files)?;
    let mut decoded = Vec::with_capacity(files.len());
    let mut total_bytes = 0usize;
    for (index, file) in files.iter().enumerate() {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(file.data_b64.as_bytes())
            .map_err(|e| format!("SEC_INVALID_INPUT: file #{index} data_b64 is invalid: {e}"))?;
        total_bytes = total_bytes
            .checked_add(bytes.len())
            .ok_or_else(|| "SEC_INVALID_INPUT: multipart file size overflow".to_string())?;
        if total_bytes > MAX_MULTIPART_TOTAL_BYTES {
            return Err(format!(
                "SEC_INVALID_INPUT: multipart files exceed {MAX_MULTIPART_TOTAL_BYTES} bytes limit"
            ));
        }
        decoded.push(DecodedMultipartFile {
            field: file.field.clone(),
            filename: file.filename.clone(),
            mime: file.mime.clone(),
            bytes,
        });
    }
    Ok(decoded)
}

fn decoded_base64_len(data_b64: &str, index: usize) -> Result<usize, String> {
    let len = data_b64.len();
    if len == 0 || !len.is_multiple_of(4) {
        return Err(format!(
            "SEC_INVALID_INPUT: file #{index} data_b64 has an invalid length"
        ));
    }
    let padding = data_b64
        .as_bytes()
        .iter()
        .rev()
        .take_while(|byte| **byte == b'=')
        .count();
    if padding > 2 {
        return Err(format!(
            "SEC_INVALID_INPUT: file #{index} data_b64 has invalid padding"
        ));
    }
    len.checked_div(4)
        .and_then(|chunks| chunks.checked_mul(3))
        .and_then(|bytes| bytes.checked_sub(padding))
        .ok_or_else(|| "SEC_INVALID_INPUT: multipart file size overflow".to_string())
}

pub(super) fn validate_multipart_files(files: &[ImageGenMultipartFile]) -> Result<(), String> {
    if files.len() > MAX_MULTIPART_FILES {
        return Err(format!(
            "SEC_INVALID_INPUT: too many multipart files (max {MAX_MULTIPART_FILES})"
        ));
    }
    let mut decoded_total = 0usize;
    for (index, file) in files.iter().enumerate() {
        if file.field.trim().is_empty() {
            return Err(format!(
                "SEC_INVALID_INPUT: file #{index} field is required"
            ));
        }
        if file.field.len() > MAX_MULTIPART_FIELD_NAME_BYTES {
            return Err(format!(
                "SEC_INVALID_INPUT: file #{index} field is too long"
            ));
        }
        if file.filename.trim().is_empty() {
            return Err(format!(
                "SEC_INVALID_INPUT: file #{index} filename is required"
            ));
        }
        if file.filename.len() > MAX_MULTIPART_FILENAME_BYTES {
            return Err(format!(
                "SEC_INVALID_INPUT: file #{index} filename is too long"
            ));
        }
        if file.mime.trim().is_empty() || file.mime.len() > MAX_MULTIPART_MIME_BYTES {
            return Err(format!("SEC_INVALID_INPUT: file #{index} mime is invalid"));
        }
        let decoded_len = decoded_base64_len(&file.data_b64, index)?;
        if decoded_len > MAX_MULTIPART_TOTAL_BYTES {
            return Err(format!(
                "SEC_INVALID_INPUT: multipart files exceed {MAX_MULTIPART_TOTAL_BYTES} bytes limit"
            ));
        }
        decoded_total = decoded_total
            .checked_add(decoded_len)
            .ok_or_else(|| "SEC_INVALID_INPUT: multipart file size overflow".to_string())?;
        if decoded_total > MAX_MULTIPART_TOTAL_BYTES {
            return Err(format!(
                "SEC_INVALID_INPUT: multipart files exceed {MAX_MULTIPART_TOTAL_BYTES} bytes limit"
            ));
        }
    }
    Ok(())
}

pub(super) fn validate_multipart_fields(fields: &[(String, String)]) -> Result<(), String> {
    if fields.len() > MAX_MULTIPART_FIELDS {
        return Err(format!(
            "SEC_INVALID_INPUT: too many multipart fields (max {MAX_MULTIPART_FIELDS})"
        ));
    }
    let mut total = 0usize;
    for (index, (name, value)) in fields.iter().enumerate() {
        if name.trim().is_empty() || name.len() > MAX_MULTIPART_FIELD_NAME_BYTES {
            return Err(format!("SEC_INVALID_INPUT: field #{index} name is invalid"));
        }
        if value.len() > MAX_MULTIPART_FIELD_VALUE_BYTES {
            return Err(format!(
                "SEC_INVALID_INPUT: field #{index} value is too long"
            ));
        }
        total = total
            .checked_add(name.len())
            .and_then(|size| size.checked_add(value.len()))
            .ok_or_else(|| "SEC_INVALID_INPUT: multipart field size overflow".to_string())?;
        if total > MAX_MULTIPART_TEXT_TOTAL_BYTES {
            return Err(format!(
                "SEC_INVALID_INPUT: multipart fields exceed {MAX_MULTIPART_TEXT_TOTAL_BYTES} bytes limit"
            ));
        }
    }
    Ok(())
}

fn build_multipart_form(
    fields: &[(String, String)],
    files: Vec<DecodedMultipartFile>,
) -> Result<reqwest::multipart::Form, String> {
    let mut form = reqwest::multipart::Form::new();
    for (name, value) in fields {
        form = form.text(name.clone(), value.clone());
    }
    for file in files {
        let part = reqwest::multipart::Part::bytes(file.bytes)
            .file_name(file.filename)
            .mime_str(&file.mime)
            .map_err(|e| format!("SEC_INVALID_INPUT: invalid mime type {}: {e}", file.mime))?;
        form = form.part(file.field, part);
    }
    Ok(form)
}

pub(crate) async fn post_multipart(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    path: &str,
    fields: &[(String, String)],
    files: &[ImageGenMultipartFile],
    timeout_secs: Option<u32>,
) -> Result<ImageGenHttpResponse, String> {
    let path = validate_request_path(path)?;
    let url = build_request_url(base_url, path)?;
    let headers = auth_headers(api_key)?;
    validate_multipart_fields(fields)?;
    let form = build_multipart_form(fields, decode_multipart_files(files)?)?;

    let response = client
        .post(url)
        .headers(headers)
        .multipart(form)
        .timeout(resolve_timeout(timeout_secs))
        .send()
        .await
        .map_err(|e| safe_reqwest_error("send image generation request", &e))?;

    read_http_response(response).await
}

pub(super) fn is_disallowed_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => !is_global_ipv4(v4),
        IpAddr::V6(v6) => {
            if let Some(mapped) = v6.to_ipv4_mapped() {
                return !is_global_ipv4(mapped);
            }
            !is_global_ipv6(v6)
        }
    }
}

fn ipv4_in(ip: Ipv4Addr, network: [u8; 4], prefix: u32) -> bool {
    let mask = if prefix == 0 {
        0
    } else {
        u32::MAX << (32 - prefix)
    };
    u32::from(ip) & mask == u32::from(Ipv4Addr::from(network)) & mask
}

fn ipv6_in(ip: Ipv6Addr, network: [u16; 8], prefix: u32) -> bool {
    let mask = if prefix == 0 {
        0
    } else {
        u128::MAX << (128 - prefix)
    };
    u128::from(ip) & mask == u128::from(Ipv6Addr::from(network)) & mask
}

fn is_global_ipv4(ip: Ipv4Addr) -> bool {
    if matches!(ip.octets(), [192, 0, 0, 9] | [192, 0, 0, 10]) {
        return true;
    }
    ![
        ([0, 0, 0, 0], 8),
        ([10, 0, 0, 0], 8),
        ([100, 64, 0, 0], 10),
        ([127, 0, 0, 0], 8),
        ([169, 254, 0, 0], 16),
        ([172, 16, 0, 0], 12),
        ([192, 0, 0, 0], 24),
        ([192, 0, 2, 0], 24),
        ([192, 88, 99, 0], 24),
        ([192, 168, 0, 0], 16),
        ([198, 18, 0, 0], 15),
        ([198, 51, 100, 0], 24),
        ([203, 0, 113, 0], 24),
        ([224, 0, 0, 0], 4),
        ([240, 0, 0, 0], 4),
    ]
    .into_iter()
    .any(|(network, prefix)| ipv4_in(ip, network, prefix))
}

fn is_global_ipv6(ip: Ipv6Addr) -> bool {
    ![
        ([0, 0, 0, 0, 0, 0, 0, 0], 128),
        ([0, 0, 0, 0, 0, 0, 0, 1], 128),
        ([0x0064, 0xff9b, 0x0001, 0, 0, 0, 0, 0], 48),
        ([0x0100, 0, 0, 0, 0, 0, 0, 0], 64),
        ([0x2001, 0, 0, 0, 0, 0, 0, 0], 23),
        ([0x2001, 0x0db8, 0, 0, 0, 0, 0, 0], 32),
        ([0x3fff, 0, 0, 0, 0, 0, 0, 0], 20),
        ([0x5f00, 0, 0, 0, 0, 0, 0, 0], 16),
        ([0xfc00, 0, 0, 0, 0, 0, 0, 0], 7),
        ([0xfe80, 0, 0, 0, 0, 0, 0, 0], 10),
        ([0xfec0, 0, 0, 0, 0, 0, 0, 0], 10),
        ([0xff00, 0, 0, 0, 0, 0, 0, 0], 8),
    ]
    .into_iter()
    .any(|(network, prefix)| ipv6_in(ip, network, prefix))
}

fn bare_host(url: &reqwest::Url) -> Result<&str, String> {
    let host = url
        .host_str()
        .ok_or_else(|| "SEC_INVALID_INPUT: image url is missing a host".to_string())?;
    Ok(host.trim_start_matches('[').trim_end_matches(']'))
}

/// Static checks only (scheme + IP-literal hosts). Hostname DNS resolution is
/// covered by `ensure_public_host`.
pub(super) fn validate_fetch_image_url(url: &str) -> Result<reqwest::Url, String> {
    let url = reqwest::Url::parse(url.trim())
        .map_err(|e| format!("SEC_INVALID_INPUT: invalid image url: {e}"))?;
    if url.scheme() != "https" {
        return Err("SEC_INVALID_INPUT: image url must use https".to_string());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("SEC_INVALID_INPUT: image url credentials are not allowed".to_string());
    }
    if url.port_or_known_default() != Some(443) {
        return Err("SEC_INVALID_INPUT: image url port must be 443".to_string());
    }
    let host = bare_host(&url)?;
    if host.parse::<IpAddr>().is_ok() {
        return Err("SEC_INVALID_INPUT: image url IP literals are not allowed".to_string());
    }
    Ok(url)
}

async fn resolve_public_addrs(url: &reqwest::Url) -> Result<(String, Vec<SocketAddr>), String> {
    let host = bare_host(url)?;
    let port = url.port_or_known_default().ok_or_else(|| {
        "SEC_INVALID_INPUT: image url does not have a valid connection port".to_string()
    })?;
    if host.parse::<IpAddr>().is_ok() {
        return Err("SEC_INVALID_INPUT: image url IP literals are not allowed".to_string());
    }
    let addrs = tokio::net::lookup_host((host, port))
        .await
        .map_err(|_| "HTTP_ERROR: failed to resolve image host".to_string())?
        .collect::<Vec<_>>();
    Ok((host.to_string(), validate_public_addrs(host, addrs)?))
}

pub(super) fn validate_public_addrs(
    host: &str,
    addrs: impl IntoIterator<Item = SocketAddr>,
) -> Result<Vec<SocketAddr>, String> {
    let mut resolved = Vec::new();
    for addr in addrs {
        if is_disallowed_ip(addr.ip()) {
            return Err(format!(
                "SEC_INVALID_INPUT: image url host resolves to a non-global address: {host}"
            ));
        }
        if !resolved.contains(&addr) {
            resolved.push(addr);
        }
    }
    if resolved.is_empty() {
        return Err(format!("HTTP_ERROR: image host did not resolve: {host}"));
    }
    Ok(resolved)
}

fn build_pinned_image_client(host: &str, addrs: &[SocketAddr]) -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .resolve_to_addrs(host, addrs)
        .connect_timeout(Duration::from_secs(30))
        .build()
        .map_err(|_| "HTTP_ERROR: failed to build image download client".to_string())
}

pub(super) fn safe_reqwest_error(operation: &str, error: &reqwest::Error) -> String {
    let category = if error.is_timeout() {
        "timeout"
    } else if error.is_connect() {
        "connection failed"
    } else if error.is_request() {
        "request failed"
    } else if error.is_body() || error.is_decode() {
        "response failed"
    } else {
        "transport failed"
    };
    format!("HTTP_ERROR: {operation}: {category}")
}

pub(super) fn resolve_image_redirect(
    current: &reqwest::Url,
    location: &str,
) -> Result<reqwest::Url, String> {
    if location.len() > 2_048 {
        return Err("HTTP_ERROR: image redirect location is too long".to_string());
    }
    let next = current
        .join(location)
        .map_err(|e| format!("HTTP_ERROR: invalid image redirect location: {e}"))?;
    validate_fetch_image_url(next.as_str())
}

pub(super) fn ensure_image_redirect_budget(followed: usize) -> Result<(), String> {
    if followed >= MAX_IMAGE_REDIRECTS {
        return Err(format!(
            "HTTP_ERROR: image download exceeded {MAX_IMAGE_REDIRECTS} redirects"
        ));
    }
    Ok(())
}

pub(super) fn is_image_content_type(content_type: &str) -> bool {
    content_type
        .trim()
        .to_ascii_lowercase()
        .starts_with("image/")
}

pub(crate) async fn fetch_image(
    url: &str,
    timeout_secs: Option<u32>,
) -> Result<ImageGenFetchedImage, String> {
    let mut current = validate_fetch_image_url(url)?;
    let mut redirects = 0usize;
    let mut response = loop {
        let (host, addrs) = resolve_public_addrs(&current).await?;
        let client = build_pinned_image_client(&host, &addrs)?;
        let response = client
            .get(current.clone())
            .timeout(resolve_timeout(timeout_secs))
            .send()
            .await
            .map_err(|e| safe_reqwest_error("download image", &e))?;

        if !response.status().is_redirection() {
            break response;
        }
        if current.cannot_be_a_base() {
            return Err("HTTP_ERROR: image redirect base URL is invalid".to_string());
        }
        let location = response
            .headers()
            .get(LOCATION)
            .ok_or_else(|| "HTTP_ERROR: image redirect is missing Location".to_string())?
            .to_str()
            .map_err(|_| "HTTP_ERROR: image redirect Location is invalid".to_string())?;
        let next = resolve_image_redirect(&current, location)?;
        if next == current {
            return Err("HTTP_ERROR: image redirect loop detected".to_string());
        }
        ensure_image_redirect_budget(redirects)?;
        redirects += 1;
        current = next;
    };

    let status = response.status().as_u16();
    if !(200..300).contains(&status) {
        let _ = read_body_with_limit(&mut response, MAX_ERROR_RESPONSE_BYTES).await;
        return Err(format!(
            "HTTP_ERROR: image download failed with status {status}"
        ));
    }

    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    if !is_image_content_type(&content_type) {
        return Err(format!(
            "HTTP_ERROR: image download returned a non-image content type: {}",
            truncate_excerpt(&content_type)
        ));
    }

    let body = read_body_capped(&mut response).await?;
    Ok(ImageGenFetchedImage {
        mime: content_type
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .to_string(),
        data_b64: base64::engine::general_purpose::STANDARD.encode(&body),
    })
}
