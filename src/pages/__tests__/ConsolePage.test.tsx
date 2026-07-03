import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { MemoryRouter } from "react-router-dom";
import { describe, expect, it, vi } from "vitest";
import type { ReactElement } from "react";
import { toast } from "sonner";
import { createTestQueryClient } from "../../test/utils/reactQuery";
import { ConsolePage } from "../ConsolePage";
import { collectAppMemoryDiagnostics } from "../../services/app/memoryDiagnostics";
import { gatewayEventNames } from "../../constants/gatewayEvents";
import {
  clearConsoleLogs,
  formatConsoleLogDetails,
  formatConsoleLogDetailsSmart,
  getConsoleDebugEnabled,
  setConsoleDebugEnabled,
  useConsoleLogs,
} from "../../services/consoleLog";

vi.mock("sonner", () => ({ toast: vi.fn() }));
vi.mock("../../services/app/memoryDiagnostics", () => ({
  collectAppMemoryDiagnostics: vi.fn(),
}));
vi.mock("../../services/consoleLog", async () => {
  const actual = await vi.importActual<typeof import("../../services/consoleLog")>(
    "../../services/consoleLog"
  );
  return {
    ...actual,
    useConsoleLogs: vi.fn(),
    clearConsoleLogs: vi.fn(),
    formatConsoleLogDetails: vi.fn(),
    formatConsoleLogDetailsSmart: vi.fn(),
    getConsoleDebugEnabled: vi.fn(),
    setConsoleDebugEnabled: vi.fn(),
  };
});

// Mock useVirtualizer so all items render in jsdom (no layout engine)
vi.mock("@tanstack/react-virtual", () => ({
  useVirtualizer: ({ count }: { count: number }) => {
    const items = Array.from({ length: count }, (_, i) => ({
      index: i,
      key: String(i),
      start: i * 48,
      size: 48,
      end: (i + 1) * 48,
    }));
    return {
      getVirtualItems: () => items,
      getTotalSize: () => count * 48,
      measureElement: () => {},
      scrollToIndex: () => {},
    };
  },
}));

function renderWithProviders(element: ReactElement) {
  const client = createTestQueryClient();
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter>{element}</MemoryRouter>
    </QueryClientProvider>
  );
}

describe("pages/ConsolePage", () => {
  it("supports filtering, toggles, clear, and expands details", async () => {
    vi.mocked(getConsoleDebugEnabled).mockReturnValue(false);

    const logs: any[] = [];
    for (let i = 0; i < 201; i += 1) {
      logs.push({ id: i + 1, tsText: "00:00:00", level: "info", title: `L${i + 1}` });
    }
    logs.push({ id: 1000, tsText: "00:00:01", level: "debug", title: "DEBUG-1" });
    logs.push({
      id: 2000,
      tsText: "00:00:02",
      level: "error",
      title: "DETAIL-LOG",
      details: { kind: "x" },
    });

    vi.mocked(useConsoleLogs).mockReturnValue(logs as any);
    vi.mocked(formatConsoleLogDetails).mockReturnValue("FORMATTED");

    renderWithProviders(<ConsolePage />);

    expect(screen.getByRole("heading", { level: 1, name: "控制台" })).toBeInTheDocument();
    expect(screen.getByText("已隐藏 1 条日志")).toBeInTheDocument();

    // With virtualization, all visible logs are rendered (no "show all" button needed).
    // The badge should show the total visible count (202 = 203 total - 1 debug).
    expect(screen.getByText("202")).toBeInTheDocument();

    // Toggle debug switch (second switch)
    const switches = screen.getAllByRole("switch");
    fireEvent.click(switches[1]);
    expect(setConsoleDebugEnabled).toHaveBeenCalledWith(true);
    expect(toast).toHaveBeenCalledWith("已开启调试日志");

    // Clear logs
    fireEvent.click(screen.getByRole("button", { name: "清空日志" }));
    expect(clearConsoleLogs).toHaveBeenCalled();
    expect(toast).toHaveBeenCalledWith("已清空控制台日志");

    // Expand details row
    const details = screen.getByText("DETAIL-LOG").closest("details") as HTMLDetailsElement | null;
    expect(details).not.toBeNull();
    (details as HTMLDetailsElement).open = true;
    fireEvent(details as HTMLDetailsElement, new Event("toggle"));

    await waitFor(() => expect(formatConsoleLogDetails).toHaveBeenCalled());
    expect(screen.getByText("FORMATTED")).toBeInTheDocument();
  });

  it("filters by metadata, copies trace IDs, toggles levels, and runs diagnostics", async () => {
    vi.mocked(getConsoleDebugEnabled).mockReturnValue(true);
    vi.mocked(formatConsoleLogDetailsSmart).mockReturnValue("");
    vi.mocked(formatConsoleLogDetails).mockReturnValue("");
    vi.mocked(collectAppMemoryDiagnostics)
      .mockResolvedValueOnce({} as any)
      .mockRejectedValueOnce(new Error("diag down"));

    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.assign(navigator, { clipboard: { writeText } });

    vi.mocked(useConsoleLogs).mockReturnValue([
      {
        id: "1",
        ts: 1,
        tsText: "00:00:01",
        level: "warn",
        title: "Circuit opened",
        eventType: gatewayEventNames.circuit,
        meta: {
          trace_id: "TRACE-123456789",
          cli_key: "claude",
          error_code: "E_PROVIDER_DOWN",
          providers: ["Provider A", "Provider B"],
        },
        details: { empty: true },
      },
      {
        id: "2",
        ts: 2,
        tsText: "00:00:02",
        level: "info",
        title: "Circuit recovered",
        eventType: gatewayEventNames.circuit,
        meta: { providers: [] },
      },
      {
        id: "3",
        ts: 3,
        tsText: "00:00:03",
        level: "error",
        title: "Fatal issue",
        details: null,
      },
      {
        id: "4",
        ts: 4,
        tsText: "00:00:04",
        level: "debug",
        title: "Debug detail",
      },
    ] as any);

    renderWithProviders(<ConsolePage />);

    expect(screen.getByText("WARN")).toBeInTheDocument();
    expect(screen.getByText("INFO")).toBeInTheDocument();
    expect(screen.getByText("ERROR")).toBeInTheDocument();
    expect(screen.getByText("DEBUG")).toBeInTheDocument();

    fireEvent.click(screen.getByTitle("点击复制 Trace ID"));
    await waitFor(() => expect(writeText).toHaveBeenCalledWith("TRACE-123456789"));
    await waitFor(() => expect(toast).toHaveBeenCalledWith("已复制到剪贴板"));

    fireEvent.click(screen.getByRole("button", { name: "过滤" }));
    fireEvent.change(screen.getByRole("textbox", { name: "搜索控制台日志" }), {
      target: { value: "provider b" },
    });
    expect(screen.getByText("Circuit opened")).toBeInTheDocument();
    expect(screen.queryByText("Circuit recovered")).not.toBeInTheDocument();

    fireEvent.change(screen.getByRole("textbox", { name: "搜索控制台日志" }), {
      target: { value: "E_PROVIDER_DOWN" },
    });
    expect(screen.getByText("Circuit opened")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "WARN" }));
    expect(screen.getByText("暂无可显示的日志")).toBeInTheDocument();
    expect(screen.getByText("调整过滤器以查看更多日志")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "WARN" }));
    const details = screen.getByText("Circuit opened").closest("details") as HTMLDetailsElement;
    details.open = true;
    fireEvent(details, new Event("toggle"));
    expect(await screen.findByText("// 无可显示的详情")).toBeInTheDocument();
    expect(screen.getByText("// 无原始数据")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "内存诊断" }));
    await waitFor(() => expect(collectAppMemoryDiagnostics).toHaveBeenCalledTimes(1));
    expect(toast).toHaveBeenCalledWith("已生成内存诊断日志");

    fireEvent.click(screen.getByRole("button", { name: "内存诊断" }));
    await waitFor(() => expect(collectAppMemoryDiagnostics).toHaveBeenCalledTimes(2));
    expect(toast).toHaveBeenCalledWith("内存诊断失败，请查看错误日志");

    fireEvent.click(screen.getAllByRole("switch")[0]!);
    expect(screen.getByText("自动滚动")).toBeInTheDocument();
  });
});
