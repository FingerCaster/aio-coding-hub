import { renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useGatewayCircuitByProviderId } from "../../../query/gateway";
import { useRequestLogsListAllQuery } from "../../../query/requestLogs";
import { buildAvailabilityTimeline } from "../../../components/usage/usageAvailabilityTimeline";
import type { CustomDateRangeApplied } from "../../../hooks/useCustomDateRange";
import type { UsagePeriod } from "../../../services/usage/usage";
import { useUsageAvailabilityData } from "../useUsageAvailabilityData";

vi.mock("../../../query/requestLogs", () => ({
  useRequestLogsListAllQuery: vi.fn(),
}));

vi.mock("../../../query/gateway", () => ({
  useGatewayCircuitByProviderId: vi.fn(),
}));

vi.mock("../../../components/usage/usageAvailabilityTimeline", () => ({
  buildAvailabilityTimeline: vi.fn(() => ({ providers: [] })),
}));

function makeLog(partial: Record<string, unknown>) {
  return {
    id: partial.id ?? 1,
    cli_key: partial.cli_key ?? "claude",
    created_at_ms: partial.created_at_ms ?? Date.now(),
    final_provider_id: partial.final_provider_id ?? 1,
    ...partial,
  } as any;
}

function mockCircuit(cli: string, circuitByProviderId: Record<number, unknown> = {}) {
  return {
    circuitByProviderId,
    refetch: vi.fn(),
    cli,
  };
}

type RangeHookProps = {
  period: UsagePeriod;
  customApplied: CustomDateRangeApplied | null;
};

function lastTimelineRange() {
  const calls = vi.mocked(buildAvailabilityTimeline).mock.calls;
  return calls[calls.length - 1]?.slice(2);
}

describe("pages/usage/useUsageAvailabilityData", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-04-16T12:00:00Z"));
    vi.clearAllMocks();

    vi.mocked(useRequestLogsListAllQuery).mockReturnValue({
      data: null,
      isLoading: false,
      isFetching: false,
      refetch: vi.fn(),
    } as any);
    vi.mocked(useGatewayCircuitByProviderId).mockImplementation(
      (cli: any) => mockCircuit(cli) as any
    );
  });

  it("passes disabled state to query options and returns null data before logs load", () => {
    vi.mocked(useRequestLogsListAllQuery).mockReturnValue({
      data: null,
      isLoading: true,
      isFetching: true,
      refetch: vi.fn(),
    } as any);

    const { result } = renderHook(() =>
      useUsageAvailabilityData({
        enabled: false,
        cliKey: "all",
        providerId: null,
        period: "daily",
        customApplied: null,
      })
    );

    expect(useRequestLogsListAllQuery).toHaveBeenCalledWith(2000, {
      enabled: false,
      refetchIntervalMs: false,
    });
    expect(result.current.data).toBeNull();
    expect(result.current.loading).toBe(false);
    expect(result.current.refreshing).toBe(false);
    expect(buildAvailabilityTimeline).not.toHaveBeenCalled();
  });

  it("filters logs by daily range, cli, provider, and merged circuit map", () => {
    const now = Date.now();
    vi.mocked(useRequestLogsListAllQuery).mockReturnValue({
      data: [
        makeLog({ id: 1, cli_key: "claude", final_provider_id: 9, created_at_ms: now - 1_000 }),
        makeLog({ id: 2, cli_key: "codex", final_provider_id: 9, created_at_ms: now - 1_000 }),
        makeLog({ id: 3, cli_key: "claude", final_provider_id: 10, created_at_ms: now - 1_000 }),
        makeLog({
          id: 4,
          cli_key: "claude",
          final_provider_id: 9,
          created_at_ms: now - 90_000_000,
        }),
      ],
      isLoading: false,
      isFetching: true,
      refetch: vi.fn(),
    } as any);
    const claudeCircuit = mockCircuit("claude", { 9: { provider_id: 9, state: "OPEN" } });
    const codexCircuit = mockCircuit("codex", { 10: { provider_id: 10, state: "CLOSED" } });
    const geminiCircuit = mockCircuit("gemini", { 11: { provider_id: 11, state: "HALF_OPEN" } });
    vi.mocked(useGatewayCircuitByProviderId)
      .mockReturnValueOnce(claudeCircuit as any)
      .mockReturnValueOnce(codexCircuit as any)
      .mockReturnValueOnce(geminiCircuit as any);

    const { result } = renderHook(() =>
      useUsageAvailabilityData({
        enabled: true,
        cliKey: "claude",
        providerId: 9,
        period: "daily",
        customApplied: null,
      })
    );

    expect(useRequestLogsListAllQuery).toHaveBeenCalledWith(2000, {
      enabled: true,
      refetchIntervalMs: 15000,
    });
    expect(buildAvailabilityTimeline).toHaveBeenCalledWith(
      [expect.objectContaining({ id: 1 })],
      {
        9: { provider_id: 9, state: "OPEN" },
        10: { provider_id: 10, state: "CLOSED" },
        11: { provider_id: 11, state: "HALF_OPEN" },
      },
      now - 24 * 60 * 60 * 1000,
      now
    );
    expect(result.current.loading).toBe(false);
    expect(result.current.refreshing).toBe(true);
  });

  it("builds weekly, monthly, all-time, and custom ranges", () => {
    const now = Date.now();
    vi.mocked(useRequestLogsListAllQuery).mockReturnValue({
      data: [makeLog({ id: 1, created_at_ms: now })],
      isLoading: false,
      isFetching: false,
      refetch: vi.fn(),
    } as any);

    const initialRangeProps: RangeHookProps = { period: "weekly", customApplied: null };
    const { rerender } = renderHook(
      ({ period, customApplied }: RangeHookProps) =>
        useUsageAvailabilityData({
          enabled: true,
          cliKey: "all",
          providerId: null,
          period,
          customApplied,
        }),
      { initialProps: initialRangeProps }
    );

    expect(lastTimelineRange()).toEqual([now - 7 * 24 * 60 * 60 * 1000, now]);

    rerender({ period: "monthly", customApplied: null });
    expect(lastTimelineRange()).toEqual([now - 30 * 24 * 60 * 60 * 1000, now]);

    rerender({ period: "allTime", customApplied: null });
    expect(lastTimelineRange()).toEqual([now - 90 * 24 * 60 * 60 * 1000, now]);

    rerender({
      period: "custom",
      customApplied: { startTs: 100, endTs: 200, startDate: "2026-01-01", endDate: "2026-01-02" },
    });
    expect(lastTimelineRange()).toEqual([100_000, 200_000]);

    rerender({ period: "custom", customApplied: null });
    expect(lastTimelineRange()).toEqual([now - 24 * 60 * 60 * 1000, now]);
  });

  it("refetches request logs and all cli circuit maps", () => {
    const logsRefetch = vi.fn();
    const claudeCircuit = mockCircuit("claude");
    const codexCircuit = mockCircuit("codex");
    const geminiCircuit = mockCircuit("gemini");
    vi.mocked(useRequestLogsListAllQuery).mockReturnValue({
      data: [],
      isLoading: true,
      isFetching: true,
      refetch: logsRefetch,
    } as any);
    vi.mocked(useGatewayCircuitByProviderId)
      .mockReturnValueOnce(claudeCircuit as any)
      .mockReturnValueOnce(codexCircuit as any)
      .mockReturnValueOnce(geminiCircuit as any);

    const { result } = renderHook(() =>
      useUsageAvailabilityData({
        enabled: true,
        cliKey: "all",
        providerId: null,
        period: "daily",
        customApplied: null,
      })
    );

    expect(result.current.loading).toBe(true);
    expect(result.current.refreshing).toBe(false);

    result.current.refetch();

    expect(logsRefetch).toHaveBeenCalledTimes(1);
    expect(claudeCircuit.refetch).toHaveBeenCalledTimes(1);
    expect(codexCircuit.refetch).toHaveBeenCalledTimes(1);
    expect(geminiCircuit.refetch).toHaveBeenCalledTimes(1);
  });
});
