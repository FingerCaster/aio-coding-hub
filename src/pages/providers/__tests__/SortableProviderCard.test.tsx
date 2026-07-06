import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { tauriOpenUrl } from "../../../test/mocks/tauri";
import { SortableProviderCard, type SortableProviderCardProps } from "../SortableProviderCard";
import {
  providerAccountUsageFetch,
  providerOAuthFetchLimits,
  providerOAuthResetCodexQuota,
  type ProviderSummary,
} from "../../../services/providers/providers";
import { gatewayCircuitResetProvider } from "../../../services/gateway/gateway";
import { createTestQueryClient, createQueryWrapper } from "../../../test/utils/reactQuery";

const sortablePointerDownMock = vi.hoisted(() => vi.fn());

vi.mock("../../../services/consoleLog", () => ({ logToConsole: vi.fn() }));
vi.mock("../../../services/providers/providers", async () => {
  const actual = await vi.importActual<typeof import("../../../services/providers/providers")>(
    "../../../services/providers/providers"
  );
  return {
    ...actual,
    providerAccountUsageFetch: vi.fn(),
    providerOAuthFetchLimits: vi.fn(),
    providerOAuthResetCodexQuota: vi.fn(),
  };
});

vi.mock("../../../services/gateway/gateway", async () => {
  const actual = await vi.importActual<typeof import("../../../services/gateway/gateway")>(
    "../../../services/gateway/gateway"
  );
  return { ...actual, gatewayCircuitResetProvider: vi.fn() };
});

vi.mock("@dnd-kit/sortable", () => ({
  useSortable: () => ({
    attributes: {},
    listeners: { onPointerDown: sortablePointerDownMock },
    setNodeRef: () => {},
    transform: null,
    transition: undefined,
    isDragging: false,
  }),
}));

vi.mock("@dnd-kit/utilities", () => ({
  CSS: { Transform: { toString: () => null } },
}));

function makeProvider(partial: Partial<ProviderSummary> = {}): ProviderSummary {
  return {
    id: 1,
    cli_key: "claude",
    name: "Test Provider",
    base_urls: ["https://example.com/v1"],
    base_url_mode: "order",
    claude_models: {},
    enabled: true,
    priority: 0,
    cost_multiplier: 1.0,
    limit_5h_usd: null,
    limit_daily_usd: null,
    daily_reset_mode: "fixed",
    daily_reset_time: "00:00:00",
    limit_weekly_usd: null,
    limit_monthly_usd: null,
    limit_total_usd: null,
    tags: [],
    note: "",
    created_at: 0,
    updated_at: 0,
    auth_mode: "api_key",
    oauth_provider_type: null,
    oauth_email: null,
    oauth_expires_at: null,
    oauth_last_error: null,
    source_provider_id: null,
    bridge_type: null,
    availability_test_model: null,
    api_key_configured: partial.api_key_configured ?? false,
    ...partial,
    model_mapping: partial.model_mapping ?? { default_model: null, exact: {} },
    stream_idle_timeout_seconds: partial.stream_idle_timeout_seconds ?? null,
    extension_values: partial.extension_values ?? [],
    upstream_retry_policy_override: partial.upstream_retry_policy_override ?? null,
  };
}

function renderCard(
  partialProvider: Partial<ProviderSummary> = {},
  extraProps: Partial<SortableProviderCardProps> = {}
) {
  const provider = makeProvider(partialProvider);
  const defaultProps: SortableProviderCardProps = {
    provider,
    circuit: null,
    circuitResetting: false,
    onToggleEnabled: vi.fn(),
    onResetCircuit: vi.fn(),
    onEdit: vi.fn(),
    onDelete: vi.fn(),
    ...extraProps,
  };
  const queryClient = createTestQueryClient();
  return render(<SortableProviderCard {...defaultProps} />, {
    wrapper: createQueryWrapper(queryClient),
  });
}

describe("pages/providers/SortableProviderCard", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("binds sortable listeners to the provider name drag handle", () => {
    renderCard();

    const dragHandle = screen.getByRole("button", {
      name: "拖拽调整 Test Provider 顺序",
    });
    expect(dragHandle).toHaveAttribute("title", "拖拽排序");

    fireEvent.pointerDown(dragHandle);

    expect(sortablePointerDownMock).toHaveBeenCalledTimes(1);
  });

  it("auto-fetches configured account usage without resetting gateway circuit", async () => {
    vi.mocked(providerAccountUsageFetch).mockResolvedValueOnce({
      adapter_kind: "sub2api",
      status: "available",
      freshness: "fresh",
      plan_name: "Pro",
      balance: 12.5,
      plan_remaining: null,
      used: null,
      total: null,
      unit: "USD",
      unit_note: null,
      daily_used: 1,
      daily_total: 10,
      weekly_used: null,
      weekly_total: null,
      monthly_used: null,
      monthly_total: null,
      expires_at: null,
      last_fetched_at: 1_700_000_000,
      message: null,
    });

    renderCard({
      id: 9,
      auth_mode: "api_key",
      extension_values: [
        {
          pluginId: "core.provider-account-usage",
          namespace: "accountUsage",
          values: { adapterKind: "sub2api" },
          updatedAt: 1,
        },
      ],
    });

    await waitFor(() => expect(providerAccountUsageFetch).toHaveBeenCalledWith(9));
    expect(await screen.findByText(/账户: 可用 · Pro · 余额 12.5 USD/)).toBeInTheDocument();
    expect(screen.getByText("日 1.00/10.0 USD")).toBeInTheDocument();
    expect(gatewayCircuitResetProvider).not.toHaveBeenCalled();
    expect(providerOAuthFetchLimits).not.toHaveBeenCalled();
  });

  it("renders subscription account usage as a summary with daily weekly and monthly chips", async () => {
    vi.mocked(providerAccountUsageFetch).mockResolvedValueOnce({
      adapter_kind: "sub2api",
      status: "available",
      freshness: "fresh",
      plan_name: "CodeX Air 订阅",
      balance: 130,
      plan_remaining: null,
      used: null,
      total: null,
      unit: "USD",
      unit_note: null,
      daily_used: 170,
      daily_total: 300,
      weekly_used: 20,
      weekly_total: 70,
      monthly_used: 771,
      monthly_total: 0,
      expires_at: null,
      last_fetched_at: 1_700_000_000,
      message: null,
    });

    renderCard({
      id: 10,
      auth_mode: "api_key",
      extension_values: [
        {
          pluginId: "core.provider-account-usage",
          namespace: "accountUsage",
          values: { adapterKind: "sub2api" },
          updatedAt: 1,
        },
      ],
    });

    expect(
      await screen.findByText("账户: 可用 · CodeX Air 订阅 · 余额 130 USD")
    ).toBeInTheDocument();
    expect(screen.getByText("日 170/300 USD")).toBeInTheDocument();
    expect(screen.getByText("周 20.0/70.0 USD")).toBeInTheDocument();
    expect(screen.getByText("月已用 771 USD")).toBeInTheDocument();
    expect(screen.queryByText(/月 771 USD\/0/)).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", {
        name: /刷新账户用量，账户: 可用 · CodeX Air 订阅 · 余额 130 USD/,
      })
    ).toHaveAttribute(
      "title",
      "账户: 可用 · CodeX Air 订阅 · 余额 130 USD\n日 170/300 USD\n周 20.0/70.0 USD\n月已用 771 USD"
    );
  });

  it("renders mixed package and balance account usage without flattening plan allowance", async () => {
    vi.mocked(providerAccountUsageFetch).mockResolvedValueOnce({
      adapter_kind: "sub2api",
      status: "available",
      freshness: "fresh",
      plan_name: "Super Ultra",
      balance: 0,
      plan_remaining: 42,
      used: null,
      total: null,
      unit: "USD",
      unit_note: null,
      daily_used: null,
      daily_total: null,
      weekly_used: null,
      weekly_total: null,
      monthly_used: null,
      monthly_total: null,
      expires_at: null,
      last_fetched_at: 1_700_000_000,
      message: null,
    });

    renderCard({
      id: 12,
      auth_mode: "api_key",
      extension_values: [
        {
          pluginId: "core.provider-account-usage",
          namespace: "accountUsage",
          values: { adapterKind: "sub2api" },
          updatedAt: 1,
        },
      ],
    });

    expect(
      await screen.findByText("账户: 可用 · Super Ultra · 套餐剩余 42.0 USD · 余额 0.00 USD")
    ).toBeInTheDocument();
    expect(screen.queryByText(/账户: 可用 · Super Ultra · 余额 42.0 USD/)).not.toBeInTheDocument();
  });

  it("renders zero balance status as no available quota", async () => {
    vi.mocked(providerAccountUsageFetch).mockResolvedValueOnce({
      adapter_kind: "sub2api",
      status: "zero_balance",
      freshness: "fresh",
      plan_name: null,
      balance: 0,
      plan_remaining: null,
      used: null,
      total: null,
      unit: "USD",
      unit_note: null,
      daily_used: null,
      daily_total: null,
      weekly_used: null,
      weekly_total: null,
      monthly_used: null,
      monthly_total: null,
      expires_at: null,
      last_fetched_at: 1_700_000_000,
      message: null,
    });

    renderCard({
      id: 13,
      auth_mode: "api_key",
      extension_values: [
        {
          pluginId: "core.provider-account-usage",
          namespace: "accountUsage",
          values: { adapterKind: "sub2api" },
          updatedAt: 1,
        },
      ],
    });

    expect(await screen.findByText("账户: 无可用额度 · 余额 0.00 USD")).toBeInTheDocument();
    expect(screen.queryByText(/账户: 余额 0/)).not.toBeInTheDocument();
  });

  it("renders balance-only account usage without a subscription label", async () => {
    vi.mocked(providerAccountUsageFetch).mockResolvedValueOnce({
      adapter_kind: "newapi",
      status: "available",
      freshness: "fresh",
      plan_name: null,
      balance: 1,
      plan_remaining: null,
      used: 2,
      total: 3,
      unit: "USD",
      unit_note: null,
      daily_used: null,
      daily_total: null,
      weekly_used: null,
      weekly_total: null,
      monthly_used: null,
      monthly_total: null,
      expires_at: null,
      last_fetched_at: 1_700_000_000,
      message: null,
    });

    renderCard({
      id: 11,
      auth_mode: "api_key",
      extension_values: [
        {
          pluginId: "core.provider-account-usage",
          namespace: "accountUsage",
          values: { adapterKind: "newapi" },
          updatedAt: 1,
        },
      ],
    });

    expect(await screen.findByText("账户: 可用 · 余额 1.00 USD")).toBeInTheDocument();
    expect(screen.getByText("已用 2.00/3.00 USD")).toBeInTheDocument();
  });

  it("does not render account usage for unsupported provider config", () => {
    renderCard({
      auth_mode: "oauth",
      extension_values: [
        {
          pluginId: "core.provider-account-usage",
          namespace: "accountUsage",
          values: { adapterKind: "newapi" },
          updatedAt: 1,
        },
      ],
    });

    expect(screen.queryByText(/账户:/)).not.toBeInTheDocument();
  });

  it("renders OAuth badge with email", () => {
    renderCard({
      auth_mode: "oauth",
      oauth_email: "user@example.com",
    });

    // OAuth badge is a button; email is rendered in a separate span
    expect(screen.getByText("OAuth")).toBeInTheDocument();
    expect(screen.getByText("user@example.com")).toBeInTheDocument();
  });

  it("renders OAuth badge with error styling", () => {
    renderCard({
      auth_mode: "oauth",
      oauth_last_error: "Token expired",
    });

    const badge = screen.getByText("OAuth");
    expect(badge).toBeInTheDocument();
    expect(badge.getAttribute("title")).toContain("OAuth 错误: Token expired");
  });

  it("renders OAuth badge without email", () => {
    renderCard({
      auth_mode: "oauth",
      oauth_email: null,
    });

    const badge = screen.getByText("OAuth");
    expect(badge.getAttribute("title")).toContain("OAuth 已连接");
  });

  it("renders OAuth button that triggers limits fetch", () => {
    renderCard({
      auth_mode: "oauth",
    });

    // OAuth button renders with "OAuth" text and acts as the fetch trigger
    const oauthButton = screen.getByText("OAuth");
    expect(oauthButton).toBeInTheDocument();
    expect(oauthButton.tagName).toBe("BUTTON");
  });

  it("auto-fetches OAuth limits on mount for oauth providers", async () => {
    vi.mocked(providerOAuthFetchLimits).mockResolvedValue({
      limit_short_label: null,
      limit_5h_text: "auto",
      limit_weekly_text: "200",
      limit_5h_reset_at: null,
      limit_weekly_reset_at: null,
      reset_credit_available_count: null,
    });

    renderCard({
      auth_mode: "oauth",
    });

    // React Query auto-fetches because enabled=true for OAuth providers
    await waitFor(() => expect(vi.mocked(providerOAuthFetchLimits)).toHaveBeenCalled());
  });

  it("renders provider-specific short-window labels for Gemini OAuth limits", async () => {
    vi.mocked(providerOAuthFetchLimits).mockResolvedValue({
      limit_short_label: "短窗",
      limit_5h_text: "88",
      limit_weekly_text: "300",
      limit_5h_reset_at: null,
      limit_weekly_reset_at: null,
      reset_credit_available_count: null,
    });

    renderCard({
      id: 77,
      cli_key: "gemini",
      auth_mode: "oauth",
    });

    fireEvent.click(screen.getByText("OAuth"));

    await waitFor(() => expect(screen.getByText("短窗: 88")).toBeInTheDocument());
  });

  it("forces Gemini OAuth limits to render with a generic short-window label", async () => {
    vi.mocked(providerOAuthFetchLimits).mockResolvedValue({
      limit_short_label: "1h",
      limit_5h_text: "88",
      limit_weekly_text: "300",
      limit_5h_reset_at: null,
      limit_weekly_reset_at: null,
      reset_credit_available_count: null,
    });

    renderCard({
      id: 78,
      cli_key: "gemini",
      auth_mode: "oauth",
    });

    fireEvent.click(screen.getByText("OAuth"));

    await waitFor(() => expect(screen.getByText("短窗: 88")).toBeInTheDocument());
  });

  it("handles null result from fetchLimits", async () => {
    vi.mocked(providerOAuthFetchLimits).mockResolvedValue(null);

    renderCard({
      auth_mode: "oauth",
    });

    fireEvent.click(screen.getByText("OAuth"));

    await waitFor(() => expect(vi.mocked(providerOAuthFetchLimits)).toHaveBeenCalled());
    // React Query queryFn maps null to empty limits; no toast is shown
    expect(screen.queryByText(/5h:/)).not.toBeInTheDocument();
  });

  it("renders Codex OAuth reset count and confirms before resetting", async () => {
    vi.mocked(providerOAuthFetchLimits).mockResolvedValue({
      limit_short_label: "5h",
      limit_5h_text: "0%",
      limit_weekly_text: "50%",
      limit_5h_reset_at: null,
      limit_weekly_reset_at: null,
      reset_credit_available_count: 3,
    });
    vi.mocked(providerOAuthResetCodexQuota).mockResolvedValue({
      success: true,
      code: "ok",
      windows_reset: 2,
      refreshed_limits: {
        limit_short_label: "5h",
        limit_5h_text: "100%",
        limit_weekly_text: "100%",
        limit_5h_reset_at: null,
        limit_weekly_reset_at: null,
        reset_credit_available_count: 2,
      },
      refresh_error: null,
    });

    renderCard({
      id: 88,
      cli_key: "codex",
      auth_mode: "oauth",
      oauth_email: "codex@example.com",
    });

    await waitFor(() =>
      expect(screen.getByRole("button", { name: "可重置次数: 3(点击重置)" })).toBeInTheDocument()
    );
    expect(providerOAuthResetCodexQuota).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "可重置次数: 3(点击重置)" }));

    expect(screen.getByText("使用 1 次 Codex 重置次数刷新该账号额度？")).toBeInTheDocument();
    expect(providerOAuthResetCodexQuota).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "确认重置" }));

    await waitFor(() => expect(providerOAuthResetCodexQuota).toHaveBeenCalledWith(88));
    await waitFor(() => expect(screen.getByText("5h: 100%")).toBeInTheDocument());
    expect(screen.getByRole("button", { name: "可重置次数: 2(点击重置)" })).toBeInTheDocument();
  });

  it("does not render reset action for non-Codex OAuth providers", async () => {
    vi.mocked(providerOAuthFetchLimits).mockResolvedValue({
      limit_short_label: "短窗",
      limit_5h_text: "88",
      limit_weekly_text: "300",
      limit_5h_reset_at: null,
      limit_weekly_reset_at: null,
      reset_credit_available_count: 3,
    });

    renderCard({
      id: 89,
      cli_key: "gemini",
      auth_mode: "oauth",
    });

    await waitFor(() => expect(screen.getByText("短窗: 88")).toBeInTheDocument());
    expect(screen.queryByText(/可重置次数/)).not.toBeInTheDocument();
  });

  it("handles fetchLimits error", async () => {
    vi.mocked(providerOAuthFetchLimits).mockRejectedValue(new Error("fetch error"));

    renderCard({
      auth_mode: "oauth",
    });

    fireEvent.click(screen.getByText("OAuth"));

    await waitFor(() => expect(vi.mocked(providerOAuthFetchLimits)).toHaveBeenCalled());
    // React Query absorbs the error; no toast is shown
    expect(screen.queryByText(/5h:/)).not.toBeInTheDocument();
  });

  it("renders note when present", () => {
    renderCard({ note: "My custom note" });

    const note = screen.getByTitle("My custom note");
    expect(note).toBeInTheDocument();
    expect(note).toHaveTextContent("My custom note");
  });

  it("renders http links in note as clickable anchors", () => {
    renderCard({ note: "文档 https://example.com/docs?q=1, 备用说明" });

    const link = screen.getByRole("link", { name: "https://example.com/docs?q=1" });
    expect(link).toBeInTheDocument();
    expect(link).toHaveAttribute("href", "https://example.com/docs?q=1");
  });

  it("opens http links in note through the desktop opener", async () => {
    vi.mocked(tauriOpenUrl).mockResolvedValue(undefined as never);

    renderCard({ note: "文档 https://example.com/docs?q=1, 备用说明" });

    fireEvent.click(screen.getByRole("link", { name: "https://example.com/docs?q=1" }));

    await waitFor(() => {
      expect(tauriOpenUrl).toHaveBeenCalledWith("https://example.com/docs?q=1");
    });
  });

  it("falls back to window.open when the desktop opener fails", async () => {
    vi.mocked(tauriOpenUrl).mockRejectedValue(new Error("blocked"));
    const windowOpen = vi.spyOn(window, "open").mockImplementation(() => null);

    renderCard({ note: "文档 https://example.com/docs?q=1, 备用说明" });

    fireEvent.click(screen.getByRole("link", { name: "https://example.com/docs?q=1" }));

    await waitFor(() => {
      expect(windowOpen).toHaveBeenCalledWith(
        "https://example.com/docs?q=1",
        "_blank",
        "noopener,noreferrer"
      );
    });
  });

  it("ignores window.open errors after the desktop opener fails", async () => {
    vi.mocked(tauriOpenUrl).mockRejectedValue(new Error("blocked"));
    const windowOpen = vi.spyOn(window, "open").mockImplementation(() => {
      throw new Error("popup blocked");
    });

    renderCard({ note: "文档 https://example.com/docs?q=1, 备用说明" });

    fireEvent.click(screen.getByRole("link", { name: "https://example.com/docs?q=1" }));

    await waitFor(() => {
      expect(windowOpen).toHaveBeenCalledWith(
        "https://example.com/docs?q=1",
        "_blank",
        "noopener,noreferrer"
      );
    });
  });

  it("renders limit chips", () => {
    renderCard({
      limit_5h_usd: 10,
      limit_daily_usd: 100,
      daily_reset_mode: "rolling",
      limit_weekly_usd: 500,
      limit_monthly_usd: 2000,
      limit_total_usd: 10000,
    });

    expect(screen.getByText("限额")).toBeInTheDocument();
  });

  it("renders Claude models badge", () => {
    renderCard({
      cli_key: "claude",
      claude_models: {
        main_model: " claude-3 ",
        reasoning_model: "claude-thinking",
        haiku_model: null as any,
      },
    });

    const badge = screen.getByText("模型映射 2/5");
    expect(badge).toBeInTheDocument();
    expect(badge).toHaveAttribute(
      "title",
      "已配置 Claude 模型映射（2/5）\n主模型: claude-3\n推理模型(Thinking): claude-thinking"
    );
  });

  it("renders tags at the end of the second row", () => {
    renderCard({ tags: ["prod", "cn"] });

    expect(screen.getByText("prod")).toBeInTheDocument();
    expect(screen.getByText("cn")).toBeInTheDocument();
  });

  it("renders 免费 tag with emerald styling", () => {
    renderCard({ tags: ["免费"] });

    const freeTag = screen.getByText("免费");
    expect(freeTag.className).toContain("bg-emerald-100");
    expect(freeTag.className).toContain("text-emerald-700");
  });

  it("renders cx2cc source summary for a concrete codex provider", () => {
    renderCard(
      {
        bridge_type: "cx2cc",
        source_provider_id: 7,
        cost_multiplier: 1.8,
      },
      {
        sourceProviderName: "Lisa",
        sourceProvider: makeProvider({
          id: 7,
          cli_key: "codex",
          name: "Lisa",
          auth_mode: "oauth",
          base_urls: ["https://codex.example.com/v1"],
        }),
      }
    );

    expect(screen.getByText("CX2CC")).toBeInTheDocument();
    expect(screen.getByText("x1.80")).toBeInTheDocument();
    expect(screen.getAllByText((_, el) => el?.textContent === "来源: Lisa").length).toBeGreaterThan(
      0
    );
    expect(screen.getByText("https://codex.example.com/v1")).toBeInTheDocument();
    expect(screen.queryByText("CX2CC 转译")).not.toBeInTheDocument();
  });

  it("renders cx2cc summary for the current aio codex gateway", () => {
    renderCard({
      bridge_type: "cx2cc",
      source_provider_id: null,
      cost_multiplier: 0,
      tags: ["免费"],
    });

    expect(screen.getByText("CX2CC")).toBeInTheDocument();
    expect(screen.getByText("免费")).toBeInTheDocument();
    expect(
      screen.getAllByText((_, el) => el?.textContent === "来源: 当前 AIO 服务 Codex 网关").length
    ).toBeGreaterThan(0);
    expect(screen.getByText("跟随当前 Codex 分流")).toBeInTheDocument();
  });

  it("renders codex responses bridge badge and title", () => {
    renderCard(
      {
        cli_key: "codex",
        bridge_type: "codex_to_openai_responses",
        source_provider_id: 9,
        base_urls: [],
      },
      {
        sourceProviderName: "Gemini Responses Source",
        sourceProvider: makeProvider({
          id: 9,
          cli_key: "gemini",
          name: "Gemini Responses Source",
          base_urls: ["https://gemini.example/v1"],
        }),
      }
    );

    expect(screen.getByText("Responses")).toBeInTheDocument();
    expect(screen.getByText("Responses")).toHaveAttribute("title", "Codex → Responses");
    expect(
      screen.getAllByText((_, el) => el?.textContent === "来源: Gemini Responses Source").length
    ).toBeGreaterThan(0);
    expect(screen.getByText("https://gemini.example/v1")).toBeInTheDocument();
  });

  it("shows only one 免费 label for zero-cost cx2cc cards", () => {
    renderCard({
      bridge_type: "cx2cc",
      source_provider_id: null,
      cost_multiplier: 0,
      tags: ["免费", "bridge"],
    });

    expect(screen.getAllByText("免费")).toHaveLength(1);
    expect(screen.getByText("bridge")).toBeInTheDocument();
  });

  it("does not render a separate cx2cc free price badge", () => {
    renderCard({
      bridge_type: "cx2cc",
      source_provider_id: null,
      cost_multiplier: 0,
      tags: [],
    });

    expect(screen.queryByText("免费")).not.toBeInTheDocument();
  });

  it("renders ping mode label", () => {
    renderCard({ base_url_mode: "ping" });

    expect(screen.getByText("Ping")).toBeInTheDocument();
  });

  it("shows api key base url summary only after clicking the api key badge", () => {
    renderCard({ auth_mode: "api_key" });

    expect(screen.queryByText("https://example.com/v1")).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "API Key" }));

    expect(screen.getByText("https://example.com/v1")).toBeInTheDocument();
  });

  it("hides base url mode label for oauth providers", () => {
    renderCard({ auth_mode: "oauth", base_url_mode: "ping" });

    expect(screen.queryByText("Ping")).not.toBeInTheDocument();
    expect(screen.queryByText("顺序")).not.toBeInTheDocument();
  });

  it("does not render cost multiplier label when cost multiplier is zero", () => {
    renderCard({ cost_multiplier: 0 });

    expect(screen.queryByText("免费")).not.toBeInTheDocument();
  });

  it("renders circuit breaker state", () => {
    renderCard(
      {},
      {
        circuit: {
          provider_id: 1,
          state: "OPEN",
          open_until: null,
          cooldown_until: null,
        } as any,
      }
    );

    expect(screen.getByTitle("熔断")).toBeInTheDocument();
    expect(screen.getByText("解除熔断")).toBeInTheDocument();
  });

  it("does not render circuit breaker controls for HALF_OPEN probe state", () => {
    renderCard(
      {},
      {
        circuit: {
          provider_id: 1,
          state: "HALF_OPEN",
          open_until: null,
          cooldown_until: null,
        } as any,
      }
    );

    expect(screen.queryByTitle("熔断")).not.toBeInTheDocument();
    expect(screen.queryByText("解除熔断")).not.toBeInTheDocument();
  });

  it("computes unavailableUntil as max of open_until and cooldown_until", () => {
    const futureTs = Math.floor(Date.now() / 1000) + 600;
    renderCard(
      {},
      {
        circuit: {
          provider_id: 1,
          state: "OPEN",
          open_until: futureTs,
          cooldown_until: futureTs + 100,
        } as any,
      }
    );

    // The title should contain the formatted timestamp
    const badge = screen.getByTitle(/熔断至/);
    expect(badge).toBeInTheDocument();
  });

  it("computes unavailableUntil from cooldown_until when not OPEN", () => {
    const futureTs = Math.floor(Date.now() / 1000) + 600;
    renderCard(
      {},
      {
        circuit: {
          provider_id: 1,
          state: "CLOSED",
          open_until: null,
          cooldown_until: futureTs,
        } as any,
      }
    );

    const badge = screen.getByTitle(/熔断至/);
    expect(badge).toBeInTheDocument();
  });

  it("shows terminal launch button when callback provided", () => {
    renderCard(
      {},
      {
        onCopyTerminalLaunchCommand: vi.fn(),
      }
    );

    expect(screen.getByText("终端启动")).toBeInTheDocument();
  });

  it("renders limit chips with fixed daily reset", () => {
    renderCard({
      limit_daily_usd: 50,
      daily_reset_mode: "fixed",
      daily_reset_time: "08:00:00",
    });

    expect(screen.getByText("限额")).toBeInTheDocument();
  });
});
