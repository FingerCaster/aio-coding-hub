// Usage:
// - Shared parser for `request_logs.attempts_json` (serialized gateway FailoverAttempt list).
// - Single contract for the provider chain view and the error-card failure summary.
// - `timeout_secs` is the structured first-byte timeout; never parse it out of `outcome`.
// - Probe state is structured metadata; consumers must not infer it from `reason`.

import type { StreamInternalErrorEvidence } from "../../generated/bindings";

export type { StreamInternalErrorEvidence } from "../../generated/bindings";

const PROBE_METADATA_TEXT_MAX_LENGTH = 64;

export type AttemptProbeMetadata = {
  probe?: boolean | null;
  probe_trigger?: string | null;
  probe_result?: string | null;
  probe_generation?: number | null;
};

export type NormalizedAttemptProbeMetadata = {
  probe: boolean | null;
  probe_trigger: string | null;
  probe_result: string | null;
  probe_generation: number | null;
};

export type AttemptJsonEntry = AttemptProbeMetadata & {
  provider_id: number;
  provider_name: string;
  base_url: string;
  requested_upstream_model?: string | null;
  outcome: string;
  status: number | null;
  provider_index?: number | null;
  retry_index?: number | null;
  session_reuse?: boolean | null;
  error_category?: string | null;
  error_code?: string | null;
  decision?: string | null;
  reason?: string | null;
  selection_method?: string | null;
  reason_code?: string | null;
  attempt_started_ms?: number | null;
  attempt_duration_ms?: number | null;
  circuit_state_before?: string | null;
  circuit_state_after?: string | null;
  circuit_failure_count?: number | null;
  circuit_failure_threshold?: number | null;
  // Circuit attribution for gate-skip attempts; the backend omits both keys
  // entirely on success and non-circuit paths (space constraint).
  circuit_recover_at_unix?: number | null;
  circuit_trigger_error_code?: string | null;
  timeout_secs?: number | null;
  stream_internal_error?: StreamInternalErrorEvidence | null;
};

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function normalizeProbeText(value: unknown): string | null | undefined {
  if (value == null) return null;
  if (typeof value !== "string") return undefined;
  return value.slice(0, PROBE_METADATA_TEXT_MAX_LENGTH);
}

/**
 * Normalizes both current payloads (explicit nulls) and legacy payloads
 * (missing keys). `null` means the attempt has no value for that probe field;
 * an invalid field type rejects only the metadata projection.
 */
export function normalizeAttemptProbeMetadata(
  value: unknown
): NormalizedAttemptProbeMetadata | null {
  if (!isRecord(value)) return null;

  const probe = value.probe;
  const probeTrigger = normalizeProbeText(value.probe_trigger);
  const probeResult = normalizeProbeText(value.probe_result);
  const probeGeneration = value.probe_generation;

  if (probe != null && typeof probe !== "boolean") return null;
  if (probeTrigger === undefined || probeResult === undefined) return null;
  if (
    probeGeneration != null &&
    (typeof probeGeneration !== "number" ||
      !Number.isSafeInteger(probeGeneration) ||
      probeGeneration < 0)
  ) {
    return null;
  }

  return {
    probe: probe ?? null,
    probe_trigger: probeTrigger,
    probe_result: probeResult,
    probe_generation: probeGeneration ?? null,
  };
}

export function isCircuitProbeAttempt(
  value: AttemptProbeMetadata & { selection_method?: string | null }
): boolean {
  return (
    value.probe === true ||
    value.selection_method === "circuit_probe" ||
    value.probe_trigger != null ||
    value.probe_result != null ||
    value.probe_generation != null
  );
}

function asRequiredString(value: unknown): string | null {
  return typeof value === "string" && value.length > 0 ? value : null;
}

function asOptionalString(value: unknown): string | null {
  if (value == null) return null;
  return typeof value === "string" ? value : null;
}

export function parseStreamInternalErrorEvidence(
  value: unknown
): StreamInternalErrorEvidence | null {
  if (!isRecord(value)) return null;

  const eventType = asRequiredString(value.event_type);
  const classification = asRequiredString(value.classification);
  const disposition = asRequiredString(value.disposition);
  if (!eventType || !classification || !disposition || typeof value.truncated !== "boolean") {
    return null;
  }

  return {
    event_type: eventType,
    error_type: asOptionalString(value.error_type),
    error_code: asOptionalString(value.error_code),
    message: asOptionalString(value.message),
    classification,
    matched_keyword: asOptionalString(value.matched_keyword),
    disposition,
    truncated: value.truncated,
  };
}

export function parseAttemptsJson(raw: string | null | undefined): AttemptJsonEntry[] | null {
  if (!raw) return null;
  try {
    const parsed = JSON.parse(raw) as unknown;
    if (!Array.isArray(parsed)) return null;
    return parsed.map((entry) => {
      if (!isRecord(entry)) return entry as AttemptJsonEntry;
      return {
        ...(entry as AttemptJsonEntry),
        stream_internal_error: parseStreamInternalErrorEvidence(entry.stream_internal_error),
      };
    });
  } catch {
    return null;
  }
}
