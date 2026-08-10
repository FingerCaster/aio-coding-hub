# KNaiFen 参考 fork 压缩请求调研

## Source

- Repository: `https://github.com/KNaiFen/aio-coding-hub`
- Reviewed main: `5b13683bd2a44699cd8c99e7aeffc317bcc19674`
- Initial zstd support: `13a3c6ffe183a0e19ced2e63b425b78815504c3f`
- Final Codex normalization: `909b7a0d4c1c6f4903bb4e39b374112dd39a9947`
- Task archive: `4243335fb49b64ca61fd790bb328cb2189bded8c`

## Findings

- `13a3c6f` adds `zstd = "0.13"`, bounded zstd decode/encode, `RequestBodyEncoding::Zstd`, reasoning/session route coverage, unchanged raw passthrough and mutated re-encode.
- `909b7a0` is the later product contract: target Codex JSON requests are normalized before `GatewayRequestBody`, so they are always sent upstream as identity JSON.
- Supported target encodings are gzip/x-gzip, deflate (zlib then raw), br, zstd/zst, repeated headers and reverse-decoded stacked layers up to 8.
- Each decoded layer is bounded by the existing request body cap.
- Invalid/damaged/over-deep encodings return a structured local 400; decoded overflow returns 413. Both stop before Provider attempts.
- Non-target requests preserve the existing transparent passthrough/re-encode behavior.
- No later commit through reviewed main modifies `request_body.rs` or `http_util.rs`; later route/body-reader changes are adjacent observability/features rather than a replacement of the compression contract.

## Port Decision

Port the latest combined behavior of both commits, adapted to the current fork-specific gateway. Do not implement only a single zstd decoder and do not restore compressed transport for target Codex requests.
