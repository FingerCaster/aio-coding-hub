import { fireEvent, render, screen } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import type { ReactElement } from "react";
import { MemoryRouter } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "vitest";
import { LogsPage } from "../LogsPage";
import { createTestQueryClient } from "../../test/utils/reactQuery";
import { clearTauriRuntime, setTauriRuntime } from "../../test/utils/tauriRuntime";
import {
  useRequestAttemptLogsByTraceIdQuery,
  useRequestLogDetailQuery,
  useRequestLogsListAllQuery,
} from "../../query/requestLogs";
import type { TraceSession, TraceSummary } from "../../services/gateway/traceStore";

const traceStoreState = vi.hoisted(() => ({
  traces: [] as TraceSession[],
}));

vi.mock("../../components/home/HomeRequestLogsPanel", () => ({
  HomeRequestLogsPanel: ({
    requestLogs,
    summaryTextOverride,
    emptyStateTitle,
    traces,
  }: {
    requestLogs: Array<{ id: number }>;
    summaryTextOverride?: string;
    emptyStateTitle?: string;
    traces: TraceSession[];
  }) => (
    <div data-testid="home-request-logs-panel">
      count:{requestLogs.length}|summary:{summaryTextOverride ?? ""}|empty:{emptyStateTitle ?? ""}
      <span data-testid="home-request-logs-traces-count">{traces.length}</span>
      <span data-testid="home-request-logs-trace-ids">
        {traces.map((trace) => trace.trace_id).join(",")}
      </span>
    </div>
  ),
}));

vi.mock("../../components/home/RequestLogDetailDialog", () => ({
  RequestLogDetailDialog: () => <div data-testid="request-log-detail-dialog" />,
}));

vi.mock("../../query/requestLogs", async () => {
  const actual =
    await vi.importActual<typeof import("../../query/requestLogs")>("../../query/requestLogs");
  return {
    ...actual,
    useRequestLogsListAllQuery: vi.fn(),
    useRequestLogDetailQuery: vi.fn(),
    useRequestAttemptLogsByTraceIdQuery: vi.fn(),
  };
});

vi.mock("../../services/gateway/traceStore", () => ({
  useTraceStore: () => ({
    traces: traceStoreState.traces,
  }),
}));

function renderWithProviders(element: ReactElement) {
  const client = createTestQueryClient();
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter>{element}</MemoryRouter>
    </QueryClientProvider>
  );
}

type TraceFixture = Omit<Partial<TraceSession>, "trace_id" | "summary"> & {
  trace_id: string;
  summary?: Partial<TraceSummary>;
};

function createTrace({
  trace_id,
  cli_key = "claude",
  method = "POST",
  path = "/v1/messages",
  summary,
  ...overrides
}: TraceFixture): TraceSession {
  const trace: TraceSession = {
    trace_id,
    cli_key,
    method,
    path,
    query: null,
    requested_model: "test-model",
    first_seen_ms: Date.now() - 1000,
    last_seen_ms: Date.now(),
    attempts: [],
    ...overrides,
  };

  if (summary) {
    const traceSummary: TraceSummary = {
      trace_id,
      cli_key,
      method,
      path,
      query: trace.query,
      requested_model: trace.requested_model,
      status: 200,
      error_category: null,
      error_code: null,
      duration_ms: 100,
      attempts: [],
    };
    Object.assign(traceSummary, summary);
    trace.summary = traceSummary;
  }

  return trace;
}

function mockRequestLogQueries(data: unknown[] = []) {
  vi.mocked(useRequestLogsListAllQuery).mockReturnValue({
    data,
    isLoading: false,
    isFetching: false,
    refetch: vi.fn(),
  } as any);
  vi.mocked(useRequestLogDetailQuery).mockReturnValue({ data: null, isFetching: false } as any);
  vi.mocked(useRequestAttemptLogsByTraceIdQuery).mockReturnValue({
    data: [],
    isFetching: false,
  } as any);
}

function traceIds() {
  return screen.getByTestId("home-request-logs-trace-ids");
}

function expectTraceIds(expected: string[]) {
  expect(traceIds().textContent?.trim()).toBe(expected.join(","));
}

describe("pages/LogsPage", () => {
  afterEach(() => {
    traceStoreState.traces = [];
  });

  it("disables filters when not running in tauri runtime", () => {
    clearTauriRuntime();
    traceStoreState.traces = [];

    vi.mocked(useRequestLogsListAllQuery).mockReturnValue({
      data: null,
      isLoading: false,
      isFetching: false,
      refetch: vi.fn(),
    } as any);
    vi.mocked(useRequestLogDetailQuery).mockReturnValue({ data: null, isFetching: false } as any);
    vi.mocked(useRequestAttemptLogsByTraceIdQuery).mockReturnValue({
      data: [],
      isFetching: false,
    } as any);

    renderWithProviders(<LogsPage />);

    expect(screen.getByRole("switch")).toBeDisabled();
    expect(screen.getByPlaceholderText("例：499 / 524 / !200 / >=400")).toBeDisabled();
    expect(screen.getByPlaceholderText("例：GW_UPSTREAM_TIMEOUT")).toBeDisabled();
    expect(screen.getByPlaceholderText("例：/v1/messages")).toBeDisabled();
  });

  it("shows validation error when status filter expression is invalid", () => {
    setTauriRuntime();
    traceStoreState.traces = [];

    vi.mocked(useRequestLogsListAllQuery).mockReturnValue({
      data: [],
      isLoading: false,
      isFetching: false,
      refetch: vi.fn(),
    } as any);
    vi.mocked(useRequestLogDetailQuery).mockReturnValue({ data: null, isFetching: false } as any);
    vi.mocked(useRequestAttemptLogsByTraceIdQuery).mockReturnValue({
      data: [],
      isFetching: false,
    } as any);

    renderWithProviders(<LogsPage />);

    fireEvent.change(screen.getByPlaceholderText("例：499 / 524 / !200 / >=400"), {
      target: { value: "nope" },
    });
    expect(screen.getByText(/表达式不合法/)).toBeInTheDocument();
  });

  it("passes live traces through to the request logs panel", () => {
    setTauriRuntime();
    traceStoreState.traces = [createTrace({ trace_id: "trace-live" })];
    mockRequestLogQueries();

    renderWithProviders(<LogsPage />);

    expect(screen.getByTestId("home-request-logs-traces-count")).toHaveTextContent("1");
    expectTraceIds(["trace-live"]);
  });

  it("filters live traces by selected CLI tab", () => {
    setTauriRuntime();
    traceStoreState.traces = [
      createTrace({ trace_id: "trace-claude", cli_key: "claude" }),
      createTrace({ trace_id: "trace-codex", cli_key: "codex" }),
    ];
    mockRequestLogQueries();

    renderWithProviders(<LogsPage />);

    expectTraceIds(["trace-claude", "trace-codex"]);

    fireEvent.click(screen.getByRole("tab", { name: "Claude" }));
    expectTraceIds(["trace-claude"]);

    fireEvent.click(screen.getByRole("tab", { name: "Codex" }));
    expectTraceIds(["trace-codex"]);

    fireEvent.click(screen.getByRole("tab", { name: "全部" }));
    expectTraceIds(["trace-claude", "trace-codex"]);
  });

  it("filters completed traces by status expression and hides in-progress traces", () => {
    setTauriRuntime();
    traceStoreState.traces = [
      createTrace({ trace_id: "trace-ok", summary: { status: 200 } }),
      createTrace({ trace_id: "trace-timeout", summary: { status: 524 } }),
      createTrace({ trace_id: "trace-live" }),
    ];
    mockRequestLogQueries();

    renderWithProviders(<LogsPage />);

    fireEvent.change(screen.getByPlaceholderText("例：499 / 524 / !200 / >=400"), {
      target: { value: "524" },
    });
    expectTraceIds(["trace-timeout"]);
  });

  it("filters completed traces by error code and hides in-progress traces", () => {
    setTauriRuntime();
    traceStoreState.traces = [
      createTrace({
        trace_id: "trace-timeout",
        summary: { error_code: "GW_UPSTREAM_TIMEOUT" },
      }),
      createTrace({
        trace_id: "trace-aborted",
        summary: { error_code: "GW_ABORTED" },
      }),
      createTrace({ trace_id: "trace-live" }),
    ];
    mockRequestLogQueries();

    renderWithProviders(<LogsPage />);

    fireEvent.change(screen.getByPlaceholderText("例：GW_UPSTREAM_TIMEOUT"), {
      target: { value: "GW_UPSTREAM_TIMEOUT" },
    });
    expectTraceIds(["trace-timeout"]);
  });

  it("filters in-progress traces by path", () => {
    setTauriRuntime();
    traceStoreState.traces = [
      createTrace({ trace_id: "trace-messages", method: "POST", path: "/v1/messages" }),
      createTrace({ trace_id: "trace-health", method: "GET", path: "/health" }),
    ];
    mockRequestLogQueries();

    renderWithProviders(<LogsPage />);

    fireEvent.change(screen.getByPlaceholderText("例：/v1/messages"), {
      target: { value: "/v1/messages" },
    });
    expectTraceIds(["trace-messages"]);
  });

  it("filters logs by status expression", () => {
    setTauriRuntime();
    traceStoreState.traces = [];

    vi.mocked(useRequestLogsListAllQuery).mockReturnValue({
      data: [
        { id: 1, cli_key: "claude", status: 200, error_code: null, method: "GET", path: "/" },
        {
          id: 2,
          cli_key: "claude",
          status: 499,
          error_code: "GW_ABORTED",
          method: "POST",
          path: "/v1",
        },
        {
          id: 3,
          cli_key: "codex",
          status: 524,
          error_code: "GW_TIMEOUT",
          method: "POST",
          path: "/v1/messages",
        },
      ],
      isLoading: false,
      isFetching: false,
      refetch: vi.fn(),
    } as any);
    vi.mocked(useRequestLogDetailQuery).mockReturnValue({ data: null, isFetching: false } as any);
    vi.mocked(useRequestAttemptLogsByTraceIdQuery).mockReturnValue({
      data: [],
      isFetching: false,
    } as any);

    renderWithProviders(<LogsPage />);

    expect(
      screen.getByText("count:3|summary:共 3 / 3 条|empty:当前没有代理记录")
    ).toBeInTheDocument();

    fireEvent.change(screen.getByPlaceholderText("例：499 / 524 / !200 / >=400"), {
      target: { value: "499" },
    });
    expect(
      screen.getByText("count:1|summary:共 1 / 3 条|empty:没有符合筛选条件的代理记录")
    ).toBeInTheDocument();
  });
  it("filters logs by negated status expression (!200)", () => {
    setTauriRuntime();
    traceStoreState.traces = [];
    vi.mocked(useRequestLogsListAllQuery).mockReturnValue({
      data: [
        { id: 1, cli_key: "claude", status: 200, error_code: null, method: "GET", path: "/" },
        { id: 2, cli_key: "claude", status: 499, error_code: null, method: "POST", path: "/v1" },
        { id: 3, cli_key: "claude", status: 524, error_code: null, method: "POST", path: "/v1" },
      ],
      isLoading: false,
      isFetching: false,
      refetch: vi.fn(),
    } as any);
    vi.mocked(useRequestLogDetailQuery).mockReturnValue({ data: null, isFetching: false } as any);
    vi.mocked(useRequestAttemptLogsByTraceIdQuery).mockReturnValue({
      data: [],
      isFetching: false,
    } as any);
    renderWithProviders(<LogsPage />);
    fireEvent.change(screen.getByPlaceholderText("例：499 / 524 / !200 / >=400"), {
      target: { value: "!200" },
    });
    expect(
      screen.getByText("count:2|summary:共 2 / 3 条|empty:没有符合筛选条件的代理记录")
    ).toBeInTheDocument();
  });

  it("filters logs by >=400 status expression", () => {
    setTauriRuntime();
    vi.mocked(useRequestLogsListAllQuery).mockReturnValue({
      data: [
        { id: 1, cli_key: "claude", status: 200, error_code: null, method: "GET", path: "/" },
        { id: 2, cli_key: "claude", status: 400, error_code: null, method: "POST", path: "/v1" },
        { id: 3, cli_key: "claude", status: 524, error_code: null, method: "POST", path: "/v1" },
      ],
      isLoading: false,
      isFetching: false,
      refetch: vi.fn(),
    } as any);
    vi.mocked(useRequestLogDetailQuery).mockReturnValue({ data: null, isFetching: false } as any);
    vi.mocked(useRequestAttemptLogsByTraceIdQuery).mockReturnValue({
      data: [],
      isFetching: false,
    } as any);
    renderWithProviders(<LogsPage />);
    fireEvent.change(screen.getByPlaceholderText("例：499 / 524 / !200 / >=400"), {
      target: { value: ">=400" },
    });
    expect(
      screen.getByText("count:2|summary:共 2 / 3 条|empty:没有符合筛选条件的代理记录")
    ).toBeInTheDocument();
  });

  it("filters logs by <=399 status expression", () => {
    setTauriRuntime();
    vi.mocked(useRequestLogsListAllQuery).mockReturnValue({
      data: [
        { id: 1, cli_key: "claude", status: 200, error_code: null, method: "GET", path: "/" },
        { id: 2, cli_key: "claude", status: 400, error_code: null, method: "POST", path: "/v1" },
      ],
      isLoading: false,
      isFetching: false,
      refetch: vi.fn(),
    } as any);
    vi.mocked(useRequestLogDetailQuery).mockReturnValue({ data: null, isFetching: false } as any);
    vi.mocked(useRequestAttemptLogsByTraceIdQuery).mockReturnValue({
      data: [],
      isFetching: false,
    } as any);
    renderWithProviders(<LogsPage />);
    fireEvent.change(screen.getByPlaceholderText("例：499 / 524 / !200 / >=400"), {
      target: { value: "<=399" },
    });
    expect(
      screen.getByText("count:1|summary:共 1 / 2 条|empty:没有符合筛选条件的代理记录")
    ).toBeInTheDocument();
  });

  it("filters logs by error_code", () => {
    setTauriRuntime();
    vi.mocked(useRequestLogsListAllQuery).mockReturnValue({
      data: [
        { id: 1, cli_key: "claude", status: 200, error_code: null, method: "GET", path: "/" },
        {
          id: 2,
          cli_key: "claude",
          status: 499,
          error_code: "GW_ABORTED",
          method: "POST",
          path: "/v1",
        },
      ],
      isLoading: false,
      isFetching: false,
      refetch: vi.fn(),
    } as any);
    vi.mocked(useRequestLogDetailQuery).mockReturnValue({ data: null, isFetching: false } as any);
    vi.mocked(useRequestAttemptLogsByTraceIdQuery).mockReturnValue({
      data: [],
      isFetching: false,
    } as any);
    renderWithProviders(<LogsPage />);
    fireEvent.change(screen.getByPlaceholderText("例：GW_UPSTREAM_TIMEOUT"), {
      target: { value: "ABORTED" },
    });
    expect(
      screen.getByText("count:1|summary:共 1 / 2 条|empty:没有符合筛选条件的代理记录")
    ).toBeInTheDocument();
  });

  it("filters logs by path", () => {
    setTauriRuntime();
    vi.mocked(useRequestLogsListAllQuery).mockReturnValue({
      data: [
        { id: 1, cli_key: "claude", status: 200, error_code: null, method: "GET", path: "/" },
        {
          id: 2,
          cli_key: "claude",
          status: 200,
          error_code: null,
          method: "POST",
          path: "/v1/messages",
        },
      ],
      isLoading: false,
      isFetching: false,
      refetch: vi.fn(),
    } as any);
    vi.mocked(useRequestLogDetailQuery).mockReturnValue({ data: null, isFetching: false } as any);
    vi.mocked(useRequestAttemptLogsByTraceIdQuery).mockReturnValue({
      data: [],
      isFetching: false,
    } as any);
    renderWithProviders(<LogsPage />);
    fireEvent.change(screen.getByPlaceholderText("例：/v1/messages"), {
      target: { value: "messages" },
    });
    expect(
      screen.getByText("count:1|summary:共 1 / 2 条|empty:没有符合筛选条件的代理记录")
    ).toBeInTheDocument();
  });
});
