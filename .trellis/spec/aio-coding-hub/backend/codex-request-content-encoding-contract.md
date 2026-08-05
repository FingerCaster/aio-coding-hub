# Codex Request Content-Encoding Contract

## Scope

This contract applies only to Codex POST requests whose path segments end in:

- `responses`
- `responses/compact`
- `chat/completions`

The match permits `/v1/`, nested gateway prefixes, and trailing slashes. Other
requests keep the existing raw compressed passthrough or mutation/re-encoding
behavior.

## Separate Compression Features

Codex remote context compaction is a semantic operation that replaces older
conversation context with a compacted representation. HTTP request compression
is a transport operation represented by `Content-Encoding`. Enabling, routing,
or diagnosing one must not be treated as configuration for the other.

## Decode Boundary

Target requests are decoded after the raw body has been read within the gateway
request limit and before plugins, model inference, Session completion, request
logging, provider selection, or retry preparation.

All `Content-Encoding` field values are combined in wire order. Comma-separated
values are parsed case-insensitively, `identity` is ignored, and the remaining
layers are decoded in reverse order.

Supported coding names:

- `gzip`, `x-gzip`
- `deflate` as zlib-wrapped data, with raw Deflate fallback
- `br`
- `zstd`, `zst`

At most eight effective encoding layers are accepted.

## Size Invariant

The configured gateway request-body limit applies to the raw body and to the
output of every decode layer. No intermediate representation may exceed that
limit. This is required even when a later layer would decode to a smaller body.

## Plain Upstream Invariant

After successful normalization, the decoded bytes replace the request-body
state and these headers are removed:

- `Content-Encoding`
- `Content-Length`
- `Transfer-Encoding`

Every plugin mutation, provider attempt, internal retry, and failover continues
from this identity state. Target Codex requests must never restore the original
compressed bytes or be recompressed before upstream delivery.

## Failure Classification

- Unknown coding, malformed compressed data, or more than eight effective
  layers: HTTP 400 and `GW_INVALID_REQUEST_CONTENT_ENCODING`.
- Any decoded layer exceeding the request limit: the existing HTTP 413 body
  too-large contract.

Both failures terminate before provider selection. They make no upstream
attempt, do not update provider failure or circuit state, and do not retry.
Public errors and persisted logs must not include request-body content,
credentials, or decoder implementation details.

## Unchanged Behavior

This contract does not change:

- Codex remote context compaction
- provider naming or authentication
- response decompression
- the gateway's upstream `Accept-Encoding: identity`
- IPC, database, or configuration schemas
