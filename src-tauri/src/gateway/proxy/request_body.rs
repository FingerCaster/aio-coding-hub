//! Usage: request body raw/decoded model for gateway passthrough.

use super::http_util::{
    gunzip_bytes_with_limit, gzip_bytes_with_limit, has_gzip_content_encoding,
    has_zstd_content_encoding, inflate_bytes_with_limit, unbrotli_bytes_with_limit,
    unzstd_bytes_with_limit, zstd_bytes_with_limit, BoundedDecodeError,
};
use axum::body::Bytes;
use axum::http::{header, HeaderMap, HeaderValue, Method};

const MAX_CONTENT_ENCODING_LAYERS: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CodexRequestNormalizationError {
    InvalidContentEncoding,
    BodyTooLarge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ContentEncoding {
    Gzip,
    Deflate,
    Brotli,
    Zstd,
}

pub(super) fn normalize_codex_request_body(
    cli_key: &str,
    method: &Method,
    forwarded_path: &str,
    headers: &mut HeaderMap,
    body: Bytes,
    max_decoded_bytes: usize,
) -> Result<Bytes, CodexRequestNormalizationError> {
    if !is_codex_json_request(cli_key, method, forwarded_path) {
        return Ok(body);
    }

    let Some(encodings) = parse_content_encodings(headers)? else {
        return Ok(body);
    };

    let mut decoded = body;
    for encoding in encodings.iter().rev() {
        decoded = decode_content_encoding(*encoding, decoded.as_ref(), max_decoded_bytes).map_err(
            |err| match err {
                BoundedDecodeError::InvalidData => {
                    CodexRequestNormalizationError::InvalidContentEncoding
                }
                BoundedDecodeError::OutputTooLarge => CodexRequestNormalizationError::BodyTooLarge,
            },
        )?;
    }

    headers.remove(header::CONTENT_ENCODING);
    headers.remove(header::CONTENT_LENGTH);
    headers.remove(header::TRANSFER_ENCODING);
    Ok(decoded)
}

fn is_codex_json_request(cli_key: &str, method: &Method, forwarded_path: &str) -> bool {
    if cli_key != "codex" || *method != Method::POST {
        return false;
    }

    let path = forwarded_path
        .split('?')
        .next()
        .unwrap_or(forwarded_path)
        .trim_end_matches('/');
    let segments = path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();

    segments.ends_with(&["responses"])
        || segments.ends_with(&["responses", "compact"])
        || segments.ends_with(&["chat", "completions"])
}

fn parse_content_encodings(
    headers: &HeaderMap,
) -> Result<Option<Vec<ContentEncoding>>, CodexRequestNormalizationError> {
    if !headers.contains_key(header::CONTENT_ENCODING) {
        return Ok(None);
    }

    let mut encodings = Vec::new();
    for value in headers.get_all(header::CONTENT_ENCODING).iter() {
        let value = value
            .to_str()
            .map_err(|_| CodexRequestNormalizationError::InvalidContentEncoding)?;
        for token in value
            .split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
        {
            let encoding = match token.to_ascii_lowercase().as_str() {
                "identity" => continue,
                "gzip" | "x-gzip" => ContentEncoding::Gzip,
                "deflate" => ContentEncoding::Deflate,
                "br" => ContentEncoding::Brotli,
                "zstd" | "zst" => ContentEncoding::Zstd,
                _ => return Err(CodexRequestNormalizationError::InvalidContentEncoding),
            };
            encodings.push(encoding);
            if encodings.len() > MAX_CONTENT_ENCODING_LAYERS {
                return Err(CodexRequestNormalizationError::InvalidContentEncoding);
            }
        }
    }
    Ok(Some(encodings))
}

fn decode_content_encoding(
    encoding: ContentEncoding,
    input: &[u8],
    max_decoded_bytes: usize,
) -> Result<Bytes, BoundedDecodeError> {
    match encoding {
        ContentEncoding::Gzip => gunzip_bytes_with_limit(input, max_decoded_bytes),
        ContentEncoding::Deflate => inflate_bytes_with_limit(input, max_decoded_bytes),
        ContentEncoding::Brotli => unbrotli_bytes_with_limit(input, max_decoded_bytes),
        ContentEncoding::Zstd => unzstd_bytes_with_limit(input, max_decoded_bytes),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RequestBodyEncoding {
    Identity,
    Gzip,
    Zstd,
    Unsupported,
}

#[derive(Debug, Clone)]
pub(super) struct GatewayRequestBody {
    raw: Bytes,
    decoded: Bytes,
    encoding: RequestBodyEncoding,
    original_content_encoding: Option<HeaderValue>,
    decoded_from_raw: bool,
    mutated: bool,
}

impl GatewayRequestBody {
    pub(super) fn from_wire(raw: Bytes, headers: &HeaderMap, max_decoded_bytes: usize) -> Self {
        let encoding = classify_request_encoding(headers);
        let original_content_encoding = headers.get(header::CONTENT_ENCODING).cloned();
        match encoding {
            RequestBodyEncoding::Gzip => {
                match gunzip_bytes_with_limit(raw.as_ref(), max_decoded_bytes) {
                    Ok(decoded) => Self {
                        raw,
                        decoded,
                        encoding,
                        original_content_encoding,
                        decoded_from_raw: true,
                        mutated: false,
                    },
                    Err(err) => {
                        tracing::warn!(error = %err, "failed to decode request gzip body for inspection; preserving raw body");
                        Self {
                            decoded: raw.clone(),
                            raw,
                            encoding,
                            original_content_encoding,
                            decoded_from_raw: false,
                            mutated: false,
                        }
                    }
                }
            }
            RequestBodyEncoding::Zstd => {
                match unzstd_bytes_with_limit(raw.as_ref(), max_decoded_bytes) {
                    Ok(decoded) => Self {
                        raw,
                        decoded,
                        encoding,
                        original_content_encoding,
                        decoded_from_raw: true,
                        mutated: false,
                    },
                    Err(err) => {
                        tracing::warn!(error = %err, "failed to decode request zstd body for inspection; preserving raw body");
                        Self {
                            decoded: raw.clone(),
                            raw,
                            encoding,
                            original_content_encoding,
                            decoded_from_raw: false,
                            mutated: false,
                        }
                    }
                }
            }
            RequestBodyEncoding::Identity | RequestBodyEncoding::Unsupported => Self {
                decoded: raw.clone(),
                raw,
                encoding,
                original_content_encoding,
                decoded_from_raw: false,
                mutated: false,
            },
        }
    }

    pub(super) fn decoded(&self) -> &Bytes {
        &self.decoded
    }

    pub(super) fn decoded_clone(&self) -> Bytes {
        self.decoded.clone()
    }

    pub(super) fn semantic_headers(&self, headers: &HeaderMap) -> HeaderMap {
        let mut semantic = headers.clone();
        semantic.remove(header::CONTENT_LENGTH);
        if self.decoded_from_raw {
            semantic.remove(header::CONTENT_ENCODING);
        }
        semantic
    }

    pub(super) fn replace_decoded(&mut self, next: Bytes) {
        if self.decoded != next {
            self.decoded = next;
            self.mutated = true;
        }
    }

    pub(super) fn is_mutated(&self) -> bool {
        self.mutated
    }

    pub(super) fn finalize_for_upstream(
        &self,
        headers: &mut HeaderMap,
        max_encoded_bytes: usize,
    ) -> Bytes {
        headers.remove(header::CONTENT_LENGTH);
        if !self.mutated {
            restore_original_content_encoding(headers, self.original_content_encoding.as_ref());
            return self.raw.clone();
        }

        match self.encoding {
            RequestBodyEncoding::Gzip if self.decoded_from_raw => {
                match gzip_bytes_with_limit(self.decoded.as_ref(), max_encoded_bytes) {
                    Ok(encoded) => {
                        restore_original_content_encoding(
                            headers,
                            self.original_content_encoding.as_ref(),
                        );
                        encoded
                    }
                    Err(err) => {
                        tracing::warn!(error = %err, "failed to re-encode request gzip body; sending identity body");
                        headers.remove(header::CONTENT_ENCODING);
                        self.decoded.clone()
                    }
                }
            }
            RequestBodyEncoding::Zstd if self.decoded_from_raw => {
                match zstd_bytes_with_limit(self.decoded.as_ref(), max_encoded_bytes) {
                    Ok(encoded) => {
                        restore_original_content_encoding(
                            headers,
                            self.original_content_encoding.as_ref(),
                        );
                        encoded
                    }
                    Err(err) => {
                        tracing::warn!(error = %err, "failed to re-encode request zstd body; sending identity body");
                        headers.remove(header::CONTENT_ENCODING);
                        self.decoded.clone()
                    }
                }
            }
            RequestBodyEncoding::Gzip
            | RequestBodyEncoding::Zstd
            | RequestBodyEncoding::Unsupported => {
                tracing::warn!(
                    encoding = ?self.encoding,
                    "request body mutated without a decoded content encoding; sending identity body"
                );
                headers.remove(header::CONTENT_ENCODING);
                self.decoded.clone()
            }
            RequestBodyEncoding::Identity => {
                headers.remove(header::CONTENT_ENCODING);
                self.decoded.clone()
            }
        }
    }
}

fn classify_request_encoding(headers: &HeaderMap) -> RequestBodyEncoding {
    let Some(value) = headers
        .get(header::CONTENT_ENCODING)
        .and_then(|value| value.to_str().ok())
    else {
        return RequestBodyEncoding::Identity;
    };
    let encodings = value
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .collect::<Vec<_>>();
    if encodings.is_empty()
        || encodings
            .iter()
            .all(|item| item.eq_ignore_ascii_case("identity"))
    {
        return RequestBodyEncoding::Identity;
    }
    if encodings.len() == 1 && has_gzip_content_encoding(headers) {
        return RequestBodyEncoding::Gzip;
    }
    if encodings.len() == 1 && has_zstd_content_encoding(headers) {
        return RequestBodyEncoding::Zstd;
    }
    RequestBodyEncoding::Unsupported
}

fn restore_original_content_encoding(headers: &mut HeaderMap, original: Option<&HeaderValue>) {
    match original {
        Some(value) => {
            headers.insert(header::CONTENT_ENCODING, value.clone());
        }
        None => {
            headers.remove(header::CONTENT_ENCODING);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::write::{DeflateEncoder, GzEncoder, ZlibEncoder};
    use flate2::Compression;
    use std::io::Write;

    const PLAIN_JSON: &[u8] = br#"{"model":"gpt-5.6-sol","input":"hello"}"#;

    fn gzip_bytes(input: &[u8]) -> Vec<u8> {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(input).expect("gzip write");
        encoder.finish().expect("gzip finish")
    }

    fn zlib_bytes(input: &[u8]) -> Vec<u8> {
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(input).expect("zlib write");
        encoder.finish().expect("zlib finish")
    }

    fn raw_deflate_bytes(input: &[u8]) -> Vec<u8> {
        let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(input).expect("deflate write");
        encoder.finish().expect("deflate finish")
    }

    fn brotli_bytes(input: &[u8]) -> Vec<u8> {
        let mut output = Vec::new();
        {
            let mut encoder = brotli::CompressorWriter::new(&mut output, 4096, 5, 22);
            encoder.write_all(input).expect("brotli write");
        }
        output
    }

    fn zstd_bytes(input: &[u8]) -> Vec<u8> {
        zstd::stream::encode_all(input, 3).expect("zstd encode")
    }

    fn unzstd_bytes(input: &[u8]) -> Vec<u8> {
        zstd::stream::decode_all(input).expect("zstd decode")
    }

    fn gunzip_bytes(input: &[u8]) -> Vec<u8> {
        let mut decoder = flate2::read::GzDecoder::new(input);
        let mut out = Vec::new();
        std::io::Read::read_to_end(&mut decoder, &mut out).expect("gzip read");
        out
    }

    fn gzip_headers(content_len: usize) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(header::CONTENT_ENCODING, HeaderValue::from_static("gzip"));
        headers.insert(
            header::CONTENT_LENGTH,
            HeaderValue::from_str(&content_len.to_string()).expect("len header"),
        );
        headers
    }

    fn zstd_headers(content_len: usize) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(header::CONTENT_ENCODING, HeaderValue::from_static("zstd"));
        headers.insert(
            header::CONTENT_LENGTH,
            HeaderValue::from_str(&content_len.to_string()).expect("len header"),
        );
        headers
    }

    fn encoded_headers(value: &'static str, content_len: usize) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(header::CONTENT_ENCODING, HeaderValue::from_static(value));
        headers.insert(
            header::CONTENT_LENGTH,
            HeaderValue::from_str(&content_len.to_string()).expect("len header"),
        );
        headers.insert(
            header::TRANSFER_ENCODING,
            HeaderValue::from_static("chunked"),
        );
        headers
    }

    fn normalize(
        body: Vec<u8>,
        headers: &mut HeaderMap,
    ) -> Result<Bytes, CodexRequestNormalizationError> {
        normalize_codex_request_body(
            "codex",
            &Method::POST,
            "/v1/responses",
            headers,
            Bytes::from(body),
            1024 * 1024,
        )
    }

    #[test]
    fn codex_json_path_matcher_accepts_only_post_endpoint_suffixes() {
        for path in [
            "/responses",
            "/v1/responses/",
            "/v1/responses?stream=true",
            "/nested/openai/v1/responses/compact",
            "/v1/chat/completions/",
        ] {
            assert!(is_codex_json_request("codex", &Method::POST, path));
        }

        for path in [
            "/responses/other",
            "/v1/responses/compact/other",
            "/v1/chat/completions/other",
            "/v1/embeddings",
            "/myresponses",
        ] {
            assert!(!is_codex_json_request("codex", &Method::POST, path));
        }
        assert!(!is_codex_json_request(
            "claude",
            &Method::POST,
            "/v1/responses"
        ));
        assert!(!is_codex_json_request(
            "codex",
            &Method::GET,
            "/v1/responses"
        ));
    }

    #[test]
    fn normalizes_every_supported_content_encoding_and_alias() {
        let cases = [
            ("GZip", gzip_bytes(PLAIN_JSON)),
            ("x-gzip", gzip_bytes(PLAIN_JSON)),
            ("deflate", zlib_bytes(PLAIN_JSON)),
            ("deflate", raw_deflate_bytes(PLAIN_JSON)),
            ("br", brotli_bytes(PLAIN_JSON)),
            ("zstd", zstd_bytes(PLAIN_JSON)),
            ("zst", zstd_bytes(PLAIN_JSON)),
        ];

        for (encoding, body) in cases {
            let mut headers = encoded_headers(encoding, body.len());
            let decoded = normalize(body, &mut headers).expect("supported encoding");

            assert_eq!(decoded.as_ref(), PLAIN_JSON, "encoding={encoding}");
            assert!(headers.get(header::CONTENT_ENCODING).is_none());
            assert!(headers.get(header::CONTENT_LENGTH).is_none());
            assert!(headers.get(header::TRANSFER_ENCODING).is_none());
        }
    }

    #[test]
    fn normalizes_repeated_stacked_encodings_in_reverse_order() {
        let gzip = gzip_bytes(PLAIN_JSON);
        let encoded = brotli_bytes(&gzip);
        let mut headers = HeaderMap::new();
        headers.append(
            header::CONTENT_ENCODING,
            HeaderValue::from_static("identity, gzip"),
        );
        headers.append(header::CONTENT_ENCODING, HeaderValue::from_static("br"));
        headers.insert(
            header::CONTENT_LENGTH,
            HeaderValue::from_str(&encoded.len().to_string()).expect("len header"),
        );

        let decoded = normalize(encoded, &mut headers).expect("stacked encoding");

        assert_eq!(decoded.as_ref(), PLAIN_JSON);
        assert!(headers.get(header::CONTENT_ENCODING).is_none());
        assert!(headers.get(header::CONTENT_LENGTH).is_none());
    }

    #[test]
    fn identity_only_is_plain_and_removes_stale_entity_headers() {
        let mut headers = encoded_headers("identity", PLAIN_JSON.len());

        let decoded =
            normalize(PLAIN_JSON.to_vec(), &mut headers).expect("identity should be accepted");

        assert_eq!(decoded.as_ref(), PLAIN_JSON);
        assert!(headers.get(header::CONTENT_ENCODING).is_none());
        assert!(headers.get(header::CONTENT_LENGTH).is_none());
        assert!(headers.get(header::TRANSFER_ENCODING).is_none());
    }

    #[test]
    fn non_target_request_keeps_compressed_bytes_and_headers() {
        let encoded = gzip_bytes(PLAIN_JSON);
        let mut headers = encoded_headers("gzip", encoded.len());

        let output = normalize_codex_request_body(
            "grok",
            &Method::POST,
            "/v1/chat/completions",
            &mut headers,
            Bytes::from(encoded.clone()),
            1024 * 1024,
        )
        .expect("non-Codex request");

        assert_eq!(output.as_ref(), encoded);
        assert_eq!(headers.get(header::CONTENT_ENCODING).unwrap(), "gzip");
        assert!(headers.get(header::CONTENT_LENGTH).is_some());
        assert!(headers.get(header::TRANSFER_ENCODING).is_some());
    }

    #[test]
    fn rejects_unknown_damaged_and_excessive_encoding_chains_without_header_mutation() {
        let cases = [
            ("snappy", PLAIN_JSON.to_vec()),
            ("gzip", b"not-a-gzip-stream".to_vec()),
            (
                "gzip, gzip, gzip, gzip, gzip, gzip, gzip, gzip, gzip",
                PLAIN_JSON.to_vec(),
            ),
        ];

        for (encoding, body) in cases {
            let mut headers = HeaderMap::new();
            headers.insert(
                header::CONTENT_ENCODING,
                HeaderValue::from_str(encoding).expect("encoding header"),
            );
            headers.insert(header::CONTENT_LENGTH, HeaderValue::from_static("42"));

            let err = normalize(body, &mut headers).expect_err("invalid encoding");

            assert_eq!(err, CodexRequestNormalizationError::InvalidContentEncoding);
            assert_eq!(
                headers.get(header::CONTENT_ENCODING).unwrap(),
                encoding,
                "encoding={encoding}"
            );
            assert!(headers.get(header::CONTENT_LENGTH).is_some());
        }
    }

    #[test]
    fn rejects_when_any_decoded_layer_exceeds_limit() {
        let inner = gzip_bytes(PLAIN_JSON);
        let encoded = zstd_bytes(&inner);
        let mut headers = encoded_headers("gzip, zstd", encoded.len());

        let err = normalize_codex_request_body(
            "codex",
            &Method::POST,
            "/v1/responses",
            &mut headers,
            Bytes::from(encoded),
            inner.len() - 1,
        )
        .expect_err("intermediate decoded layer should exceed limit");

        assert_eq!(err, CodexRequestNormalizationError::BodyTooLarge);
        assert!(headers.get(header::CONTENT_ENCODING).is_some());
        assert!(headers.get(header::CONTENT_LENGTH).is_some());
    }

    #[test]
    fn every_decoder_enforces_the_output_limit() {
        let plain = vec![b'a'; 128 * 1024];
        let cases = [
            ("gzip", gzip_bytes(&plain)),
            ("deflate", zlib_bytes(&plain)),
            ("deflate", raw_deflate_bytes(&plain)),
            ("br", brotli_bytes(&plain)),
            ("zstd", zstd_bytes(&plain)),
        ];

        for (encoding, body) in cases {
            let mut headers = encoded_headers(encoding, body.len());
            let err = normalize_codex_request_body(
                "codex",
                &Method::POST,
                "/v1/responses",
                &mut headers,
                Bytes::from(body),
                1024,
            )
            .expect_err("decoded body should exceed limit");

            assert_eq!(
                err,
                CodexRequestNormalizationError::BodyTooLarge,
                "encoding={encoding}"
            );
            assert!(headers.get(header::CONTENT_ENCODING).is_some());
        }
    }

    #[test]
    fn accepts_exactly_eight_effective_encoding_layers() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_ENCODING,
            HeaderValue::from_static("identity, gzip, gzip, gzip, gzip, gzip, gzip, gzip, gzip"),
        );

        let encodings = parse_content_encodings(&headers)
            .expect("eight layers should parse")
            .expect("encoding header");

        assert_eq!(encodings.len(), MAX_CONTENT_ENCODING_LAYERS);
    }

    #[test]
    fn unchanged_gzip_body_uses_semantic_headers_for_hooks_and_raw_bytes_for_upstream() {
        let plain = Bytes::from_static(br#"{"input":"hello 13344441520"}"#);
        let raw = Bytes::from(gzip_bytes(plain.as_ref()));
        let wire_headers = gzip_headers(raw.len());

        let body = GatewayRequestBody::from_wire(raw.clone(), &wire_headers, 1024 * 1024);
        let mut hook_headers = body.semantic_headers(&wire_headers);
        let upstream = body.finalize_for_upstream(&mut hook_headers, 1024 * 1024);

        assert_eq!(body.decoded(), &plain);
        assert_eq!(body.decoded_clone(), plain);
        assert!(!body.is_mutated());
        assert_eq!(upstream, raw);
        assert!(body
            .semantic_headers(&wire_headers)
            .get(header::CONTENT_ENCODING)
            .is_none());
        assert!(body
            .semantic_headers(&wire_headers)
            .get(header::CONTENT_LENGTH)
            .is_none());
        assert_eq!(hook_headers.get(header::CONTENT_ENCODING).unwrap(), "gzip");
        assert!(hook_headers.get(header::CONTENT_LENGTH).is_none());
    }

    #[test]
    fn mutated_gzip_body_is_reencoded_and_length_is_removed() {
        let plain = Bytes::from_static(br#"{"input":"hello 13344441520"}"#);
        let raw = Bytes::from(gzip_bytes(plain.as_ref()));
        let wire_headers = gzip_headers(raw.len());
        let mut body = GatewayRequestBody::from_wire(raw, &wire_headers, 1024 * 1024);
        let mut hook_headers = body.semantic_headers(&wire_headers);

        body.replace_decoded(Bytes::from(r#"{"input":"hello [电话]"}"#));
        let upstream = body.finalize_for_upstream(&mut hook_headers, 1024 * 1024);

        assert!(body.is_mutated());
        assert_eq!(hook_headers.get(header::CONTENT_ENCODING).unwrap(), "gzip");
        assert!(hook_headers.get(header::CONTENT_LENGTH).is_none());
        assert_eq!(
            gunzip_bytes(upstream.as_ref()),
            r#"{"input":"hello [电话]"}"#.as_bytes()
        );
    }

    #[test]
    fn invalid_gzip_body_stays_raw_when_unchanged() {
        let raw = Bytes::from_static(b"not-gzip");
        let wire_headers = gzip_headers(raw.len());

        let body = GatewayRequestBody::from_wire(raw.clone(), &wire_headers, 1024 * 1024);
        let mut hook_headers = body.semantic_headers(&wire_headers);
        let upstream = body.finalize_for_upstream(&mut hook_headers, 1024 * 1024);

        assert_eq!(body.decoded(), &raw);
        assert_eq!(upstream, raw);
        assert_eq!(hook_headers.get(header::CONTENT_ENCODING).unwrap(), "gzip");
        assert!(hook_headers.get(header::CONTENT_LENGTH).is_none());
    }

    #[test]
    fn mutated_invalid_gzip_body_falls_back_to_identity() {
        let raw = Bytes::from_static(b"not-gzip");
        let wire_headers = gzip_headers(raw.len());
        let mut body = GatewayRequestBody::from_wire(raw, &wire_headers, 1024 * 1024);
        let mut hook_headers = body.semantic_headers(&wire_headers);

        body.replace_decoded(Bytes::from_static(br#"{"input":"changed"}"#));
        let upstream = body.finalize_for_upstream(&mut hook_headers, 1024 * 1024);

        assert_eq!(upstream, Bytes::from_static(br#"{"input":"changed"}"#));
        assert!(hook_headers.get(header::CONTENT_ENCODING).is_none());
        assert!(hook_headers.get(header::CONTENT_LENGTH).is_none());
    }

    #[test]
    fn unchanged_zstd_body_uses_semantic_headers_for_hooks_and_raw_bytes_for_upstream() {
        let plain = Bytes::from_static(br#"{"input":"hello 13344441520"}"#);
        let raw = Bytes::from(zstd_bytes(plain.as_ref()));
        let wire_headers = zstd_headers(raw.len());

        let body = GatewayRequestBody::from_wire(raw.clone(), &wire_headers, 1024 * 1024);
        let mut hook_headers = body.semantic_headers(&wire_headers);
        let upstream = body.finalize_for_upstream(&mut hook_headers, 1024 * 1024);

        assert_eq!(body.decoded(), &plain);
        assert!(!body.is_mutated());
        assert_eq!(upstream, raw);
        assert!(body
            .semantic_headers(&wire_headers)
            .get(header::CONTENT_ENCODING)
            .is_none());
        assert_eq!(hook_headers.get(header::CONTENT_ENCODING).unwrap(), "zstd");
        assert!(hook_headers.get(header::CONTENT_LENGTH).is_none());
    }

    #[test]
    fn mutated_zstd_body_is_reencoded_and_length_is_removed() {
        let plain = Bytes::from_static(br#"{"input":"hello 13344441520"}"#);
        let raw = Bytes::from(zstd_bytes(plain.as_ref()));
        let wire_headers = zstd_headers(raw.len());
        let mut body = GatewayRequestBody::from_wire(raw, &wire_headers, 1024 * 1024);
        let mut hook_headers = body.semantic_headers(&wire_headers);

        body.replace_decoded(Bytes::from(r#"{"input":"hello [电话]"}"#));
        let upstream = body.finalize_for_upstream(&mut hook_headers, 1024 * 1024);

        assert!(body.is_mutated());
        assert_eq!(hook_headers.get(header::CONTENT_ENCODING).unwrap(), "zstd");
        assert!(hook_headers.get(header::CONTENT_LENGTH).is_none());
        assert_eq!(
            unzstd_bytes(upstream.as_ref()),
            r#"{"input":"hello [电话]"}"#.as_bytes()
        );
    }

    #[test]
    fn invalid_zstd_body_stays_raw_when_unchanged() {
        let raw = Bytes::from_static(b"not-zstd");
        let wire_headers = zstd_headers(raw.len());

        let body = GatewayRequestBody::from_wire(raw.clone(), &wire_headers, 1024 * 1024);
        let mut hook_headers = body.semantic_headers(&wire_headers);
        let upstream = body.finalize_for_upstream(&mut hook_headers, 1024 * 1024);

        assert_eq!(body.decoded(), &raw);
        assert_eq!(upstream, raw);
        assert_eq!(hook_headers.get(header::CONTENT_ENCODING).unwrap(), "zstd");
        assert!(hook_headers.get(header::CONTENT_LENGTH).is_none());
    }

    #[test]
    fn mutated_invalid_zstd_body_falls_back_to_identity() {
        let raw = Bytes::from_static(b"not-zstd");
        let wire_headers = zstd_headers(raw.len());
        let mut body = GatewayRequestBody::from_wire(raw, &wire_headers, 1024 * 1024);
        let mut hook_headers = body.semantic_headers(&wire_headers);

        body.replace_decoded(Bytes::from_static(br#"{"input":"changed"}"#));
        let upstream = body.finalize_for_upstream(&mut hook_headers, 1024 * 1024);

        assert_eq!(upstream, Bytes::from_static(br#"{"input":"changed"}"#));
        assert!(hook_headers.get(header::CONTENT_ENCODING).is_none());
        assert!(hook_headers.get(header::CONTENT_LENGTH).is_none());
    }

    #[test]
    fn mutated_unsupported_encoding_drops_encoding_header() {
        let raw = Bytes::from_static(br#"{"input":"hello"}"#);
        let mut wire_headers = HeaderMap::new();
        wire_headers.insert(header::CONTENT_ENCODING, HeaderValue::from_static("br"));
        wire_headers.insert(header::CONTENT_LENGTH, HeaderValue::from_static("17"));
        let mut body = GatewayRequestBody::from_wire(raw, &wire_headers, 1024 * 1024);
        let mut hook_headers = body.semantic_headers(&wire_headers);

        body.replace_decoded(Bytes::from_static(br#"{"input":"changed"}"#));
        let upstream = body.finalize_for_upstream(&mut hook_headers, 1024 * 1024);

        assert_eq!(upstream, Bytes::from_static(br#"{"input":"changed"}"#));
        assert!(hook_headers.get(header::CONTENT_ENCODING).is_none());
        assert!(hook_headers.get(header::CONTENT_LENGTH).is_none());
    }

    #[test]
    fn compound_zstd_encoding_stays_unsupported() {
        let raw = Bytes::from_static(br#"{"input":"hello"}"#);
        let mut wire_headers = HeaderMap::new();
        wire_headers.insert(
            header::CONTENT_ENCODING,
            HeaderValue::from_static("gzip, zstd"),
        );

        let body = GatewayRequestBody::from_wire(raw.clone(), &wire_headers, 1024 * 1024);

        assert_eq!(body.encoding, RequestBodyEncoding::Unsupported);
        assert_eq!(body.decoded(), &raw);
    }
}
