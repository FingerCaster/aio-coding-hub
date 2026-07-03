import { describe, expect, it, vi } from "vitest";
import { commands } from "../../../generated/bindings";
import { logToConsole } from "../../consoleLog";
import { createRequestLogDetail, createRequestLogSummary } from "../requestLogFixtures";
import {
  REQUEST_ATTEMPT_LOGS_MAX_LIMIT,
  REQUEST_LOGS_MAX_LIMIT,
  REQUEST_LOGS_MIN_LIMIT,
  REQUEST_LOG_TRACE_ID_MAX_LENGTH,
  type RequestAttemptLog,
  normalizeRequestAttemptLogsLimit,
  normalizeRequestLogCursorId,
  normalizeRequestLogId,
  normalizeRequestLogTraceId,
  normalizeRequestLogsLimit,
  requestAttemptLogsByTraceId,
  requestLogGet,
  requestLogGetByTraceId,
  requestLogsCodexReasoningGuardStats,
  requestLogsList,
  requestLogsListAfterId,
  requestLogsListAfterIdAll,
  requestLogsListAll,
  type CodexReasoningGuardStats,
} from "../requestLogs";

vi.mock("../../../generated/bindings", async () => {
  const actual = await vi.importActual<typeof import("../../../generated/bindings")>(
    "../../../generated/bindings"
  );
  return {
    ...actual,
    commands: {
      ...actual.commands,
      requestLogsList: vi.fn(),
      requestLogsListAll: vi.fn(),
      requestLogsListAfterId: vi.fn(),
      requestLogsListAfterIdAll: vi.fn(),
      requestLogGet: vi.fn(),
      requestLogGetByTraceId: vi.fn(),
      requestAttemptLogsByTraceId: vi.fn(),
      requestLogsCodexReasoningGuardStats: vi.fn(),
    },
  };
});

function makeRequestAttemptLog(overrides: Partial<RequestAttemptLog> = {}): RequestAttemptLog {
  return {
    id: 1,
    trace_id: "trace-1",
    cli_key: "claude",
    attempt_index: 0,
    provider_id: 1,
    provider_name: "Provider",
    base_url: "https://example.com",
    outcome: "success",
    status: 200,
    attempt_started_ms: 1,
    attempt_duration_ms: 2,
    created_at: 1,
    ...overrides,
  };
}

function makeCodexReasoningGuardStats(
  overrides: Partial<CodexReasoningGuardStats> = {}
): CodexReasoningGuardStats {
  return {
    hit_request_count: 3,
    hit_attempt_count: 7,
    token_hit_attempt_count: 7,
    feature_hit_attempt_count: 0,
    reasoning_token_hit_request_count: 3,
    final_answer_only_high_xhigh_hit_request_count: 0,
    normal_request_count: 9,
    total_request_count: 12,
    hit_rate: 0.25,
    feature_sample_request_count: 0,
    feature_sample_count: 0,
    final_answer_only_sample_count: 0,
    high_xhigh_final_answer_only_sample_count: 0,
    reasoning_516_final_answer_only_no_commentary_count: 0,
    compaction_exempt_sample_count: 0,
    reasoning_tokens_coverage_count: 0,
    final_answer_only_coverage_count: 0,
    commentary_observed_coverage_count: 0,
    reasoning_effort_coverage_count: 0,
    duration_ms_coverage_count: 0,
    output_tokens_coverage_count: 0,
    continuation_triggered_request_count: 0,
    continuation_triggered_attempt_count: 0,
    continuation_repaired_request_count: 0,
    continuation_repaired_attempt_count: 0,
    continuation_non_repaired_attempt_count: 0,
    continuation_repair_rate: 0,
    continuation_average_sent_rounds: 0,
    continuation_by_status: [],
    by_model: [],
    by_model_and_effort: [],
    ...overrides,
  };
}

vi.mock("../../consoleLog", async () => {
  const actual = await vi.importActual<typeof import("../../consoleLog")>("../../consoleLog");
  return {
    ...actual,
    logToConsole: vi.fn(),
  };
});

describe("services/gateway/requestLogs", () => {
  it("rethrows invoke errors and logs", async () => {
    vi.mocked(commands.requestLogsList).mockRejectedValueOnce(new Error("request logs boom"));

    await expect(requestLogsList("claude", 10)).rejects.toThrow("request logs boom");
    expect(logToConsole).toHaveBeenCalledWith(
      "error",
      "读取请求日志失败",
      expect.objectContaining({
        cmd: "request_logs_list",
        error: expect.stringContaining("request logs boom"),
      })
    );
  });

  it("maps generated DB error envelopes at request-log service boundaries", async () => {
    vi.mocked(commands.requestLogsList).mockResolvedValueOnce({
      status: "error",
      error: "DB_ERROR: database is locked",
    });

    await expect(requestLogsList("claude", 10)).rejects.toThrow("DB_ERROR");
    expect(logToConsole).toHaveBeenCalledWith(
      "error",
      "读取请求日志失败",
      expect.objectContaining({
        cmd: "request_logs_list",
        args: { cliKey: "claude", limit: 10 },
        error: expect.stringContaining("DB_ERROR"),
      })
    );
  });

  it("treats null invoke result as error with runtime", async () => {
    vi.mocked(commands.requestLogsList).mockResolvedValueOnce({
      status: "ok",
      data: null as never,
    });

    await expect(requestLogsList("claude", 10)).rejects.toThrow(
      "IPC_NULL_RESULT: request_logs_list"
    );
  });

  it("passes request logs command args with stable contract fields", async () => {
    vi.mocked(commands.requestLogsList).mockResolvedValueOnce({ status: "ok", data: [] });
    vi.mocked(commands.requestLogsListAll).mockResolvedValueOnce({ status: "ok", data: [] });
    vi.mocked(commands.requestLogsListAfterId).mockResolvedValueOnce({
      status: "ok",
      data: [],
    });
    vi.mocked(commands.requestLogsListAfterIdAll).mockResolvedValueOnce({
      status: "ok",
      data: [],
    });
    vi.mocked(commands.requestLogGet).mockResolvedValueOnce({
      status: "ok",
      data: createRequestLogDetail(),
    });
    vi.mocked(commands.requestLogGetByTraceId).mockResolvedValueOnce({
      status: "ok",
      data: null,
    });
    vi.mocked(commands.requestAttemptLogsByTraceId).mockResolvedValueOnce({
      status: "ok",
      data: [makeRequestAttemptLog()],
    });
    vi.mocked(commands.requestLogsCodexReasoningGuardStats).mockResolvedValueOnce({
      status: "ok",
      data: makeCodexReasoningGuardStats(),
    });

    await requestLogsList("claude", 10);
    await requestLogsListAll(20);
    await requestLogsListAfterId("codex", 5, 30);
    await requestLogsListAfterIdAll(6, 40);
    await requestLogGet(1);
    await requestLogGetByTraceId("t1");
    await requestAttemptLogsByTraceId("t1", 99);
    await requestLogsCodexReasoningGuardStats({
      startCreatedAtMs: 1_770_000_000_000,
      endCreatedAtMs: 1_770_086_400_000,
    });

    expect(commands.requestLogsList).toHaveBeenCalledWith("claude", 10);
    expect(commands.requestLogsListAll).toHaveBeenCalledWith(20);
    expect(commands.requestLogsListAfterId).toHaveBeenCalledWith("codex", 5, 30);
    expect(commands.requestLogsListAfterIdAll).toHaveBeenCalledWith(6, 40);
    expect(commands.requestLogGet).toHaveBeenCalledWith(1);
    expect(commands.requestLogGetByTraceId).toHaveBeenCalledWith("t1");
    expect(commands.requestAttemptLogsByTraceId).toHaveBeenCalledWith("t1", 99);
    expect(commands.requestLogsCodexReasoningGuardStats).toHaveBeenCalledWith(
      1_770_000_000_000,
      1_770_086_400_000
    );
  });

  it("normalizes request log list limits before ipc", async () => {
    vi.mocked(commands.requestLogsList).mockClear();
    vi.mocked(commands.requestLogsListAll).mockClear();
    vi.mocked(commands.requestLogsListAfterId).mockClear();
    vi.mocked(commands.requestLogsListAfterIdAll).mockClear();
    vi.mocked(commands.requestAttemptLogsByTraceId).mockClear();

    vi.mocked(commands.requestLogsList).mockResolvedValueOnce({ status: "ok", data: [] });
    vi.mocked(commands.requestLogsListAll).mockResolvedValueOnce({ status: "ok", data: [] });
    vi.mocked(commands.requestLogsListAfterId).mockResolvedValueOnce({
      status: "ok",
      data: [],
    });
    vi.mocked(commands.requestLogsListAfterIdAll).mockResolvedValueOnce({
      status: "ok",
      data: [],
    });
    vi.mocked(commands.requestAttemptLogsByTraceId).mockResolvedValueOnce({
      status: "ok",
      data: [],
    });

    expect(normalizeRequestLogsLimit(undefined)).toBeNull();
    expect(normalizeRequestLogsLimit(null)).toBeNull();
    expect(normalizeRequestLogsLimit(0)).toBe(REQUEST_LOGS_MIN_LIMIT);
    expect(normalizeRequestLogsLimit(999)).toBe(REQUEST_LOGS_MAX_LIMIT);
    expect(normalizeRequestAttemptLogsLimit(999)).toBe(REQUEST_ATTEMPT_LOGS_MAX_LIMIT);

    await requestLogsList("claude", 0);
    await requestLogsListAll(999);
    await requestLogsListAfterId("codex", 5, 999);
    await requestLogsListAfterIdAll(6, 0);
    await requestAttemptLogsByTraceId("t1", 999);

    expect(commands.requestLogsList).toHaveBeenCalledWith("claude", REQUEST_LOGS_MIN_LIMIT);
    expect(commands.requestLogsListAll).toHaveBeenCalledWith(REQUEST_LOGS_MAX_LIMIT);
    expect(commands.requestLogsListAfterId).toHaveBeenCalledWith(
      "codex",
      5,
      REQUEST_LOGS_MAX_LIMIT
    );
    expect(commands.requestLogsListAfterIdAll).toHaveBeenCalledWith(6, REQUEST_LOGS_MIN_LIMIT);
    expect(commands.requestAttemptLogsByTraceId).toHaveBeenCalledWith(
      "t1",
      REQUEST_ATTEMPT_LOGS_MAX_LIMIT
    );
  });

  it("rejects invalid request log limits before ipc", async () => {
    vi.mocked(commands.requestLogsList).mockClear();
    vi.mocked(commands.requestLogsListAfterIdAll).mockClear();
    vi.mocked(commands.requestAttemptLogsByTraceId).mockClear();

    await expect(requestLogsList("claude", Number.NaN)).rejects.toThrow("SEC_INVALID_INPUT");
    await expect(requestLogsListAfterIdAll(1, 1.5)).rejects.toThrow("SEC_INVALID_INPUT");
    await expect(requestAttemptLogsByTraceId("t1", Number.POSITIVE_INFINITY)).rejects.toThrow(
      "SEC_INVALID_INPUT"
    );

    expect(commands.requestLogsList).not.toHaveBeenCalled();
    expect(commands.requestLogsListAfterIdAll).not.toHaveBeenCalled();
    expect(commands.requestAttemptLogsByTraceId).not.toHaveBeenCalled();
  });

  it("rejects invalid request log ids before ipc", async () => {
    vi.mocked(commands.requestLogsListAfterId).mockClear();
    vi.mocked(commands.requestLogsListAfterIdAll).mockClear();
    vi.mocked(commands.requestLogGet).mockClear();

    expect(normalizeRequestLogId(1)).toBe(1);
    expect(normalizeRequestLogCursorId(0)).toBe(0);
    expect(normalizeRequestLogCursorId(9)).toBe(9);

    await expect(requestLogsListAfterId("claude", -1, 10)).rejects.toThrow("SEC_INVALID_INPUT");
    await expect(requestLogsListAfterIdAll(1.5, 10)).rejects.toThrow("SEC_INVALID_INPUT");
    await expect(requestLogGet(0)).rejects.toThrow("SEC_INVALID_INPUT");
    await expect(requestLogGet(Number.NaN)).rejects.toThrow("SEC_INVALID_INPUT");

    expect(commands.requestLogsListAfterId).not.toHaveBeenCalled();
    expect(commands.requestLogsListAfterIdAll).not.toHaveBeenCalled();
    expect(commands.requestLogGet).not.toHaveBeenCalled();
  });

  it("normalizes trace ids before request-log trace ipc", async () => {
    vi.mocked(commands.requestLogGetByTraceId).mockClear();
    vi.mocked(commands.requestAttemptLogsByTraceId).mockClear();
    vi.mocked(commands.requestLogGetByTraceId).mockResolvedValueOnce({
      status: "ok",
      data: null,
    });
    vi.mocked(commands.requestAttemptLogsByTraceId).mockResolvedValueOnce({
      status: "ok",
      data: [],
    });

    expect(normalizeRequestLogTraceId(" trace-1 ")).toBe("trace-1");

    await requestLogGetByTraceId(" trace-1 ");
    await requestAttemptLogsByTraceId(" trace-2 ", 10);

    expect(commands.requestLogGetByTraceId).toHaveBeenCalledWith("trace-1");
    expect(commands.requestAttemptLogsByTraceId).toHaveBeenCalledWith("trace-2", 10);
  });

  it("rejects invalid trace ids before request-log trace ipc", async () => {
    vi.mocked(commands.requestLogGetByTraceId).mockClear();
    vi.mocked(commands.requestAttemptLogsByTraceId).mockClear();
    const tooLongTraceId = "t".repeat(REQUEST_LOG_TRACE_ID_MAX_LENGTH + 1);

    await expect(requestLogGetByTraceId("   ")).rejects.toThrow("SEC_INVALID_INPUT");
    await expect(requestLogGetByTraceId("trace\n1")).rejects.toThrow("SEC_INVALID_INPUT");
    await expect(requestAttemptLogsByTraceId(tooLongTraceId, 10)).rejects.toThrow(
      "SEC_INVALID_INPUT"
    );

    expect(commands.requestLogGetByTraceId).not.toHaveBeenCalled();
    expect(commands.requestAttemptLogsByTraceId).not.toHaveBeenCalled();
  });

  it("maps non-empty command responses and default limit fallbacks", async () => {
    vi.mocked(commands.requestLogsList).mockResolvedValueOnce({
      status: "ok",
      data: [createRequestLogSummary({ cli_key: "codex" }) as any],
    });
    vi.mocked(commands.requestLogsListAll).mockResolvedValueOnce({
      status: "ok",
      data: [createRequestLogSummary({ cli_key: "gemini" }) as any],
    });
    vi.mocked(commands.requestLogsListAfterId).mockResolvedValueOnce({
      status: "ok",
      data: [createRequestLogSummary({ cli_key: "claude" }) as any],
    });
    vi.mocked(commands.requestLogsListAfterIdAll).mockResolvedValueOnce({
      status: "ok",
      data: [createRequestLogSummary({ cli_key: "codex" }) as any],
    });
    vi.mocked(commands.requestLogGet).mockResolvedValueOnce({
      status: "ok",
      data: createRequestLogDetail({ cli_key: "gemini" }) as any,
    });
    vi.mocked(commands.requestLogGetByTraceId).mockResolvedValueOnce({
      status: "ok",
      data: createRequestLogDetail({ cli_key: "codex" }) as any,
    });
    vi.mocked(commands.requestAttemptLogsByTraceId).mockResolvedValueOnce({
      status: "ok",
      data: [makeRequestAttemptLog({ cli_key: "gemini" }) as any],
    });
    const codexReasoningGuardStats = makeCodexReasoningGuardStats({
      hit_request_count: 11,
      hit_attempt_count: 19,
      token_hit_attempt_count: 17,
      feature_hit_attempt_count: 2,
      reasoning_token_hit_request_count: 10,
      final_answer_only_high_xhigh_hit_request_count: 1,
      normal_request_count: 29,
      total_request_count: 40,
      hit_rate: 0.275,
      feature_sample_request_count: 4,
      feature_sample_count: 6,
      final_answer_only_sample_count: 3,
      high_xhigh_final_answer_only_sample_count: 2,
      reasoning_516_final_answer_only_no_commentary_count: 1,
      compaction_exempt_sample_count: 1,
      reasoning_tokens_coverage_count: 5,
      final_answer_only_coverage_count: 4,
      commentary_observed_coverage_count: 4,
      reasoning_effort_coverage_count: 6,
      duration_ms_coverage_count: 6,
      output_tokens_coverage_count: 2,
      by_model: [
        {
          requested_model: "gpt-5-codex",
          total_request_count: 20,
          hit_request_count: 10,
          normal_request_count: 10,
          hit_attempt_count: 17,
          hit_rate: 0.5,
        },
      ],
      by_model_and_effort: [
        {
          requested_model: "gpt-5-codex",
          reasoning_effort: "high",
          total_request_count: 20,
          hit_request_count: 10,
          normal_request_count: 10,
          hit_attempt_count: 17,
          hit_rate: 0.5,
        },
      ],
    });
    vi.mocked(commands.requestLogsCodexReasoningGuardStats).mockResolvedValueOnce({
      status: "ok",
      data: codexReasoningGuardStats,
    });

    await expect(requestLogsList("codex")).resolves.toEqual([
      expect.objectContaining({ cli_key: "codex" }),
    ]);
    await expect(requestLogsListAll()).resolves.toEqual([
      expect.objectContaining({ cli_key: "gemini" }),
    ]);
    await expect(requestLogsListAfterId("claude", 10)).resolves.toEqual([
      expect.objectContaining({ cli_key: "claude" }),
    ]);
    await expect(requestLogsListAfterIdAll(10)).resolves.toEqual([
      expect.objectContaining({ cli_key: "codex" }),
    ]);
    await expect(requestLogGet(2)).resolves.toEqual(expect.objectContaining({ cli_key: "gemini" }));
    await expect(requestLogGetByTraceId("trace-2")).resolves.toEqual(
      expect.objectContaining({ cli_key: "codex" })
    );
    await expect(requestAttemptLogsByTraceId("trace-2")).resolves.toEqual([
      expect.objectContaining({ cli_key: "gemini" }),
    ]);
    await expect(requestLogsCodexReasoningGuardStats()).resolves.toEqual(codexReasoningGuardStats);

    expect(commands.requestLogsList).toHaveBeenCalledWith("codex", null);
    expect(commands.requestLogsListAll).toHaveBeenCalledWith(null);
    expect(commands.requestLogsListAfterId).toHaveBeenCalledWith("claude", 10, null);
    expect(commands.requestLogsListAfterIdAll).toHaveBeenCalledWith(10, null);
    expect(commands.requestAttemptLogsByTraceId).toHaveBeenCalledWith("trace-2", null);
    expect(commands.requestLogsCodexReasoningGuardStats).toHaveBeenCalledWith(null, null);
  });

  it("rejects invalid Codex reasoning guard stats timestamps", async () => {
    vi.mocked(commands.requestLogsCodexReasoningGuardStats).mockClear();

    await expect(requestLogsCodexReasoningGuardStats({ startCreatedAtMs: 0 })).rejects.toThrow(
      "SEC_INVALID_INPUT: invalid sinceCreatedAtMs=0"
    );
    await expect(requestLogsCodexReasoningGuardStats({ endCreatedAtMs: 1.5 })).rejects.toThrow(
      "SEC_INVALID_INPUT: invalid endCreatedAtMs=1.5"
    );
    await expect(
      requestLogsCodexReasoningGuardStats({
        startCreatedAtMs: 1_770_086_400_000,
        endCreatedAtMs: 1_770_000_000_000,
      })
    ).rejects.toThrow(
      "SEC_INVALID_INPUT: invalid createdAtRange start=1770086400000 end=1770000000000"
    );
    await expect(requestLogsCodexReasoningGuardStats({ startCreatedAtMs: 1.5 })).rejects.toThrow(
      "SEC_INVALID_INPUT: invalid sinceCreatedAtMs=1.5"
    );
    expect(commands.requestLogsCodexReasoningGuardStats).not.toHaveBeenCalled();
  });
});
