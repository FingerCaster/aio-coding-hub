# Multi-provider 503 root-cause research

## User observation

Within roughly two minutes, the screenshot repeatedly showed:

- Codex / `gpt-5.6-sol-max`: 503, `GW_ALL_PROVIDERS_UNAVAILABLE`, no final provider,
  “切换 2 次”, about 13 ms.
- Adjacent `AI INPUT-Air`: 502 upstream 4XX, “切换 3 次”, session reuse, about
  5.89/6.16 seconds.
- `AI INPUT-Plus`: an intervening 200 success for the same displayed model with session reuse.

These labels alone do not prove how many providers were eligible or called because UI `attempt_count`
mixes skips, retries and providers.

## Read-only local log reproduction

The SQLite database under `.aio-coding-hub` contains the exact timing pattern:

| Log | Local time | Final | Duration | Attempt evidence |
| --- | --- | --- | ---: | --- |
| 61005 | 00:31:33.552 | 502 `GW_UPSTREAM_4XX` | 6159 ms | Plus 429; muyuan skipped OPEN; Air 429 |
| 61006 | 00:31:39.908 | 503 all unavailable | 13 ms | Air skipped cooldown; muyuan skipped OPEN |
| 61009 | 00:31:50.211 | 502 `GW_UPSTREAM_4XX` | 5892 ms | Plus 429; muyuan skipped OPEN; Air 429 |
| 61010 | 00:31:56.327 | 503 all unavailable | 13 ms | Air skipped cooldown; muyuan skipped OPEN |

The 502/503 pairs share one session. Nearby Plus 200 rows belong to another session and completed
after their requests had already started; they do not prove Plus was gate-eligible at the 503 instant.
No request body, base URL, trace/session identifier or credential was extracted.

The persisted settings at investigation time were:

- `failover_max_attempts_per_provider = 5`
- `failover_max_providers_to_try = 5`
- `circuit_breaker_failure_threshold = 5`
- `provider_cooldown_seconds = 30`

## Code cause

1. `src-tauri/src/gateway/proxy/handler/provider_selection.rs:131` obtains the session-bound provider.
   At lines 142-145, a denied circuit decision removes that provider from the vector and returns
   before the failover loop can record it.
2. `src-tauri/src/gateway/proxy/handler/failover_loop/prepare/provider_checks.rs:41` is the common gate.
   Lines 62-84 encode OPEN/cooldown denial as a skipped attempt with circuit diagnostics.
3. `src-tauri/src/gateway/proxy/handler/failover_loop/mod.rs:311` iterates the remaining vector and
   checks the configured Ready-provider budget at lines 312-314.
4. `src-tauri/src/gateway/proxy/handler/failover_loop/prepare/provider_iterator.rs:331` increments
   `providers_tried` only after preparation succeeds. A gate skip therefore does not consume the cap.
5. `src-tauri/src/infra/request_logs/queries.rs:328` defines `attempt_count` as parsed attempts length;
   lines 335-338 separately define failover from route hops.
6. `src/components/home/requestLogPresentation.ts:591` receives both route and attempt count, but lines
   614-615 and 641-643 label raw attempt count as switches.

## Regression origin

`git blame` identifies commit `7088dcf45e7b8d07a26e1e63a20b9f3e3967b75f` as adding
`providers.retain(|provider| provider.id != bound_provider_id)` on 2026-07-04. Before that commit, the
temporarily denied bound provider remained available for the common gate to skip and record. The same
commit changed a unit test to require vector removal, so the regression is reproducible without a live
provider.

## Falsifiable conclusion

- The observed 503 was not caused by a provider limit of two and did not omit a currently allowed
  third upstream call. All three configured candidates were unavailable at that decision point.
- The real defect is that one unavailable candidate was erased before the canonical gate, producing
  incomplete attempts/route evidence; the UI then described attempt rows as switches.
- This conclusion is falsified if a post-fix exact route test either sends an upstream request to a
  denied provider or still produces fewer than three skip rows for the reproduced candidate set.

## Regression matrix

| Case | Expected |
| --- | --- |
| Bound A cooldown, B OPEN, C cooldown | 503, three skipped hops, zero upstream calls |
| Bound A denied, B fails, C succeeds | A skipped, B failed, C success; binding preference does not bypass gate |
| A/B/C all eligible, cap 5 | all needed providers can be attempted |
| A/B/C all eligible, cap 2 | only two Ready providers consume the cap |
| One provider retries twice then B succeeds | route provider count 2, transition count 1, attempt rows 3 |
| Model discovery | one attempt per provider, failover allowed, circuit health neutral |
| Stream/non-stream, large body, cancellation | unchanged usage/response-id/TTFB/20 MiB contracts |

## Risk and rollback

The later common gate becomes the sole circuit decision, so a provider crossing a cooldown boundary
between selection and preparation may become allowed. Tests must lock this intended single-owner
behavior. No schema migration is involved; revert the coherent selection/presentation commit if route
or circuit regressions appear.
