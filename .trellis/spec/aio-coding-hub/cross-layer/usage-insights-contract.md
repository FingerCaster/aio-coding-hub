# Usage Insights Contract

## Scenario: Folder Usage, Development Time, And Provider Metrics Trends

### 1. Scope / Trigger

Use this contract when changing usage folder lookup/filtering, the v2 folder or
day leaderboard, estimated development time, provider latency/TTFB/output-rate
trends, their generated bindings, TanStack Query keys, or the Home/Usage UI.

The data flow is:

```text
request_logs + session folder lookup -> Rust aggregation -> generated binding
  -> frontend service normalization -> provider-scoped query key -> Home/Usage UI
```

### 2. Signatures

The generated IPC boundary is owned by these commands and DTOs:

```rust
async fn usage_leaderboard_v2(
    scope: String,
    params: UsageQueryParams,
    limit: Option<u32>,
) -> Result<Vec<UsageLeaderboardRow>, String>;

async fn usage_folder_options_v1(
    params: UsageQueryParams,
) -> Result<Vec<UsageFolderOptionV1>, String>;

async fn usage_provider_metrics_trend_v1(
    params: UsageQueryParams,
    limit: Option<u32>,
) -> Result<Vec<UsageProviderMetricsTrendRowV1>, String>;
```

`UsageQueryParams` uses camel-case fields `period`, `startTs`, `endTs`,
`cliKey`, `providerId`, `folderKeys`, `dayStartHour`, `fullIdleGapMinutes`,
`sessionBreakGapMinutes`, and `excludeCx2CcGatewayBridge`. The accepted scopes
are `cli`, `provider`, `model`, `folder`, and `day`; periods are `daily`,
`weekly`, `monthly`, `allTime`, and `custom`.

The leaderboard adds `folder_path`, `first_request_created_at_ms`,
`last_request_created_at_ms`, `last_request_completed_at_ms`,
`estimated_development_time_ms`, and a nullable 24-element
`hourly_estimated_development_time_ms`. A metrics row contains its local-time
bucket (`day`, optional `hour`), stable provider key/name, nullable average
duration/TTFB/output rate, and successful request count.

### 3. Contracts

- Folder lookup is available only for Claude and Codex sessions. A resolved,
  non-empty folder path is the stable folder key. Missing, unsupported, or
  invalid metadata maps to the selectable `__unknown__` bucket with the
  localized unknown-folder label and a null path. Folder options sort by
  tokens, requests, name, then key; folder filters apply consistently to
  summary, leaderboard, and day detail.
- Folder keys are trimmed, de-duplicated, and sorted at the frontend boundary;
  an empty collection becomes null. The backend repeats trim/de-dup and treats
  an empty result as no folder filter.
- Development time is produced only for `day` and `folder` leaderboard scopes.
  Request intervals use `created_at_ms` when positive, otherwise epoch seconds,
  clamp negative durations to zero, and merge overlaps before gap accounting.
- Gaps up to `fullIdleGapMinutes` count fully; gaps above
  `sessionBreakGapMinutes` count zero. Intermediate gaps use the linear weight
  `gap * (sessionBreak - gap) / (sessionBreak - fullIdle)`. Defaults are 15 and
  30 minutes, valid ranges are 1..=30 and 15..=60, and full-idle must remain
  strictly below session-break.
- A day estimate is capped at 24 hours and is distributed across exactly 24
  local-hour buckets. Folder estimates are computed per folder/day and then
  summed, so idle time never bridges calendar-day boundaries. `dayStartHour`
  is limited to 0..=9 and owns day/folder bucketing.
- Metrics trends use successful, non-excluded request logs with a positive
  final provider id. `excludeCx2CcGatewayBridge` must affect both top-provider
  selection and returned buckets. Provider names come from the provider row,
  then the final attempt fallback; unresolved/invalid identities are dropped.
- Trend buckets are local hour for `daily`, local day for
  `weekly`/`monthly`/`custom`, and local month for `allTime`. Trends deliberately
  ignore `folderKeys`, `dayStartHour`, and development-time thresholds.
- Average duration divides successful duration by successful request count.
  TTFB and output rate include only rows where TTFB is present and strictly
  less than duration. Output rate is total output tokens divided by total
  post-TTFB generation seconds; unavailable denominators produce null.
- Public frontend limits are safe integers normalized to 1..=200. A null
  leaderboard limit defaults to 200; a null metrics-trend limit selects all
  eligible providers. Query keys include every normalized filter that can
  change a result and omit fields deliberately ignored by that endpoint.
  `keepPreviousData` is presentation only and does not create a second cache
  owner.
- Home development-time thresholds are a UI preference under
  `homeUsageDevelopmentTimeThresholds`. Invalid JSON, storage failures, invalid
  ranges, or reversed thresholds fall back to 15/30 without weakening backend
  validation; same-tab and `storage` events notify all subscribers.

### 4. Validation & Error Matrix

| Input / condition | Required result |
| --- | --- |
| Unknown period or scope | `SEC_INVALID_INPUT`; run no aggregation |
| Invalid CLI key or non-positive provider id | `SEC_INVALID_INPUT` |
| Timestamp is negative/non-safe at the frontend | `SEC_INVALID_INPUT` before IPC |
| `folderKeys` is not a string array | `SEC_INVALID_INPUT` before IPC |
| Empty/duplicate folder keys | Drop empties, de-duplicate; all-empty becomes no filter |
| `dayStartHour` outside 0..=9 | `SEC_INVALID_INPUT` |
| Full-idle outside 1..=30 or break outside 15..=60 | `SEC_INVALID_INPUT` |
| Full-idle is greater than or equal to break | `SEC_INVALID_INPUT` |
| Session has no supported folder metadata | Aggregate under `__unknown__`, never drop usage |
| TTFB is null or greater than/equal to duration | Exclude it from TTFB and output-rate averages |
| Trend limit is above 200 | Clamp to 200 |
| Provider name cannot be validated | Omit that provider trend row |
| Storage is absent, malformed, or unwritable | Use defaults; queries remain valid |

### 5. Good / Base / Bad Cases

- Good: overlapping requests merge into one interval, a short idle gap is
  weighted once, and the day total equals the sum of its 24 hourly buckets.
- Good: selecting a folder changes summary, day, provider, and model queries
  through the same normalized folder key while the unknown bucket remains
  selectable.
- Good: a weekly provider trend excludes failed requests and invalid TTFB,
  keeps the bridge exclusion in its query key, and returns null for a missing
  rate denominator.
- Base: no folder filter and default 15/30 thresholds preserve ordinary usage
  aggregation; providers without session metadata remain visible as unknown.
- Bad: key a trend without `providerId`, `limit`, or bridge exclusion, causing
  one cached result to satisfy a different request.
- Bad: add raw gaps between every request without merging overlaps or splitting
  by day, which double-counts activity and joins separate work sessions.

### 6. Tests Required

- Rust: folder resolution, unknown bucket, duplicate/malformed lookup rows,
  folder-filter propagation, deterministic ordering, and day-detail parity.
- Rust: overlap merging, zero/negative durations, full/weighted/zero gap
  boundaries, reversed-threshold rejection, day-start offsets, 24-hour cap,
  hourly sum parity, folder/day isolation, and empty-day rows.
- Rust: trend bucket selection, top-provider limit, bridge exclusion, provider
  name fallback, success filtering, invalid TTFB exclusion, duration/TTFB/rate
  arithmetic, and null denominator behavior.
- TypeScript: normalization and canonical query keys for reordered folder keys,
  every threshold/filter/limit, and endpoint-specific ignored fields.
- UI: folder selection and unknown display, persisted threshold fallback and
  subscriptions, development-time/request-duration mode switching, empty/error
  states, metric selection, tooltip ordering, and invalid chart payloads.
- Run generated-binding verification, focused frontend/Rust tests, `pnpm build`,
  full precommit/prepush gates, Rustfmt, and Clippy after cross-layer changes.

### 7. Wrong vs Correct

#### Wrong

```text
query key = ["usage", "metrics", period]
estimated time = sum(duration) + every gap between request rows
output rate = sum(output_tokens) / sum(duration)
```

This aliases provider/filter requests, double-counts overlapping activity, and
includes latency before the first token in generation rate.

#### Correct

```text
normalized filters -> complete endpoint-owned query key
requests -> merged intervals -> bounded weighted gaps -> per-day/per-hour estimate
valid successful TTFB rows -> output_tokens / (duration - TTFB)
```

Each layer preserves the same filter identity and the backend remains the sole
owner of aggregation semantics.

## Scenario: Cost Aggregates Beyond Signed 64-Bit Totals

### 1. Scope / Trigger

Session, folder/day/provider/model leaderboards, usage summaries and provider
spend gates aggregate stored request costs across arbitrarily many rows.

### 2. Signatures

Persisted per-request `cost_usd_femto` remains unchanged. Aggregation uses
SQLite `TOTAL(COALESCE(cost_usd_femto, 0))` and Rust `f64` consumers,
including `SessionStatsAggregate.total_cost_usd_femto`. USD conversion uses
`1e15`; published number DTOs and query keys keep their existing shape.

### 3. Contracts

Never decode a floating aggregate through `i64` or sum femto values with
SQLite integer `SUM`. Preserve provider/client usage ownership and exclusions.
Clamp negative aggregate usage to zero at existing consumer boundaries; active
session cost remains None for nonpositive totals. A request with positive
output usage and no effective output rate has unknown cost (None), not a
partially priced input-only total. Priority rows may use existing base-rate
fallback; Actual service-tier precedence and incremental log updates remain.

### 4. Validation / Error Matrix

| Case | Expected |
| --- | --- |
| Two valid stored costs sum above i64 max | Finite f64 aggregate, successful query |
| Empty aggregate | Zero; session optional presentation remains None |
| Negative aggregate | Existing nonnegative consumer projection |
| Positive output tokens, missing effective output rate | Unknown request cost |
| Missing priority output rate, present base rate | Existing base fallback |

### 5. Examples

Good: two `3_i64 << 61` femto rows aggregate before USD conversion.
Boundary: a zero-cost active session keeps its prior nullable UI contract.
Bad: silently overflow to an integer or report input-only cost as fully known.

### 6. Tests

Overflow fixtures cover request-log sessions, gateway session projection,
provider limit gates, provider usage and all leaderboard scopes. Domain cost
tests cover missing output prices and base fallback. Existing tier tests
retain Actual priority billing and raw/client usage separation.

### 7. Wrong / Correct

Wrong: `row.get::<_, i64>("total_cost")? as f64` after an integer SUM.
Correct: SQL TOTAL and f64 decoding throughout the aggregate consumer chain.
