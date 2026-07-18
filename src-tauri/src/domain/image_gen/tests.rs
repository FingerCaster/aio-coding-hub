use super::config::{config_connection, config_get, config_set};
use super::transport::{
    build_request_url, decode_multipart_files, ensure_image_redirect_budget, is_disallowed_ip,
    is_image_content_type, post_json, post_multipart, resolve_image_redirect, resolve_timeout,
    safe_failure_summary, safe_reqwest_error, validate_fetch_image_url, validate_multipart_fields,
    validate_multipart_files, validate_public_addrs, validate_request_path, ImageGenMultipartFile,
};
use crate::db;
use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

fn test_db(name: &str) -> (tempfile::TempDir, db::Db) {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = db::init_for_tests(&dir.path().join(name)).expect("init db");
    (dir, db)
}

// -- config --

#[test]
fn config_get_returns_unconfigured_defaults_when_missing() {
    let (_dir, db) = test_db("image-gen-missing.db");

    let view = config_get(&db, "gpt-image").expect("config_get");

    assert_eq!(view.adapter_id, "gpt-image");
    assert_eq!(view.base_url, "");
    assert_eq!(view.model, "");
    assert!(!view.api_key_configured);
}

#[test]
fn config_set_replace_clear_preserve_semantics() {
    let (_dir, db) = test_db("image-gen-semantics.db");

    // replace: Some(value)
    let view = config_set(
        &db,
        "gpt-image",
        "https://api.example.com",
        "gpt-image-2",
        Some("sk-secret"),
    )
    .expect("set with key");
    assert!(view.api_key_configured);
    assert_eq!(view.base_url, "https://api.example.com");
    assert_eq!(view.model, "gpt-image-2");

    // preserve: None keeps the stored key while updating other fields
    let view = config_set(
        &db,
        "gpt-image",
        "https://api2.example.com",
        "gpt-image-2-2026-04-21",
        None,
    )
    .expect("set preserve");
    assert!(view.api_key_configured);
    assert_eq!(view.base_url, "https://api2.example.com");
    assert_eq!(view.model, "gpt-image-2-2026-04-21");
    let (base_url, api_key) = config_connection(&db, "gpt-image").expect("connection");
    assert_eq!(base_url, "https://api2.example.com");
    assert_eq!(api_key, "sk-secret");

    // clear: Some("")
    let view = config_set(
        &db,
        "gpt-image",
        "https://api3.example.com",
        "gpt-image-2",
        Some(""),
    )
    .expect("set clear");
    assert!(!view.api_key_configured);
    let (_base_url, api_key) = config_connection(&db, "gpt-image").expect("connection");
    assert_eq!(api_key, "");
    // clear 只清 key：base_url/model 同请求值一并落库。
    let persisted = config_get(&db, "gpt-image").expect("config_get after clear");
    assert_eq!(persisted.base_url, "https://api3.example.com");
    assert_eq!(persisted.model, "gpt-image-2");
    assert!(!persisted.api_key_configured);
}

#[test]
fn config_view_never_contains_api_key_plaintext() {
    let (_dir, db) = test_db("image-gen-no-leak.db");

    config_set(
        &db,
        "gpt-image",
        "https://api.example.com",
        "gpt-image-2",
        Some("sk-super-secret"),
    )
    .expect("set with key");

    let view = config_get(&db, "gpt-image").expect("config_get");
    let serialized = serde_json::to_string(&view).expect("serialize view");
    assert!(!serialized.contains("sk-super-secret"));
    assert!(serialized.contains("\"apiKeyConfigured\":true"));
}

#[test]
fn config_rejects_empty_adapter_id() {
    let (_dir, db) = test_db("image-gen-bad-adapter.db");

    let err = config_get(&db, "   ").expect_err("empty adapter_id should fail");
    assert!(err.to_string().contains("SEC_INVALID_INPUT"));
}

#[test]
fn config_connection_fails_when_config_missing() {
    let (_dir, db) = test_db("image-gen-conn-missing.db");

    let err = config_connection(&db, "gpt-image").expect_err("missing config should fail");
    assert!(err.to_string().contains("SEC_INVALID_INPUT"));
}

// -- path allowlist --

#[test]
fn request_path_allowlist_accepts_only_image_endpoints() {
    assert!(validate_request_path("/v1/images/generations").is_ok());
    assert!(validate_request_path("/v1/images/edits").is_ok());

    for path in [
        "/v1/chat/completions",
        "/v1/images/generations/../chat",
        "v1/images/generations",
        "/v1/images/edits/",
        "/v1/images/generations?api_key=SYNTHETIC_SECRET",
        "/v1/images/edits#SYNTHETIC_SECRET",
        "",
    ] {
        let err = validate_request_path(path).expect_err("path should be rejected");
        assert!(err.contains("SEC_INVALID_INPUT"), "unexpected error: {err}");
        assert!(!err.contains("SYNTHETIC_SECRET"), "secret leaked: {err}");
        if !path.is_empty() {
            assert!(!err.contains(path), "rejected path leaked: {err}");
        }
    }
}

#[test]
fn generation_failure_summary_is_fixed_bounded_and_secret_free() {
    let summary = safe_failure_summary(422);
    assert_eq!(
        summary,
        "HTTP 422: upstream image generation request failed"
    );
    assert!(summary.chars().count() <= 512);
    assert!(!summary.contains("SYNTHETIC_SECRET"));
}

fn spawn_image_gen_failure_server(body: Vec<u8>) -> (String, std::thread::JoinHandle<()>) {
    use std::io::{Read as _, Write as _};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind failure server");
    let address = listener.local_addr().expect("failure server address");
    let worker = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().expect("accept failure request");
        socket
            .set_read_timeout(Some(std::time::Duration::from_secs(2)))
            .expect("read timeout");
        let mut request = Vec::new();
        let mut chunk = [0_u8; 4096];
        let (header_end, content_length) = loop {
            let read = socket.read(&mut chunk).expect("read request headers");
            if read == 0 {
                panic!("request closed before headers completed");
            }
            request.extend_from_slice(&chunk[..read]);
            if let Some(offset) = request.windows(4).position(|window| window == b"\r\n\r\n") {
                let header_end = offset + 4;
                let headers = String::from_utf8_lossy(&request[..header_end]);
                let content_length = headers
                    .lines()
                    .find_map(|line| {
                        line.split_once(':').and_then(|(name, value)| {
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().ok())
                                .flatten()
                        })
                    })
                    .unwrap_or(0);
                break (header_end, content_length);
            }
        };
        while request.len() < header_end + content_length {
            let read = socket.read(&mut chunk).expect("read request body");
            if read == 0 {
                break;
            }
            request.extend_from_slice(&chunk[..read]);
        }
        write!(
            socket,
            "HTTP/1.1 422 Unprocessable Entity\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .expect("response headers");
        let _ = socket.write_all(&body);
    });
    (format!("http://{address}"), worker)
}

#[tokio::test]
async fn json_and_multipart_failures_use_bounded_secret_free_transport_summary() {
    let secret_body = format!(
        "{{\"error\":{{\"message\":\"SYNTHETIC_SECRET{}\"}}}}",
        "x".repeat(9 * 1024)
    )
    .into_bytes();
    let client = reqwest::Client::builder().build().expect("client");

    let (base_url, worker) = spawn_image_gen_failure_server(secret_body.clone());
    let json = post_json(
        &client,
        &base_url,
        "synthetic-key",
        "/v1/images/generations",
        &serde_json::json!({"prompt": "test"}),
        Some(5),
    )
    .await
    .expect("bounded JSON failure response");
    worker.join().expect("JSON server");
    assert_eq!(json.status, 422);
    assert_eq!(json.body_text, safe_failure_summary(422));
    assert!(!json.body_text.contains("SYNTHETIC_SECRET"));

    let (base_url, worker) = spawn_image_gen_failure_server(secret_body);
    let multipart = post_multipart(
        &client,
        &base_url,
        "synthetic-key",
        "/v1/images/edits",
        &[("prompt".to_string(), "test".to_string())],
        &[],
        Some(5),
    )
    .await
    .expect("bounded multipart failure response");
    worker.join().expect("multipart server");
    assert_eq!(multipart.status, 422);
    assert_eq!(multipart.body_text, safe_failure_summary(422));
    assert!(!multipart.body_text.contains("SYNTHETIC_SECRET"));
}

// -- base url validation & join --

#[test]
fn build_request_url_joins_and_validates_scheme() {
    let url =
        build_request_url("https://api.example.com", "/v1/images/generations").expect("https base");
    assert_eq!(
        url.as_str(),
        "https://api.example.com/v1/images/generations"
    );

    // trailing slash is trimmed
    let url =
        build_request_url("https://api.example.com/", "/v1/images/edits").expect("trailing slash");
    assert_eq!(url.as_str(), "https://api.example.com/v1/images/edits");

    // custom path relays keep their prefix
    let url = build_request_url("https://relay.example.com/openai", "/v1/images/generations")
        .expect("custom path");
    assert_eq!(
        url.as_str(),
        "https://relay.example.com/openai/v1/images/generations"
    );

    // http allowed only for loopback debugging hosts
    assert!(build_request_url("http://127.0.0.1:37123", "/v1/images/edits").is_ok());
    assert!(build_request_url("http://localhost:8080", "/v1/images/edits").is_ok());
    let err = build_request_url("http://evil.example.com", "/v1/images/edits")
        .expect_err("plain http should fail");
    assert!(err.contains("SEC_INVALID_INPUT"));

    let err = build_request_url("ftp://api.example.com", "/v1/images/edits")
        .expect_err("ftp should fail");
    assert!(err.contains("SEC_INVALID_INPUT"));

    let err = build_request_url("   ", "/v1/images/edits").expect_err("empty base_url should fail");
    assert!(err.contains("SEC_INVALID_INPUT"));
}

#[test]
fn build_request_url_deduplicates_v1_suffix() {
    let url =
        build_request_url("https://api.example.com/v1", "/v1/images/generations").expect("v1 base");
    assert_eq!(
        url.as_str(),
        "https://api.example.com/v1/images/generations"
    );

    let url =
        build_request_url("https://api.example.com/v1/", "/v1/images/edits").expect("v1 slash");
    assert_eq!(url.as_str(), "https://api.example.com/v1/images/edits");
}

// -- fetch_image validation --

#[test]
fn fetch_image_url_rejects_http_and_private_hosts() {
    assert!(validate_fetch_image_url("https://cdn.example.com/img.png").is_ok());
    for url in [
        "http://cdn.example.com/img.png",
        "https://93.184.216.34/img.png",
        "https://127.0.0.1/img.png",
        "https://10.0.0.8/img.png",
        "https://192.168.1.2/img.png",
        "https://169.254.0.1/img.png",
        "https://[::1]/img.png",
        "https://user:secret@cdn.example.com/img.png",
        "https://cdn.example.com:8443/img.png",
        "not a url",
    ] {
        let err = validate_fetch_image_url(url).expect_err("url should be rejected");
        assert!(err.contains("SEC_INVALID_INPUT"), "unexpected error: {err}");
    }
}

#[test]
fn disallowed_ip_covers_all_non_global_ranges() {
    for ip in [
        "127.0.0.1",
        "10.1.2.3",
        "172.16.0.1",
        "192.168.0.1",
        "169.254.10.10",
        "100.64.0.1",
        "100.127.255.254",
        "198.18.0.1",
        "198.19.255.254",
        "192.0.2.1",
        "198.51.100.1",
        "203.0.113.1",
        "240.0.0.1",
        "0.0.0.0",
        "255.255.255.255",
        "::1",
        "::2",
        "::8.8.8.8",
        "::192.168.0.1",
        "fc00::1",
        "fe80::1",
        "::ffff:192.168.0.1",
        "::ffff:100.64.0.1",
        "::ffff:198.18.0.1",
        "64:ff9b::127.0.0.1",
        "64:ff9b::192.168.0.1",
        "2002:7f00:1::",
        "2002:c0a8:1::",
        "2001:db8::1",
        "2001:2::1",
    ] {
        let ip: IpAddr = ip.parse().expect("parse ip");
        assert!(is_disallowed_ip(ip), "should be disallowed: {ip}");
    }

    for ip in [
        "93.184.216.34",
        "8.8.8.8",
        "192.0.0.9",
        "2606:2800:220:1:248:1893:25c8:1946",
        "::ffff:8.8.8.8",
        "64:ff9b::8.8.8.8",
        "2002:0808:0808::",
    ] {
        let ip: IpAddr = ip.parse().expect("parse ip");
        assert!(!is_disallowed_ip(ip), "should be allowed: {ip}");
    }
}

#[test]
fn public_dns_validation_rejects_any_ipv4_compatible_ipv6_answer() {
    let public: SocketAddr = "[2606:2800:220:1:248:1893:25c8:1946]:443"
        .parse()
        .expect("public IPv6");
    let compatible: SocketAddr = "[::8.8.8.8]:443".parse().expect("compatible IPv6");
    let error = validate_public_addrs("cdn.example.test", [public, compatible])
        .expect_err("one non-global DNS answer must reject the host");
    assert!(error.contains("non-global address"));
}

#[test]
fn public_dns_validation_classifies_embedded_ipv4_in_ipv6_answers() {
    let pure_aaaa: SocketAddr = "[2606:2800:220:1:248:1893:25c8:1946]:443"
        .parse()
        .expect("public IPv6");
    let public_nat64: SocketAddr = "[64:ff9b::8.8.8.8]:443".parse().expect("public NAT64");
    let private_nat64: SocketAddr = "[64:ff9b::10.0.0.1]:443".parse().expect("private NAT64");
    let loopback_6to4: SocketAddr = "[2002:7f00:1::]:443".parse().expect("loopback 6to4");
    let public_a: SocketAddr = "93.184.216.34:443".parse().expect("public IPv4");

    assert_eq!(
        validate_public_addrs("cdn.example.test", [pure_aaaa, public_nat64])
            .expect("pure and translated public AAAA answers"),
        vec![pure_aaaa, public_nat64]
    );
    assert!(validate_public_addrs("cdn.example.test", [public_a, private_nat64]).is_err());
    assert!(validate_public_addrs("cdn.example.test", [pure_aaaa, loopback_6to4]).is_err());
}

#[tokio::test]
async fn fetch_image_rejects_localhost_hostname() {
    let err = super::fetch_image("https://localhost/img.png", Some(5))
        .await
        .expect_err("localhost should be rejected before any request");
    assert!(
        err.contains("non-global address"),
        "unexpected error: {err}"
    );
}

#[test]
fn image_redirect_validation_rechecks_every_target() {
    let current = reqwest::Url::parse("https://cdn.example.com/path/image.png").expect("url");
    assert_eq!(
        resolve_image_redirect(&current, "../next.png")
            .expect("relative redirect")
            .as_str(),
        "https://cdn.example.com/next.png"
    );
    for location in [
        "http://cdn.example.com/image.png",
        "https://127.0.0.1/image.png",
        "https://user:secret@cdn.example.com/image.png",
        "https://cdn.example.com:8443/image.png",
    ] {
        assert!(
            resolve_image_redirect(&current, location).is_err(),
            "redirect target should fail: {location}"
        );
    }
}

#[test]
fn image_dns_results_reject_mixed_private_answers_and_redirect_limit() {
    let public: SocketAddr = "93.184.216.34:443".parse().expect("public addr");
    let private: SocketAddr = "127.0.0.1:443".parse().expect("private addr");
    assert_eq!(
        validate_public_addrs("cdn.example.com", [public])
            .expect("public answer")
            .as_slice(),
        &[public]
    );
    assert!(validate_public_addrs("cdn.example.com", [public, private]).is_err());
    assert!(validate_public_addrs("cdn.example.com", []).is_err());
    assert!(ensure_image_redirect_budget(4).is_ok());
    assert!(ensure_image_redirect_budget(5).is_err());
}

// -- content type --

#[test]
fn image_content_type_check() {
    assert!(is_image_content_type("image/png"));
    assert!(is_image_content_type(" Image/JPEG; charset=binary"));
    assert!(!is_image_content_type("application/json"));
    assert!(!is_image_content_type("text/html"));
    assert!(!is_image_content_type(""));
}

// -- multipart --

#[test]
fn multipart_files_decode_preserves_field_filename_mime() {
    let files = vec![
        ImageGenMultipartFile {
            field: "image[]".to_string(),
            filename: "input-1.png".to_string(),
            mime: "image/png".to_string(),
            data_b64: "aGVsbG8=".to_string(), // "hello"
        },
        ImageGenMultipartFile {
            field: "image[]".to_string(),
            filename: "input-2.jpeg".to_string(),
            mime: "image/jpeg".to_string(),
            data_b64: "d29ybGQ=".to_string(), // "world"
        },
    ];

    let decoded = decode_multipart_files(&files).expect("decode files");
    assert_eq!(decoded.len(), 2);
    assert_eq!(decoded[0].field, "image[]");
    assert_eq!(decoded[0].filename, "input-1.png");
    assert_eq!(decoded[0].mime, "image/png");
    assert_eq!(decoded[0].bytes, b"hello");
    assert_eq!(decoded[1].filename, "input-2.jpeg");
    assert_eq!(decoded[1].bytes, b"world");
}

#[test]
fn multipart_files_reject_invalid_base64_and_empty_metadata() {
    let bad_b64 = vec![ImageGenMultipartFile {
        field: "image[]".to_string(),
        filename: "input-1.png".to_string(),
        mime: "image/png".to_string(),
        data_b64: "!!not-base64!!".to_string(),
    }];
    let err = decode_multipart_files(&bad_b64).expect_err("invalid base64 should fail");
    assert!(err.contains("SEC_INVALID_INPUT"));

    let empty_field = vec![ImageGenMultipartFile {
        field: "  ".to_string(),
        filename: "input-1.png".to_string(),
        mime: "image/png".to_string(),
        data_b64: "aGVsbG8=".to_string(),
    }];
    let err = decode_multipart_files(&empty_field).expect_err("empty field should fail");
    assert!(err.contains("field is required"));

    let empty_filename = vec![ImageGenMultipartFile {
        field: "image[]".to_string(),
        filename: "".to_string(),
        mime: "image/png".to_string(),
        data_b64: "aGVsbG8=".to_string(),
    }];
    let err = decode_multipart_files(&empty_filename).expect_err("empty filename should fail");
    assert!(err.contains("filename is required"));
}

#[test]
fn multipart_preflight_rejects_all_limits_before_decode() {
    let small = ImageGenMultipartFile {
        field: "image[]".to_string(),
        filename: "input.png".to_string(),
        mime: "image/png".to_string(),
        data_b64: "aGVsbG8=".to_string(),
    };
    let too_many = vec![small.clone(); 33];
    assert!(validate_multipart_files(&too_many)
        .expect_err("too many files")
        .contains("too many multipart files"));

    for (label, file) in [
        (
            "field",
            ImageGenMultipartFile {
                field: "x".repeat(129),
                ..small.clone()
            },
        ),
        (
            "filename",
            ImageGenMultipartFile {
                filename: "x".repeat(256),
                ..small.clone()
            },
        ),
        (
            "mime",
            ImageGenMultipartFile {
                mime: "x".repeat(129),
                ..small.clone()
            },
        ),
    ] {
        assert!(
            validate_multipart_files(&[file]).is_err(),
            "accepted overlong {label}"
        );
    }

    let oversized = ImageGenMultipartFile {
        data_b64: "A".repeat((64_usize * 1024 * 1024).div_ceil(3) * 4 + 4),
        ..small.clone()
    };
    let error = decode_multipart_files(&[
        ImageGenMultipartFile {
            data_b64: "!!!!".to_string(),
            ..small
        },
        oversized,
    ])
    .expect_err("preflight must reject aggregate before decoding earlier invalid data");
    assert!(error.contains("exceed"), "unexpected error: {error}");

    assert!(validate_multipart_fields(&[("x".repeat(129), "value".to_string())]).is_err());
    assert!(
        validate_multipart_fields(&[("name".to_string(), "x".repeat(1024 * 1024 + 1))]).is_err()
    );
}

#[test]
fn multipart_invalid_mime_is_rejected_before_large_base64_decode() {
    let file = ImageGenMultipartFile {
        field: "image[]".to_string(),
        filename: "input.png".to_string(),
        mime: "not a mime".to_string(),
        data_b64: "!!!!".repeat(2 * 1024 * 1024),
    };

    let error = decode_multipart_files(&[file]).expect_err("invalid MIME must fail preflight");
    assert!(
        error.contains("invalid mime type"),
        "unexpected error: {error}"
    );
    assert!(!error.contains("data_b64"));
}

#[tokio::test]
async fn reqwest_transport_error_does_not_echo_credentialed_url() {
    let secret = "SYNTHETIC_SECRET";
    let error = reqwest::Client::builder()
        .proxy(reqwest::Proxy::all("http://127.0.0.1:9").expect("proxy"))
        .build()
        .expect("client")
        .get(format!(
            "https://example.test/image?token={secret}#fragment"
        ))
        .send()
        .await
        .expect_err("closed local proxy must fail");
    let safe = safe_reqwest_error("download image", &error);
    assert!(!safe.contains(secret));
    assert!(!safe.contains("token="));
    assert!(safe.contains("HTTP_ERROR: download image"));
}

// -- timeout --

#[test]
fn timeout_defaults_to_600_and_clamps_to_1_900() {
    assert_eq!(resolve_timeout(None), Duration::from_secs(600));
    assert_eq!(resolve_timeout(Some(0)), Duration::from_secs(1));
    assert_eq!(resolve_timeout(Some(30)), Duration::from_secs(30));
    assert_eq!(resolve_timeout(Some(10_000)), Duration::from_secs(900));
}

// -- history --

use super::history::{
    ensure_writable_dir, read_image, read_image_with_roots, read_images_with_budget_with_roots,
    set_before_history_file_read_test_hook, set_before_history_read_open_test_hook,
    set_before_persist_file_create_test_hook, set_before_quarantine_test_hook,
    set_before_stats_open_test_hook, set_persist_failure_point,
    set_storage_stats_byte_bias_for_test, set_storage_stats_entry_count_seed_for_test,
    storage_cleanup, storage_cleanup_with_roots, storage_stats, storage_stats_with_roots,
    task_delete, task_persist, tasks_clear, tasks_list, tasks_list_with_roots,
    tasks_page_with_roots, ImageGenTaskFilePayload, ImageGenTaskPersistPayload,
    PersistFailurePoint,
};
#[cfg(windows)]
use super::history::{
    set_after_quarantine_validation_test_hook, set_after_windows_directory_enumeration_test_hook,
};
use base64::Engine as _;

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn file_payload(bytes: &[u8], mime: &str) -> ImageGenTaskFilePayload {
    ImageGenTaskFilePayload {
        mime: mime.to_string(),
        data_b64: b64(bytes),
    }
}

fn done_task_payload(id: &str, created_at: i64) -> ImageGenTaskPersistPayload {
    ImageGenTaskPersistPayload {
        id: id.to_string(),
        adapter_id: None,
        prompt: "a red square".to_string(),
        request_json: r#"{"size":"1024x1024"}"#.to_string(),
        status: "done".to_string(),
        error: None,
        usage_json: Some(r#"{"total_tokens":10}"#.to_string()),
        created_at,
        elapsed_ms: Some(1234),
        images: vec![file_payload(b"png-bytes", "image/png")],
        thumbs: vec![file_payload(b"thumb-bytes", "image/webp")],
        ref_images: vec![file_payload(b"ref-bytes", "image/png")],
    }
}

#[test]
fn history_persist_list_read_delete_full_chain() {
    let (_db_dir, db) = test_db("image-gen-history-chain.db");
    let storage = tempfile::tempdir().expect("storage tempdir");

    let row =
        task_persist(&db, storage.path(), done_task_payload("task-1", 100)).expect("persist task");

    let task_dir = std::fs::canonicalize(storage.path())
        .expect("canonical storage")
        .join("task-1");
    assert_eq!(row.id, "task-1");
    assert_eq!(row.adapter_id, "gpt-image");
    assert_eq!(row.status, "done");
    assert_eq!(row.created_at, 100);
    assert_eq!(row.elapsed_ms, Some(1234));
    assert_eq!(row.dir, "task-1");
    assert_eq!(
        std::fs::read(task_dir.join("image-1.png")).expect("image file"),
        b"png-bytes"
    );
    assert_eq!(
        std::fs::read(task_dir.join("thumb-1.webp")).expect("thumb file"),
        b"thumb-bytes"
    );
    assert_eq!(
        std::fs::read(task_dir.join("ref-1.png")).expect("ref file"),
        b"ref-bytes"
    );

    let listed = tasks_list(&db, storage.path(), None, 50).expect("list tasks");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].images.len(), 1);
    assert_eq!(listed[0].images[0].path, "task-1/image-1.png");
    assert_eq!(
        listed[0].images[0].thumb_path.as_deref(),
        Some("task-1/thumb-1.webp")
    );
    assert_eq!(listed[0].images[0].mime, "image/png");
    assert_eq!(listed[0].ref_images.len(), 1);
    assert_eq!(listed[0].ref_images[0].thumb_path, None);

    let fetched =
        read_image(&db, storage.path(), &listed[0].images[0].path).expect("read image back");
    assert_eq!(fetched.mime, "image/png");
    assert_eq!(fetched.data_b64, b64(b"png-bytes"));

    let stats = storage_stats(&db, storage.path()).expect("stats");
    assert_eq!(stats.task_count, 1);
    assert_eq!(
        stats.total_bytes,
        (b"png-bytes".len() + b"thumb-bytes".len() + b"ref-bytes".len()) as i64
    );

    task_delete(&db, storage.path(), "task-1").expect("delete task");
    assert!(!task_dir.exists());
    assert!(tasks_list(&db, storage.path(), None, 50)
        .expect("list after delete")
        .is_empty());

    // Idempotent: deleting again succeeds.
    task_delete(&db, storage.path(), "task-1").expect("delete task twice");
}

#[test]
fn history_persists_failed_task_and_paginates_newest_first() {
    let (_db_dir, db) = test_db("image-gen-history-pagination.db");
    let storage = tempfile::tempdir().expect("storage tempdir");

    for (id, created_at) in [("t1", 1_i64), ("t2", 2), ("t3", 3)] {
        let mut payload = done_task_payload(id, created_at);
        if id == "t2" {
            payload.status = "error".to_string();
            payload.error = Some("HTTP_ERROR: upstream 500".to_string());
            payload.usage_json = None;
            payload.images = Vec::new();
            payload.thumbs = Vec::new();
        }
        task_persist(&db, storage.path(), payload).expect("persist");
    }

    let first_page = tasks_list(&db, storage.path(), None, 2).expect("first page");
    assert_eq!(
        first_page.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(),
        vec!["t3", "t2"]
    );
    assert_eq!(first_page[1].status, "error");
    assert_eq!(
        first_page[1].error.as_deref(),
        Some("HTTP_ERROR: upstream 500")
    );
    assert!(first_page[1].images.is_empty());
    // Request snapshot survives for relay debugging.
    assert_eq!(first_page[1].request_json, r#"{"size":"1024x1024"}"#);

    let second_page =
        tasks_list(&db, storage.path(), Some(first_page[1].created_at), 2).expect("second page");
    assert_eq!(
        second_page
            .iter()
            .map(|t| t.id.as_str())
            .collect::<Vec<_>>(),
        vec!["t1"]
    );

    // limit 0 is clamped to 1.
    assert_eq!(
        tasks_list(&db, storage.path(), None, 0)
            .expect("clamped list")
            .len(),
        1
    );
}

#[test]
fn history_opaque_cursor_paginates_same_timestamp_without_gaps_or_duplicates() {
    let (_db_dir, db) = test_db("image-gen-history-opaque-pagination.db");
    let storage = tempfile::tempdir().expect("storage tempdir");
    for index in 0..55 {
        task_persist(
            &db,
            storage.path(),
            done_task_payload(&format!("t{index:03}"), 1_700_000_000_000),
        )
        .expect("persist same-timestamp task");
    }
    let roots = vec![storage.path().to_path_buf()];
    let mut cursor = None;
    let mut ids = Vec::new();
    let mut page_sizes = Vec::new();
    loop {
        let page = tasks_page_with_roots(&db, &roots, cursor.as_deref(), 20).expect("page");
        page_sizes.push(page.items.len());
        ids.extend(page.items.into_iter().map(|task| task.id));
        let Some(next) = page.next_cursor else {
            break;
        };
        cursor = Some(next);
    }

    assert_eq!(page_sizes, vec![20, 20, 15]);
    let expected = (0..55)
        .rev()
        .map(|index| format!("t{index:03}"))
        .collect::<Vec<_>>();
    assert_eq!(ids, expected);
    let unique = ids.iter().collect::<std::collections::HashSet<_>>();
    assert_eq!(unique.len(), ids.len());
}

#[test]
fn history_opaque_cursor_rejects_invalid_and_legacy_values() {
    let (_db_dir, db) = test_db("image-gen-history-invalid-cursor.db");
    let storage = tempfile::tempdir().expect("storage tempdir");
    task_persist(&db, storage.path(), done_task_payload("t1", 1)).expect("persist");
    let roots = vec![storage.path().to_path_buf()];
    let unknown_version = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(br#"{"v":2,"created_at":1,"id":"t1"}"#);
    let invalid_id = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(br#"{"v":1,"created_at":1,"id":"../escape"}"#);

    for cursor in ["1", "!!!", unknown_version.as_str(), invalid_id.as_str()] {
        let error = tasks_page_with_roots(&db, &roots, Some(cursor), 20)
            .expect_err("invalid cursor must fail closed");
        assert!(error.to_string().contains("cursor") || error.to_string().contains("task id"));
    }
}

#[test]
fn history_persist_rejects_existing_task_id_without_overwriting_files() {
    let (_db_dir, db) = test_db("image-gen-history-upsert.db");
    let storage = tempfile::tempdir().expect("storage tempdir");

    task_persist(&db, storage.path(), done_task_payload("t1", 1)).expect("persist");
    let original = std::fs::read(storage.path().join("t1/image-1.png")).expect("original image");
    let mut updated = done_task_payload("t1", 2);
    updated.prompt = "a blue square".to_string();
    let err = task_persist(&db, storage.path(), updated).expect_err("duplicate id must fail");
    assert!(err.to_string().contains("already exists"));

    let listed = tasks_list(&db, storage.path(), None, 50).expect("list");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].prompt, "a red square");
    assert_eq!(listed[0].created_at, 1);
    assert_eq!(
        std::fs::read(storage.path().join("t1/image-1.png")).expect("unchanged image"),
        original
    );
}

#[test]
fn history_persist_rejects_preexisting_task_directory_and_hardlink_target() {
    let (_db_dir, db) = test_db("image-gen-history-hardlink.db");
    let outer = tempfile::tempdir().expect("outer tempdir");
    let storage = outer.path().join("storage");
    let task_dir = storage.join("t1");
    std::fs::create_dir_all(&task_dir).expect("precreate task dir");
    let outside = outer.path().join("outside.png");
    std::fs::write(&outside, b"outside-original").expect("write outside file");
    std::fs::hard_link(&outside, task_dir.join("image-1.png")).expect("create hardlink");

    let err = task_persist(&db, &storage, done_task_payload("t1", 1))
        .expect_err("preexisting task dir must fail");
    assert!(err.to_string().contains("already exists"));
    assert_eq!(
        std::fs::read(&outside).expect("read outside file"),
        b"outside-original"
    );
    let conn = db.open_connection().expect("open db");
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM image_gen_tasks", [], |row| row.get(0))
        .expect("row count");
    assert_eq!(count, 0);
}

#[test]
fn history_persist_rejects_preexisting_symlink_without_touching_target_when_supported() {
    let (_db_dir, db) = test_db("image-gen-history-symlink.db");
    let outer = tempfile::tempdir().expect("outer tempdir");
    let storage = outer.path().join("storage");
    std::fs::create_dir_all(&storage).expect("create storage");
    let outside_dir = outer.path().join("outside-task");
    std::fs::create_dir_all(&outside_dir).expect("create outside dir");
    let outside = outside_dir.join("image-1.png");
    std::fs::write(&outside, b"outside-original").expect("write outside file");
    let link = storage.join("t1");

    #[cfg(unix)]
    let linked = std::os::unix::fs::symlink(&outside_dir, &link).is_ok();
    #[cfg(windows)]
    let linked = std::os::windows::fs::symlink_dir(&outside_dir, &link).is_ok();

    if !linked {
        return;
    }
    let err = task_persist(&db, &storage, done_task_payload("t1", 1))
        .expect_err("symlink task dir must fail");
    assert!(err.to_string().contains("already exists"));
    assert_eq!(
        std::fs::read(&outside).expect("read outside file"),
        b"outside-original"
    );
    let conn = db.open_connection().expect("open db");
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM image_gen_tasks", [], |row| row.get(0))
        .expect("row count");
    assert_eq!(count, 0);
}

#[test]
fn history_persist_rejects_invalid_input() {
    let (_db_dir, db) = test_db("image-gen-history-invalid.db");
    let storage = tempfile::tempdir().expect("storage tempdir");

    // Path traversal / separators / dots in the id.
    for bad_id in ["../evil", "a/b", "a\\b", "  ", "a.b"] {
        let payload = done_task_payload(bad_id, 1);
        let err = task_persist(&db, storage.path(), payload).expect_err("bad id should fail");
        assert!(err.to_string().contains("SEC_INVALID_INPUT"), "{bad_id}");
    }

    // Invalid status.
    let mut payload = done_task_payload("t1", 1);
    payload.status = "loading".to_string();
    let err = task_persist(&db, storage.path(), payload).expect_err("bad status should fail");
    assert!(err.to_string().contains("status must be"));

    // More thumbs than images.
    let mut payload = done_task_payload("t1", 1);
    payload.thumbs.push(file_payload(b"extra", "image/webp"));
    let err = task_persist(&db, storage.path(), payload).expect_err("extra thumbs should fail");
    assert!(err.to_string().contains("more thumbs than images"));

    // Invalid base64.
    let mut payload = done_task_payload("t1", 1);
    payload.images[0].data_b64 = "!!bad!!".to_string();
    let err = task_persist(&db, storage.path(), payload).expect_err("bad base64 should fail");
    assert!(err.to_string().contains("data_b64 is invalid"));

    // Oversized payload (rejected on the encoded-length pre-check).
    let mut payload = done_task_payload("t1", 1);
    payload.images[0].data_b64 = "A".repeat(97 * 1024 * 1024 / 3 * 4);
    let err = task_persist(&db, storage.path(), payload).expect_err("oversized should fail");
    assert!(err.to_string().contains("exceeds"));

    // Nothing persisted, no stray dirs.
    assert!(tasks_list(&db, storage.path(), None, 50)
        .expect("list")
        .is_empty());
    assert!(!storage.path().join("t1").exists());
}

#[test]
fn history_read_image_rejects_out_of_bounds_paths() {
    let (_db_dir, db) = test_db("image-gen-history-bounds.db");
    let outer = tempfile::tempdir().expect("outer tempdir");
    let storage = outer.path().join("store");
    std::fs::create_dir_all(&storage).expect("create storage");

    task_persist(&db, &storage, done_task_payload("t1", 1)).expect("persist");

    let secret = outer.path().join("secret.txt");
    std::fs::write(&secret, b"secret").expect("write secret");

    // Absolute path outside the storage dir.
    let err =
        read_image(&db, &storage, &secret.to_string_lossy()).expect_err("outside path should fail");
    assert!(err.to_string().contains("SEC_INVALID_INPUT"));

    // `..` traversal that resolves outside the storage dir.
    let traversal = storage.join("t1").join("..").join("..").join("secret.txt");
    let err =
        read_image(&db, &storage, &traversal.to_string_lossy()).expect_err("traversal should fail");
    assert!(err.to_string().contains("SEC_INVALID_INPUT"));

    // The storage root itself is not a servable file.
    let err =
        read_image(&db, &storage, &storage.to_string_lossy()).expect_err("root dir should fail");
    assert!(err.to_string().contains("SEC_INVALID_INPUT"));

    // Nonexistent path.
    let err = read_image(&db, &storage, &storage.join("nope.png").to_string_lossy())
        .expect_err("missing path should fail");
    assert!(err.to_string().contains("SEC_INVALID_INPUT"));

    // Symlink inside the storage dir escaping to the secret is rejected after
    // canonicalization.
    #[cfg(unix)]
    {
        let link = storage.join("t1").join("link.png");
        std::os::unix::fs::symlink(&secret, &link).expect("create symlink");
        let err = read_image(&db, &storage, &link.to_string_lossy())
            .expect_err("symlink escape should fail");
        assert!(err.to_string().contains("SEC_INVALID_INPUT"));
    }
}

#[test]
fn history_read_rejects_same_name_hardlink_swap_after_validation() {
    let (_db_dir, db) = test_db("image-gen-history-read-hardlink-swap.db");
    let outer = tempfile::tempdir().expect("outer");
    let storage = outer.path().join("storage");
    std::fs::create_dir_all(&storage).expect("storage");
    task_persist(&db, &storage, done_task_payload("t1", 1)).expect("persist");

    let image = storage.join("t1/image-1.png");
    let moved = storage.join("t1/image-original.png");
    let outside = outer.path().join("outside.png");
    std::fs::write(&outside, b"SYNTHETIC_SECRET_OUTSIDE").expect("outside");
    let hook_image = image.clone();
    let hook_moved = moved.clone();
    let hook_outside = outside.clone();
    set_before_history_read_open_test_hook(Box::new(move || {
        std::fs::rename(&hook_image, &hook_moved).expect("move validated image");
        std::fs::hard_link(&hook_outside, &hook_image).expect("same-name hardlink swap");
    }));

    let error = read_image(&db, &storage, "t1/image-1.png")
        .expect_err("identity-changing hardlink swap must fail closed");
    assert!(error.to_string().contains("SEC_INVALID_INPUT"));
    assert_eq!(
        std::fs::read(&outside).expect("outside remains readable"),
        b"SYNTHETIC_SECRET_OUTSIDE"
    );
}

#[test]
fn history_batch_hydration_reserves_budget_before_starting_next_read() {
    let (_db_dir, db) = test_db("image-gen-history-hydrate-budget.db");
    let storage = tempfile::tempdir().expect("storage");
    let mut payload = done_task_payload("t1", 1);
    payload.images = vec![
        file_payload(b"abc", "image/png"),
        file_payload(b"def", "image/png"),
    ];
    payload.thumbs.clear();
    payload.ref_images.clear();
    task_persist(&db, storage.path(), payload).expect("persist");

    let reads = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let hook_reads = reads.clone();
    set_before_history_file_read_test_hook(Box::new(move || {
        hook_reads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }));
    let roots = vec![storage.path().to_path_buf()];
    let references = vec!["t1/image-1.png".to_string(), "t1/image-2.png".to_string()];
    let error = read_images_with_budget_with_roots(&db, &roots, &references, 4, 4)
        .expect_err("aggregate budget must reject the second image before reading it");
    assert!(error.to_string().contains("hydration budget exceeded"));
    assert_eq!(reads.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[test]
fn history_persist_writes_through_validated_task_handle_after_path_rebind() {
    let (_db_dir, db) = test_db("image-gen-history-persist-path-rebind.db");
    let outer = tempfile::tempdir().expect("outer");
    let storage = outer.path().join("storage");
    let outside = outer.path().join("outside");
    std::fs::create_dir_all(&outside).expect("outside");
    let sentinel = outside.join("sentinel");
    std::fs::write(&sentinel, b"outside-unchanged").expect("sentinel");
    let task = storage.join("t1");
    let moved = storage.join("owned-moved");
    let hook_task = task.clone();
    let hook_moved = moved.clone();
    let hook_outside = outside.clone();
    set_before_persist_file_create_test_hook(Box::new(move || {
        std::fs::rename(&hook_task, &hook_moved).expect("move owned task dir");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&hook_outside, &hook_task).expect("rebind task symlink");
        #[cfg(windows)]
        junction::create(&hook_outside, &hook_task).expect("rebind task junction");
    }));

    task_persist(&db, &storage, done_task_payload("t1", 1))
        .expect_err("rebound task path must fail final validation");
    assert_eq!(
        std::fs::read(&sentinel).expect("outside sentinel"),
        b"outside-unchanged"
    );
    assert!(!outside.join("image-1.png").exists());
    assert!(!outside.join("thumb-1.webp").exists());
    assert!(!outside.join("ref-1.png").exists());
    let conn = db.open_connection().expect("db");
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM image_gen_tasks", [], |row| row.get(0))
        .expect("row count");
    assert_eq!(count, 0);
}

#[test]
fn history_read_image_rejects_db_recorded_dirs_outside_current_trusted_root() {
    let (_db_dir, db) = test_db("image-gen-history-old-dir.db");
    let old_storage = tempfile::tempdir().expect("old storage");
    let new_storage = tempfile::tempdir().expect("new storage");

    task_persist(&db, old_storage.path(), done_task_payload("t1", 1)).expect("persist");
    let err = read_image(&db, new_storage.path(), "t1/image-1.png")
        .expect_err("DB path outside the trusted root must fail closed");
    assert!(err.to_string().contains("SEC_INVALID_INPUT"));
}

#[test]
fn history_tasks_remain_operable_across_allowlisted_storage_roots() {
    let (_db_dir, db) = test_db("image-gen-history-multiple-roots.db");
    let old_storage = tempfile::tempdir().expect("old storage");
    let new_storage = tempfile::tempdir().expect("new storage");
    task_persist(&db, old_storage.path(), done_task_payload("old", 1)).expect("persist old");
    task_persist(&db, new_storage.path(), done_task_payload("new", 2)).expect("persist new");
    let roots = vec![
        old_storage.path().to_path_buf(),
        new_storage.path().to_path_buf(),
    ];

    let listed = tasks_list_with_roots(&db, &roots, None, 50).expect("list both roots");
    assert_eq!(
        listed
            .iter()
            .map(|task| task.id.as_str())
            .collect::<Vec<_>>(),
        vec!["new", "old"]
    );
    let fetched = read_image_with_roots(&db, &roots, "old/image-1.png")
        .expect("read old task after root switch");
    assert_eq!(fetched.data_b64, b64(b"png-bytes"));
    let stats =
        storage_stats_with_roots(&db, new_storage.path(), &roots).expect("multi-root stats");
    assert_eq!(stats.task_count, 2);
    assert_eq!(stats.dir, new_storage.path().to_string_lossy());
    assert!(stats.total_bytes > 0);

    assert_eq!(
        storage_cleanup_with_roots(&db, &roots, 1).expect("cleanup across roots"),
        1
    );
    assert!(!old_storage.path().join("old").exists());
    assert!(new_storage.path().join("new").exists());
    assert_eq!(
        tasks_list_with_roots(&db, &roots, None, 50)
            .expect("list after cleanup")
            .len(),
        1
    );
}

#[test]
fn history_tampered_db_dir_cannot_delete_outside_root_or_remove_row() {
    let (_db_dir, db) = test_db("image-gen-history-tampered-delete.db");
    let storage = tempfile::tempdir().expect("storage");
    let outside = tempfile::tempdir().expect("outside");
    task_persist(&db, storage.path(), done_task_payload("t1", 1)).expect("persist");

    let outside_task = outside.path().join("t1");
    std::fs::create_dir_all(&outside_task).expect("outside task dir");
    std::fs::write(outside_task.join("sentinel"), b"keep").expect("sentinel");
    let conn = db.open_connection().expect("open db");
    conn.execute(
        "UPDATE image_gen_tasks SET dir = ?1 WHERE id = 't1'",
        rusqlite::params![outside_task.to_string_lossy().to_string()],
    )
    .expect("tamper dir");
    drop(conn);

    let err = task_delete(&db, storage.path(), "t1").expect_err("delete must fail closed");
    assert!(err.to_string().contains("trusted storage root"));
    assert!(outside_task.join("sentinel").exists());
    assert!(tasks_list(&db, storage.path(), None, 50).is_err());
    let conn = db.open_connection().expect("open db after failed delete");
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM image_gen_tasks", [], |row| row.get(0))
        .expect("row count");
    assert_eq!(count, 1);
}

#[test]
fn history_clear_validates_all_db_dirs_before_any_delete() {
    let (_db_dir, db) = test_db("image-gen-history-tampered-clear.db");
    let storage = tempfile::tempdir().expect("storage");
    let outside = tempfile::tempdir().expect("outside");
    task_persist(&db, storage.path(), done_task_payload("good", 1)).expect("persist good");
    task_persist(&db, storage.path(), done_task_payload("bad", 2)).expect("persist bad");
    let outside_task = outside.path().join("bad");
    std::fs::create_dir_all(&outside_task).expect("outside task");
    let conn = db.open_connection().expect("open db");
    conn.execute(
        "UPDATE image_gen_tasks SET dir = ?1 WHERE id = 'bad'",
        rusqlite::params![outside_task.to_string_lossy().to_string()],
    )
    .expect("tamper dir");
    drop(conn);

    tasks_clear(&db, storage.path()).expect_err("clear must validate before deleting");
    assert!(storage.path().join("good").exists());
    assert!(storage.path().join("bad").exists());
    assert!(outside_task.exists());
    assert!(tasks_list(&db, storage.path(), None, 50).is_err());
    let conn = db.open_connection().expect("open db after failed clear");
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM image_gen_tasks", [], |row| row.get(0))
        .expect("row count");
    assert_eq!(count, 2);
}

#[test]
fn history_parent_link_rebinding_blocks_read_delete_clear_and_cleanup() {
    let (_db_dir, db) = test_db("image-gen-history-parent-rebind.db");
    let outer = tempfile::tempdir().expect("outer");
    let base = outer.path().join("base");
    let storage = base.join("storage");
    std::fs::create_dir_all(&storage).expect("create storage");
    task_persist(&db, &storage, done_task_payload("t1", 1)).expect("persist");

    let original = outer.path().join("original");
    std::fs::rename(&base, &original).expect("move trusted parent");
    let attacker = outer.path().join("attacker");
    let attacker_task = attacker.join("storage").join("t1");
    std::fs::create_dir_all(&attacker_task).expect("create attacker task");
    for (name, bytes) in [
        ("image-1.png", b"outside-image".as_slice()),
        ("thumb-1.webp", b"outside-thumb".as_slice()),
        ("ref-1.png", b"outside-ref".as_slice()),
    ] {
        std::fs::write(attacker_task.join(name), bytes).expect("write outside sentinel");
    }

    #[cfg(unix)]
    std::os::unix::fs::symlink(&attacker, &base).expect("create parent symlink");
    #[cfg(windows)]
    junction::create(&attacker, &base).expect("create parent junction");

    assert!(read_image(&db, &storage, "t1/image-1.png").is_err());
    assert!(task_delete(&db, &storage, "t1").is_err());
    assert!(tasks_clear(&db, &storage).is_err());
    assert!(storage_cleanup(&db, &storage, 0).is_err());
    assert_eq!(
        std::fs::read(attacker_task.join("image-1.png")).expect("outside image"),
        b"outside-image"
    );
    let conn = db.open_connection().expect("open db");
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM image_gen_tasks", [], |row| row.get(0))
        .expect("row count");
    assert_eq!(count, 1);
    drop(conn);

    #[cfg(unix)]
    std::fs::remove_file(&base).expect("remove parent symlink");
    #[cfg(windows)]
    junction::delete(&base).expect("remove parent junction");
    std::fs::rename(&original, &base).expect("restore trusted parent");
}

#[test]
fn history_delete_rechecks_task_identity_after_validation_barrier() {
    let (_db_dir, db) = test_db("image-gen-history-delete-barrier.db");
    let storage = tempfile::tempdir().expect("storage");
    let outside = tempfile::tempdir().expect("outside");
    task_persist(&db, storage.path(), done_task_payload("t1", 1)).expect("persist");
    let task_path = storage.path().join("t1");
    let moved_path = storage.path().join("moved-original");
    let outside_task = outside.path().join("outside-task");
    std::fs::create_dir_all(&outside_task).expect("outside task");
    let sentinel = outside_task.join("sentinel");
    std::fs::write(&sentinel, b"outside-stays").expect("outside sentinel");
    let hook_task_path = task_path.clone();
    let hook_moved_path = moved_path.clone();
    let hook_outside_task = outside_task.clone();
    set_before_quarantine_test_hook(Box::new(move || {
        std::fs::rename(&hook_task_path, &hook_moved_path).expect("swap task after validation");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&hook_outside_task, &hook_task_path)
            .expect("create task symlink");
        #[cfg(windows)]
        junction::create(&hook_outside_task, &hook_task_path).expect("create task junction");
    }));

    task_delete(&db, storage.path(), "t1").expect_err("identity swap must fail closed");
    assert_eq!(
        std::fs::read(&sentinel).expect("outside sentinel"),
        b"outside-stays"
    );
    assert!(moved_path.exists());
    let conn = db.open_connection().expect("open db");
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM image_gen_tasks", [], |row| row.get(0))
        .expect("row count");
    assert_eq!(count, 1);
    drop(conn);

    #[cfg(windows)]
    if task_path.exists() {
        junction::delete(&task_path).expect("remove task junction");
    }
}

#[cfg(windows)]
#[test]
fn history_delete_rejects_quarantine_rebinding_after_identity_validation() {
    let (_db_dir, db) = test_db("image-gen-history-quarantine-barrier.db");
    let storage = tempfile::tempdir().expect("storage");
    let outside = tempfile::tempdir().expect("outside");
    task_persist(&db, storage.path(), done_task_payload("t1", 1)).expect("persist");
    let outside_task = outside.path().join("outside-task");
    std::fs::create_dir_all(&outside_task).expect("outside task");
    let sentinel = outside_task.join("sentinel");
    std::fs::write(&sentinel, b"outside-stays").expect("outside sentinel");
    let moved_quarantine = storage.path().join("moved-quarantine");
    let hook_root = storage.path().to_path_buf();
    let hook_outside = outside_task.clone();
    let hook_moved = moved_quarantine.clone();
    set_after_quarantine_validation_test_hook(Box::new(move || {
        let quarantine = std::fs::read_dir(&hook_root)
            .expect("read storage root")
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .find(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with(".aio-quarantine-"))
            })
            .expect("quarantine exists after rename");
        std::fs::rename(&quarantine, &hook_moved).expect("move validated quarantine");
        junction::create(&hook_outside, &quarantine).expect("replace quarantine with junction");
    }));

    task_delete(&db, storage.path(), "t1").expect_err("quarantine rebinding must fail closed");
    assert_eq!(
        std::fs::read(&sentinel).expect("outside sentinel"),
        b"outside-stays"
    );
    assert!(moved_quarantine.join("image-1.png").exists());
    let conn = db.open_connection().expect("open db");
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM image_gen_tasks", [], |row| row.get(0))
        .expect("row count");
    assert_eq!(count, 1);
    drop(conn);

    let rebound = std::fs::read_dir(storage.path())
        .expect("read storage cleanup")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(".aio-quarantine-"))
        })
        .expect("rebound junction remains");
    junction::delete(&rebound).expect("remove rebound quarantine junction");
}

#[cfg(windows)]
#[test]
fn history_delete_rejects_child_replacement_between_handle_enumeration_and_open() {
    let (_db_dir, db) = test_db("image-gen-history-child-file-id-barrier.db");
    let storage = tempfile::tempdir().expect("storage");
    let outside = tempfile::tempdir().expect("outside");
    task_persist(&db, storage.path(), done_task_payload("t1", 1)).expect("persist");
    let sentinel = outside.path().join("sentinel");
    std::fs::write(&sentinel, b"outside-stays").expect("outside sentinel");
    let hook_root = storage.path().to_path_buf();
    let hook_sentinel = sentinel.clone();
    set_after_windows_directory_enumeration_test_hook(Box::new(move || {
        let quarantine = std::fs::read_dir(&hook_root)
            .expect("read storage root")
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .find(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with(".aio-quarantine-"))
            })
            .expect("quarantine exists during handle enumeration");
        let children = std::fs::read_dir(&quarantine)
            .expect("read quarantine children")
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .collect::<Vec<_>>();
        for child in children {
            let name = child.file_name().expect("child name").to_owned();
            let moved = quarantine.join(format!("moved-{}", name.to_string_lossy()));
            std::fs::rename(&child, moved).expect("move enumerated child");
            std::fs::hard_link(&hook_sentinel, &child)
                .expect("replace enumerated child with outside hardlink");
        }
    }));

    task_delete(&db, storage.path(), "t1").expect_err("child file-id replacement must fail closed");
    assert_eq!(
        std::fs::read(&sentinel).expect("outside sentinel"),
        b"outside-stays"
    );
    let quarantine = std::fs::read_dir(storage.path())
        .expect("read storage root")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(".aio-quarantine-"))
        })
        .expect("quarantine remains after fail-closed delete");
    assert!(quarantine.join("moved-image-1.png").exists());
    let conn = db.open_connection().expect("open db");
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM image_gen_tasks", [], |row| row.get(0))
        .expect("row count");
    assert_eq!(count, 1);
}

#[test]
fn history_read_rejects_tampered_stored_filename_even_inside_root() {
    let (_db_dir, db) = test_db("image-gen-history-tampered-file.db");
    let storage = tempfile::tempdir().expect("storage");
    task_persist(&db, storage.path(), done_task_payload("t1", 1)).expect("persist");
    let conn = db.open_connection().expect("open db");
    conn.execute(
        "UPDATE image_gen_tasks SET images_json = ?1 WHERE id = 't1'",
        rusqlite::params![r#"[{"file":"../image-1.png","mime":"image/png"}]"#],
    )
    .expect("tamper file metadata");
    drop(conn);

    let list_err = tasks_list(&db, storage.path(), None, 50)
        .expect_err("unsafe DB filename must fail the entire list");
    assert!(list_err
        .to_string()
        .contains("unsafe stored image filename"));
    let err = read_image(&db, storage.path(), "t1/image-1.png")
        .expect_err("unsafe DB filename must fail closed");
    assert!(err.to_string().contains("unsafe stored image filename"));
}

#[test]
fn history_cleanup_and_clear_boundaries() {
    let (_db_dir, db) = test_db("image-gen-history-cleanup.db");
    let storage = tempfile::tempdir().expect("storage tempdir");

    for (id, created_at) in [("t1", 1_i64), ("t2", 2), ("t3", 3)] {
        task_persist(&db, storage.path(), done_task_payload(id, created_at)).expect("persist");
    }

    // keep_count larger than total: nothing deleted.
    assert_eq!(
        storage_cleanup(&db, storage.path(), 10).expect("cleanup keep 10"),
        0
    );
    assert_eq!(
        tasks_list(&db, storage.path(), None, 50)
            .expect("list")
            .len(),
        3
    );

    // keep_count 1: the two oldest tasks (rows + dirs) are deleted.
    assert_eq!(
        storage_cleanup(&db, storage.path(), 1).expect("cleanup keep 1"),
        2
    );
    let remaining = tasks_list(&db, storage.path(), None, 50).expect("list");
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].id, "t3");
    assert!(!storage.path().join("t1").exists());
    assert!(!storage.path().join("t2").exists());
    assert!(storage.path().join("t3").exists());

    // keep_count 0 behaves like clear.
    task_persist(&db, storage.path(), done_task_payload("t4", 4)).expect("persist");
    assert_eq!(
        storage_cleanup(&db, storage.path(), 0).expect("cleanup keep 0"),
        2
    );
    assert!(tasks_list(&db, storage.path(), None, 50)
        .expect("list")
        .is_empty());
    assert!(!storage.path().join("t3").exists());
    assert!(!storage.path().join("t4").exists());

    // tasks_clear deletes rows and dirs and reports the count.
    task_persist(&db, storage.path(), done_task_payload("t5", 5)).expect("persist");
    task_persist(&db, storage.path(), done_task_payload("t6", 6)).expect("persist");
    assert_eq!(tasks_clear(&db, storage.path()).expect("clear"), 2);
    assert!(tasks_list(&db, storage.path(), None, 50)
        .expect("list")
        .is_empty());
    assert!(!storage.path().join("t5").exists());
    assert!(!storage.path().join("t6").exists());
}

#[test]
fn history_persist_removes_task_dir_when_db_write_fails() {
    let (_db_dir, db) = test_db("image-gen-history-rollback.db");
    let storage = tempfile::tempdir().expect("storage tempdir");

    {
        let conn = db.open_connection().expect("open db");
        conn.execute_batch("DROP TABLE image_gen_tasks")
            .expect("drop table to force db failure");
    }

    let err = task_persist(&db, storage.path(), done_task_payload("t1", 1))
        .expect_err("persist should fail when the table is gone");
    assert!(err.to_string().contains("DB_ERROR"));
    assert!(
        !storage.path().join("t1").exists(),
        "task dir must be rolled back after db failure"
    );
}

#[test]
fn history_persist_before_insert_failure_removes_owned_dir_without_db_row() {
    let (_db_dir, db) = test_db("image-gen-history-before-insert.db");
    let storage = tempfile::tempdir().expect("storage tempdir");
    set_persist_failure_point(PersistFailurePoint::BeforeInsert);

    task_persist(&db, storage.path(), done_task_payload("t1", 1))
        .expect_err("injected before-insert failure");

    assert!(!storage.path().join("t1").exists());
    let conn = db.open_connection().expect("open db");
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM image_gen_tasks", [], |row| row.get(0))
        .expect("row count");
    assert_eq!(count, 0);
}

#[test]
fn history_persist_post_insert_validation_failure_rolls_back_row_and_owned_dir() {
    let (_db_dir, db) = test_db("image-gen-history-post-insert.db");
    let storage = tempfile::tempdir().expect("storage tempdir");
    set_persist_failure_point(PersistFailurePoint::PostInsertValidation);

    let error = task_persist(&db, storage.path(), done_task_payload("t1", 1))
        .expect_err("post-insert validation failure");

    assert!(error.to_string().contains("SEC_INVALID_INPUT"));
    assert!(!storage.path().join("t1").exists());
    let conn = db.open_connection().expect("open db");
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM image_gen_tasks", [], |row| row.get(0))
        .expect("row count");
    assert_eq!(count, 0);
}

#[cfg(unix)]
#[test]
fn history_ensure_writable_dir_rejects_unwritable_path() {
    use std::os::unix::fs::PermissionsExt;

    let outer = tempfile::tempdir().expect("outer tempdir");
    let readonly = outer.path().join("readonly");
    std::fs::create_dir_all(&readonly).expect("create readonly dir");
    std::fs::set_permissions(&readonly, std::fs::Permissions::from_mode(0o555)).expect("chmod");

    let err = ensure_writable_dir(&readonly.join("sub")).expect_err("unwritable dir should fail");
    assert!(err.to_string().contains("SEC_INVALID_INPUT"));

    // Restore permissions so the tempdir can be cleaned up.
    std::fs::set_permissions(&readonly, std::fs::Permissions::from_mode(0o755))
        .expect("chmod restore");
}

#[test]
fn history_ensure_writable_dir_creates_missing_dir() {
    let outer = tempfile::tempdir().expect("outer tempdir");
    let target = outer.path().join("a").join("b");

    ensure_writable_dir(&target).expect("should create and validate dir");
    assert!(target.is_dir());
    assert!(!target.join(".aio-write-probe").exists());
}

#[test]
fn history_task_row_serialization_has_no_sensitive_fields() {
    let (_db_dir, db) = test_db("image-gen-history-serialize.db");
    let storage = tempfile::tempdir().expect("storage tempdir");
    task_persist(&db, storage.path(), done_task_payload("t1", 1)).expect("persist");

    let listed = tasks_list(&db, storage.path(), None, 50).expect("list");
    let json = serde_json::to_string(&listed[0]).expect("serialize row");
    // Rows carry opaque references + metadata only; never paths, keys or image payloads.
    assert!(!json.contains("api_key"));
    assert!(!json.contains("apiKey"));
    assert!(!json.contains("dataB64"));
    assert_eq!(listed[0].dir, "t1");
    assert!(!json.contains(&storage.path().to_string_lossy().to_string()));
}

#[test]
fn storage_stats_rejects_reparse_or_symlink_entries() {
    let (_db_dir, db) = test_db("image-gen-history-stats-link.db");
    let storage = tempfile::tempdir().expect("storage");
    task_persist(&db, storage.path(), done_task_payload("t-link", 1)).expect("persist");
    let task_dir = storage.path().join("t-link");
    let outside = storage.path().join("outside-target");
    std::fs::create_dir_all(&outside).expect("outside");
    std::fs::write(outside.join("secret.bin"), b"secret").expect("secret");
    let link = task_dir.join("escape");

    #[cfg(windows)]
    {
        junction::create(&outside, &link).expect("create junction test entry");
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&outside, &link).expect("symlink dir");
    }

    let started = std::time::Instant::now();
    let err = storage_stats(&db, storage.path()).expect_err("link entry must fail closed");
    assert!(err.to_string().contains("SEC_INVALID_INPUT"));
    assert!(
        started.elapsed() < std::time::Duration::from_secs(2),
        "malicious entry must not hang stats"
    );
}

#[cfg(unix)]
#[test]
fn storage_stats_rejects_fifo_replacement_without_hanging() {
    const TEST_FILTER: &str = "storage_stats_rejects_fifo_replacement_without_hanging";
    if std::env::var_os("AIO_IMAGE_STATS_FIFO_WATCHDOG_CHILD").is_some() {
        use std::os::unix::ffi::OsStrExt as _;

        let (_db_dir, db) = test_db("image-gen-history-stats-fifo.db");
        let storage = tempfile::tempdir().expect("storage");
        task_persist(&db, storage.path(), done_task_payload("t-fifo", 1)).expect("persist");
        let task_dir = storage.path().join("t-fifo");
        let regular = task_dir.join("image-1.png");
        let moved = task_dir.join("image-original.png");
        let hook_regular = regular.clone();
        let hook_moved = moved.clone();
        set_before_stats_open_test_hook(Box::new(move |_| {
            std::fs::rename(&hook_regular, &hook_moved).expect("move enumerated image");
            let c_path =
                std::ffi::CString::new(hook_regular.as_os_str().as_bytes()).expect("fifo path");
            assert_eq!(unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) }, 0);
        }));

        let error = storage_stats_with_roots(&db, storage.path(), &[storage.path().to_path_buf()])
            .expect_err("FIFO replacement must fail closed");
        assert!(error.to_string().contains("SEC_INVALID_INPUT"), "{error}");
        return;
    }

    let mut child =
        std::process::Command::new(std::env::current_exe().expect("current test executable"))
            .arg(TEST_FILTER)
            .arg("--nocapture")
            .env("AIO_IMAGE_STATS_FIFO_WATCHDOG_CHILD", "1")
            .spawn()
            .expect("spawn image stats watchdog child");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        match child.try_wait().expect("poll image stats watchdog child") {
            Some(status) => {
                assert!(status.success(), "FIFO child failed: {status}");
                break;
            }
            None if std::time::Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                panic!(
                    "production image storage stats did not fail closed before watchdog deadline"
                );
            }
            None => std::thread::sleep(std::time::Duration::from_millis(10)),
        }
    }
}

#[test]
fn storage_stats_counts_nested_legal_tree() {
    let (_db_dir, db) = test_db("image-gen-history-stats-nested.db");
    let storage = tempfile::tempdir().expect("storage");
    task_persist(&db, storage.path(), done_task_payload("t-nested", 1)).expect("persist");
    let nested = storage.path().join("t-nested").join("a").join("b");
    std::fs::create_dir_all(&nested).expect("nested dirs");
    std::fs::write(nested.join("extra.bin"), b"abcdef").expect("extra");
    let stats = storage_stats(&db, storage.path()).expect("nested stats");
    assert!(stats.total_bytes >= 6);
}

#[test]
fn storage_stats_rejects_depth_over_limit() {
    let (_db_dir, db) = test_db("image-gen-history-stats-depth.db");
    let storage = tempfile::tempdir().expect("storage");
    task_persist(&db, storage.path(), done_task_payload("t-depth", 1)).expect("persist");
    let mut cur = storage.path().join("t-depth");
    for i in 0..66 {
        cur = cur.join(format!("d{i}"));
        std::fs::create_dir_all(&cur).expect("mkdir");
    }
    std::fs::write(cur.join("leaf.bin"), b"x").expect("leaf");
    let err = storage_stats(&db, storage.path()).expect_err("depth 65+ must fail");
    assert!(
        err.to_string().contains("max depth") || err.to_string().contains("SEC_INVALID_INPUT"),
        "unexpected: {err}"
    );
}

#[test]
fn storage_stats_rejects_entry_budget_overflow() {
    let (_db_dir, db) = test_db("image-gen-history-stats-entries.db");
    let storage = tempfile::tempdir().expect("storage");
    task_persist(&db, storage.path(), done_task_payload("t-entries", 1)).expect("persist");
    set_storage_stats_entry_count_seed_for_test(100_000);

    let err = storage_stats_with_roots(&db, storage.path(), &[storage.path().to_path_buf()])
        .expect_err("the real 100001st entry must fail closed");
    assert!(err.to_string().contains("max entries"), "unexpected: {err}");
}

#[test]
fn storage_stats_rejects_i64_max_plus_one_through_production_entry() {
    let (_db_dir, db) = test_db("image-gen-history-stats-byte-overflow.db");
    let storage = tempfile::tempdir().expect("storage");
    task_persist(&db, storage.path(), done_task_payload("t-bytes", 1)).expect("persist");
    set_storage_stats_byte_bias_for_test(i64::MAX as u64);

    let err = storage_stats_with_roots(&db, storage.path(), &[storage.path().to_path_buf()])
        .expect_err("i64::MAX plus one byte must fail closed");
    assert!(
        err.to_string().contains("representable range"),
        "unexpected: {err}"
    );
}

#[test]
fn storage_stats_rejects_enumerate_to_open_identity_race() {
    let (_db_dir, db) = test_db("image-gen-history-stats-identity-race.db");
    let storage = tempfile::tempdir().expect("storage");
    task_persist(&db, storage.path(), done_task_payload("t-race", 1)).expect("persist");
    let task_dir = storage.path().join("t-race");
    let replacement_dir = tempfile::tempdir().expect("replacement dir");
    std::fs::write(
        replacement_dir.path().join("replacement.bin"),
        b"replacement",
    )
    .expect("replacement file");
    let task_for_hook = task_dir.clone();
    let replacement_for_hook = replacement_dir.path().join("replacement.bin");
    set_before_stats_open_test_hook(Box::new(move |entry_name| {
        let original = task_for_hook.join(entry_name);
        let moved = task_for_hook.join(format!("moved-{entry_name}"));
        std::fs::rename(&original, &moved).expect("move enumerated entry");
        std::fs::rename(&replacement_for_hook, &original).expect("replace enumerated entry");
    }));

    let err = storage_stats_with_roots(&db, storage.path(), &[storage.path().to_path_buf()])
        .expect_err("identity replacement must fail closed");
    assert!(
        err.to_string().contains("identity changed") || err.to_string().contains("hard link"),
        "unexpected: {err}"
    );
}
