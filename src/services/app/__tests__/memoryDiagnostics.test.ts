import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { QueryClient } from "@tanstack/react-query";
import { collectAppMemoryDiagnostics } from "../memoryDiagnostics";
import { queryClient } from "../../../query/queryClient";
import { commands } from "../../../generated/bindings";
import { logToConsole } from "../../consoleLog";

vi.mock("../../../generated/bindings", () => ({
  commands: {
    appMemoryDiagnosticsGet: vi.fn(),
  },
}));

vi.mock("../../consoleLog", async () => {
  const actual = await vi.importActual<typeof import("../../consoleLog")>("../../consoleLog");
  return {
    ...actual,
    logToConsole: vi.fn(),
  };
});

const backendSnapshot = {
  process: {
    resident_set_size: 10,
  },
} as any;

function setMemorySnapshot(memory: {
  usedJSHeapSize?: number;
  totalJSHeapSize?: number;
  jsHeapSizeLimit?: number;
}) {
  Object.defineProperty(performance, "memory", {
    configurable: true,
    value: memory,
  });
}

describe("services/app/memoryDiagnostics", () => {
  beforeEach(() => {
    queryClient.clear();
    vi.mocked(commands.appMemoryDiagnosticsGet).mockResolvedValue({
      status: "ok",
      data: backendSnapshot,
    });
    vi.mocked(logToConsole).mockClear();
    window.history.replaceState(null, "", "/diagnostics?tab=memory");
  });

  afterEach(() => {
    queryClient.clear();
    delete (performance as Performance & { memory?: unknown }).memory;
  });

  it("collects backend snapshot plus sorted frontend query diagnostics", async () => {
    const circular: Record<string, unknown> = { name: "circular" };
    circular.self = circular;
    setMemorySnapshot({
      usedJSHeapSize: 100,
      totalJSHeapSize: 200,
      jsHeapSizeLimit: 300,
    });

    queryClient.setQueryData(["small", 1], "ok");
    queryClient.setQueryData(["large", "payload"], {
      values: Array.from({ length: 12 }, (_, index) => ({
        index,
        label: "x".repeat(20),
      })),
    });
    queryClient.setQueryData(["cycle"], circular);

    const report = await collectAppMemoryDiagnostics();

    expect(report.backend).toBe(backendSnapshot);
    expect(report.frontend.href).toContain("/diagnostics?tab=memory");
    expect(report.frontend.query_count).toBe(3);
    expect(report.frontend.query_estimated_bytes).toBeGreaterThan(0);
    expect(report.frontend.js_heap).toEqual({
      used_js_heap_size: 100,
      total_js_heap_size: 200,
      js_heap_size_limit: 300,
    });

    expect(report.frontend.query_groups.map((group) => group.key)).toContain("large");
    expect(report.frontend.query_groups[0]!.estimated_bytes).toBeGreaterThanOrEqual(
      report.frontend.query_groups[1]!.estimated_bytes
    );
    expect(report.frontend.top_queries[0]!.estimated_bytes).toBeGreaterThanOrEqual(
      report.frontend.top_queries[1]!.estimated_bytes
    );
    expect(report.frontend.top_queries).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          query_key: JSON.stringify(["cycle"]),
          status: "success",
          fetch_status: "idle",
          observers: 0,
          truncated: false,
        }),
      ])
    );
    expect(logToConsole).toHaveBeenCalledWith("info", "内存诊断快照已生成", report);
  });

  it("handles non-array query keys, long keys, missing heap data, and top query limits", async () => {
    const isolatedClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const originalGetQueryCache = queryClient.getQueryCache;
    vi.spyOn(queryClient, "getQueryCache").mockImplementation(() => isolatedClient.getQueryCache());

    for (let index = 0; index < 25; index += 1) {
      isolatedClient.setQueryData(
        index === 0 ? ({ objectKey: true } as unknown as readonly unknown[]) : ["group", index],
        {
          text: "x".repeat(index + 1),
        }
      );
    }
    isolatedClient.setQueryData(["long", "x".repeat(700)], { text: "x".repeat(1_000) });

    const report = await collectAppMemoryDiagnostics();

    expect(report.frontend.query_count).toBe(26);
    expect(report.frontend.top_queries).toHaveLength(20);
    expect(report.frontend.query_groups).toEqual(
      expect.arrayContaining([
        expect.objectContaining({ key: "unknown", count: 1 }),
        expect.objectContaining({ key: "group", count: 24 }),
        expect.objectContaining({ key: "long", count: 1 }),
      ])
    );
    expect(
      report.frontend.top_queries.some((query) => query.query_key.includes("[Truncated]"))
    ).toBe(true);
    expect(report.frontend.js_heap).toBeUndefined();

    queryClient.getQueryCache = originalGetQueryCache;
    isolatedClient.clear();
  });
});
