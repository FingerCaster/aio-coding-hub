# Provider OAuth Device-Flow Contract

## Scenario: Change Codex Or Grok Device Authorization

### 1. Scope / Trigger

Use this contract when changing Codex or Grok device authorization start/poll
requests, response parsing, polling intervals, expiry arithmetic, flow
cancellation, or token persistence. These paths consume untrusted remote JSON
and coordinate one process-owned OAuth flow across repeated IPC calls.

### 2. Signatures

The generated IPC start, poll, and cancel boundaries are implemented by:

```rust
pub(crate) async fn provider_oauth_start_device_flow(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    provider_id: i64,
) -> Result<ProviderOAuthDeviceCodeStartResult, String>;

pub(crate) async fn provider_oauth_poll_device_flow(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    input: ProviderOAuthDeviceCodePollInput,
) -> Result<ProviderOAuthDeviceCodePollResult, String>;

pub(crate) async fn provider_oauth_cancel_device_flow(
    flow_id: String,
) -> Result<ProviderOAuthDeviceCodeCancelResult, String>;
```

The generated poll result has required fields `completed: boolean`,
`slow_down: boolean`, `provider_id: number`, `provider_type: string`, and
`expires_at: number | null`.

Remote responses pass through the internal bounded adapter:

```rust
async fn read_device_json_value(
    response: reqwest::Response,
    context: &str,
) -> Result<(reqwest::StatusCode, Option<serde_json::Value>), String>;
```

### 3. Contracts

- Device authorization and token polling responses use the shared bounded JSON
  reader with a 256 KiB body cap. Do not call unbounded `text()` or `json()`.
- The active flow is a server-owned capability binding `flow_id`, provider id,
  CLI/provider type, device code, user code, and deadline. Poll IPC accepts only
  `flow_id`; callers cannot resubmit or redirect any bound ownership field.
- A successful body must be a JSON object of the expected response type.
  Required device code, user code, verification URI, and access-token fields
  are non-empty after trimming. Grok token success additionally requires the
  `Bearer` token type.
- Remote polling intervals are clamped to 1 through 60 seconds before the
  three-second safety margin is added with saturating arithmetic. Values such
  as `u64::MAX` produce 63, never panic or wrap.
- Device-code `expires_in` is bounded to `1..=86400` seconds. Token
  `expires_in` is required and bounded to `1..=31536000` seconds. Missing,
  zero, negative, overflowing, or larger values fail before token persistence.
  Epoch addition uses checked arithmetic.
- `authorization_pending` keeps the current flow and returns incomplete with
  `slow_down=false`. RFC 8628 `slow_down` returns incomplete with
  `slow_down=true`. The frontend adds 5000 ms to its current polling interval
  before the next poll and retains every increase.
  Expired, denied, or other recognized terminal token responses cancel only
  the matching current flow. Malformed input persists no token. Successful
  token persistence atomically completes the same flow; a stale or explicitly
  canceled flow cannot commit tokens.
- Returned errors and logs contain safe operation/status diagnostics only.
  They never reproduce a remote body, URL, device code, access/refresh token,
  ID token, or upstream error description.

### 4. Validation & Error Matrix

| Input / condition | Required result |
| --- | --- |
| Response exceeds 256 KiB | Fail bounded read; expose none of the body |
| Response is invalid JSON, not an object, or has empty required fields | Fail safely; persist no token |
| Remote interval is 0, 5, or `u64::MAX` | Poll interval is respectively 4, 8, or 63 seconds |
| Device expiry is 0 or greater than 86400 | Reject the start result and cancel the matching flow |
| Token expiry is missing, non-positive, greater than 31536000, or overflows epoch addition | Reject; persist no token and do not complete the flow |
| Grok success token type is missing or not `Bearer` | Terminal failure; persist no token |
| `authorization_pending` | Return incomplete and retain current flow ownership |
| `slow_down` after current interval `N` | Return `slow_down=true`; next and later intervals are at least `N+5s` |
| Expired or denied response | Cancel the matching flow and return a safe terminal error |
| Flow is canceled/replaced while a poll is pending | Late response cannot persist credentials or cancel the replacement flow |
| Provider CLI/type changes while a poll is pending | Reject before persistence; write no token |
| Poll supplies an unknown, expired, or stale flow id | Reject without a remote request or token write |
| Valid success for the current flow | Persist the validated token set and complete exactly that flow |

### 5. Good / Base / Bad Cases

- Good: a pending response returns incomplete and preserves the same flow;
  a later valid Bearer response for that flow is persisted exactly once.
- Good: `u64::MAX` as the remote interval is bounded to 63 seconds, while an
  extreme expiry is rejected instead of becoming a non-expiring login.
- Good: intervals `1s -> slow_down -> pending -> slow_down` produce poll times
  `0s, 6s, 12s, 23s`.
- Base: a valid device response supplies non-empty codes and verification URI,
  then normal polling eventually produces a validated token set.
- Bad: call `response.json()` before applying a body cap, or add the safety
  margin to an attacker-controlled interval before clamping it.
- Bad: persist a token before rechecking flow ownership, allowing a canceled
  or replaced request to commit late credentials.
- Bad: include the remote error description or token-shaped response fields in
  an IPC error or diagnostic log.

### 6. Tests Required

- Cover oversized, invalid JSON, non-object, empty required fields, and safe
  non-success status handling for both authorization and token endpoints.
- Cover zero/default/maximum intervals and expiry extremes without panic or
  arithmetic wrap; invalid token expiry cannot produce a complete outcome or
  reach persistence.
- Cover repeated `slow_down` and pending responses with fake-clock assertions;
  every increase is at least five seconds and remains in effect.
- Cover pending, expired, denied, invalid token type, explicit cancellation,
  replacement ownership, provider/CLI drift, code binding, cross-provider
  isolation, and the normal success/persistence path.
- Use `SYNTHETIC_SECRET` in remote bodies and token-shaped fields; assert it is
  absent from returned errors and captured logs.
- Run the focused OAuth command tests and the full Rust library suite after
  changing the shared bounded reader, flow ownership, or persistence behavior.

### 7. Wrong vs Correct

#### Wrong

```rust
let payload: DeviceTokenResponse = response.json().await?;
let delay = Duration::from_secs(payload.interval + 3);
let expires_at = now.saturating_add(payload.expires_in.unwrap_or(0));
```

The response allocation is unbounded and attacker-controlled arithmetic can
overflow before the delay is constructed.

#### Correct

```text
response -> 256 KiB bounded reader -> JSON object/type validation
         -> required-field validation -> bounded Result expiry arithmetic
         -> recheck ownership -> pending / slow_down(+5s) / atomic completion
```

Remote values describe a candidate device-flow transition; bounded parsing and
current-flow ownership must both succeed before they can affect local state.
