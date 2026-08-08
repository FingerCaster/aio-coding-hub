import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { ProviderAccountUsageSection } from "../ProviderAccountUsageSection";
import type { UseProviderEditorFormReturn } from "../useProviderEditorForm";

function makeForm(partial: Partial<UseProviderEditorFormReturn> = {}): UseProviderEditorFormReturn {
  return {
    editingProviderId: null,
    authMode: "api_key",
    apiKeyConfigured: false,
    saving: false,
    accountUsageAdapterKind: "disabled",
    setAccountUsageAdapterKind: vi.fn(),
    accountUsageNewApiQueryMode: "billing",
    setAccountUsageNewApiQueryMode: vi.fn(),
    accountUsageNewApiUserId: "",
    setAccountUsageNewApiUserId: vi.fn(),
    accountUsageNewApiAccessToken: "",
    setAccountUsageNewApiAccessToken: vi.fn(),
    accountUsageNewApiAccessTokenConfigured: false,
    accountUsageCredentialsPresent: false,
    accountUsageCredentialsRequired: false,
    clearAccountUsageCredentials: vi.fn(),
    accountUsageTimedRefreshEnabled: true,
    setAccountUsageTimedRefreshEnabled: vi.fn(),
    accountUsageRefreshIntervalSeconds: 300,
    setAccountUsageRefreshIntervalSeconds: vi.fn(),
    accountUsageCustomScript: "",
    setAccountUsageCustomScript: vi.fn(),
    accountUsageCustomAllowedOrigins: [],
    setAccountUsageCustomAllowedOrigins: vi.fn(),
    accountUsageCustomAllowedOriginsCount: 0,
    accountUsageCustomAllowedOriginsError: null,
    accountUsageCustomTimeoutSeconds: 10,
    setAccountUsageCustomTimeoutSeconds: vi.fn(),
    accountUsageCustomEnabled: false,
    setAccountUsageCustomEnabled: vi.fn(),
    accountUsageCustomTestPending: false,
    accountUsageCustomTestInFlight: false,
    accountUsageCustomTestResult: null,
    accountUsageCustomTestError: null,
    testAccountUsageCustomScript: vi.fn(),
    ...partial,
  } as unknown as UseProviderEditorFormReturn;
}

function getDisclosure() {
  const summary = screen.getByText("账户用量", { selector: "summary span" }).closest("summary");
  const details = summary?.closest("details") as HTMLDetailsElement | null;
  if (!summary || !details) throw new Error("账户用量折叠面板不存在");
  return { details, summary };
}

function openDisclosure() {
  const disclosure = getDisclosure();
  fireEvent.click(disclosure.summary);
  expect(disclosure.details.open).toBe(true);
  return disclosure;
}

function expectFullWidthRadioOptions(group: HTMLElement) {
  expect(group).toHaveClass("w-full");
  expect(group).not.toHaveClass("w-auto");
  within(group)
    .getAllByRole("radio")
    .forEach((option) => expect(option).toHaveClass("flex-1"));
}

function expectElementBefore(first: HTMLElement, second: HTMLElement) {
  expect(first.compareDocumentPosition(second) & Node.DOCUMENT_POSITION_FOLLOWING).not.toBe(0);
}

const summaryCases: Array<[string, Partial<UseProviderEditorFormReturn>, string]> = [
  ["关闭", {}, "关闭"],
  ["Sub2Api", { accountUsageAdapterKind: "sub2api" }, "Sub2Api"],
  [
    "NewApi 模型令牌额度",
    { accountUsageAdapterKind: "newapi", accountUsageNewApiQueryMode: "billing" },
    "NewApi · 模型令牌额度",
  ],
  [
    "NewApi 用户账户余额",
    { accountUsageAdapterKind: "newapi", accountUsageNewApiQueryMode: "account" },
    "NewApi · 用户账户余额",
  ],
  [
    "自定义 JS 未启用",
    { accountUsageAdapterKind: "custom", accountUsageCustomEnabled: false },
    "自定义 JS · 未启用",
  ],
  [
    "自定义 JS 已启用",
    { accountUsageAdapterKind: "custom", accountUsageCustomEnabled: true },
    "自定义 JS · 已启用",
  ],
];

describe("ProviderAccountUsageSection", () => {
  it.each(summaryCases)("renders %s status closed by default", (_name, partial, status) => {
    render(<ProviderAccountUsageSection form={makeForm(partial)} />);

    const { details, summary } = getDisclosure();
    expect(details.open).toBe(false);
    expect(within(summary).getByText(status)).toBeInTheDocument();
    expect(screen.getByRole("radiogroup", { name: "账户用量适配器" })).not.toBeVisible();
  });

  it("opens and closes while keeping disabled-only controls absent", () => {
    render(<ProviderAccountUsageSection form={makeForm()} />);

    const { details, summary } = openDisclosure();
    expect(screen.getByRole("radiogroup", { name: "账户用量适配器" })).toBeInTheDocument();
    expect(screen.queryByRole("switch", { name: "定时刷新账户用量" })).not.toBeInTheDocument();
    expect(screen.queryByRole("spinbutton")).not.toBeInTheDocument();

    fireEvent.click(summary);
    expect(details.open).toBe(false);
    expect(screen.getByRole("radiogroup", { name: "账户用量适配器" })).not.toBeVisible();
  });

  it("updates the summary without resetting the disclosure", () => {
    const setAdapterKind = vi.fn();
    const setQueryMode = vi.fn();
    const { rerender } = render(
      <ProviderAccountUsageSection
        form={makeForm({
          setAccountUsageAdapterKind: setAdapterKind,
          setAccountUsageNewApiQueryMode: setQueryMode,
        })}
      />
    );

    const { details } = openDisclosure();
    fireEvent.click(screen.getByRole("radio", { name: "NewApi" }));
    expect(setAdapterKind).toHaveBeenCalledWith("newapi");

    rerender(
      <ProviderAccountUsageSection
        form={makeForm({
          accountUsageAdapterKind: "newapi",
          accountUsageNewApiQueryMode: "billing",
          setAccountUsageAdapterKind: setAdapterKind,
          setAccountUsageNewApiQueryMode: setQueryMode,
        })}
      />
    );
    expect(details.open).toBe(true);
    expect(within(getDisclosure().summary).getByText("NewApi · 模型令牌额度")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("radio", { name: "用户账户余额" }));
    expect(setQueryMode).toHaveBeenCalledWith("account");

    rerender(
      <ProviderAccountUsageSection
        form={makeForm({
          accountUsageAdapterKind: "newapi",
          accountUsageNewApiQueryMode: "account",
          setAccountUsageAdapterKind: setAdapterKind,
          setAccountUsageNewApiQueryMode: setQueryMode,
        })}
      />
    );
    expect(within(getDisclosure().summary).getByText("NewApi · 用户账户余额")).toBeInTheDocument();
  });

  it("renders timed refresh controls for configured account usage", () => {
    const setTimedRefreshEnabled = vi.fn();
    const setRefreshIntervalSeconds = vi.fn();
    render(
      <ProviderAccountUsageSection
        form={makeForm({
          accountUsageAdapterKind: "sub2api",
          accountUsageTimedRefreshEnabled: true,
          accountUsageRefreshIntervalSeconds: 120,
          setAccountUsageTimedRefreshEnabled: setTimedRefreshEnabled,
          setAccountUsageRefreshIntervalSeconds: setRefreshIntervalSeconds,
        })}
      />
    );

    openDisclosure();
    const selectorRow = screen.getByRole("group", { name: "账户用量选择设置" });
    const refreshRow = screen.getByRole("group", { name: "账户用量刷新设置" });
    const adapterGroup = within(selectorRow).getByRole("radiogroup", {
      name: "账户用量适配器",
    });
    const refreshSwitch = within(refreshRow).getByRole("switch", {
      name: "定时刷新账户用量",
    });
    const refreshInterval = within(refreshRow).getByRole("spinbutton");

    expect(selectorRow).toHaveClass("grid", "sm:grid-cols-2");
    expect(refreshRow).toHaveClass("grid", "sm:grid-cols-2");
    expectElementBefore(selectorRow, refreshRow);
    expectFullWidthRadioOptions(adapterGroup);

    fireEvent.click(refreshSwitch);
    fireEvent.change(refreshInterval, { target: { value: "180" } });

    expect(setTimedRefreshEnabled).toHaveBeenCalledWith(false);
    expect(setRefreshIntervalSeconds).toHaveBeenCalledWith(180);
    expect(refreshInterval).toHaveAttribute("min", "60");
    expect(refreshInterval).toHaveAttribute("max", "300");
  });

  it("keeps NewApi selectors, credentials, and refresh controls in natural responsive rows", () => {
    render(
      <ProviderAccountUsageSection
        form={makeForm({
          accountUsageAdapterKind: "newapi",
          accountUsageNewApiQueryMode: "account",
          accountUsageNewApiUserId: "42",
          accountUsageCredentialsPresent: true,
        })}
      />
    );

    openDisclosure();
    const selectorRow = screen.getByRole("group", { name: "账户用量选择设置" });
    const credentialsRow = screen.getByRole("group", { name: "账户用量凭据设置" });
    const refreshRow = screen.getByRole("group", { name: "账户用量刷新设置" });
    const adapterGroup = within(selectorRow).getByRole("radiogroup", {
      name: "账户用量适配器",
    });
    const queryModeGroup = within(selectorRow).getByRole("radiogroup", {
      name: "NewApi 查询方式",
    });

    expect(within(selectorRow).getByText("NewApi 查询方式")).toBeInTheDocument();
    expect(selectorRow).toHaveClass("grid", "sm:grid-cols-2");
    expect(credentialsRow).toHaveClass("grid", "sm:grid-cols-2");
    expect(refreshRow).toHaveClass("grid", "sm:grid-cols-2");
    expectElementBefore(selectorRow, credentialsRow);
    expectElementBefore(credentialsRow, refreshRow);
    expectFullWidthRadioOptions(adapterGroup);
    expectFullWidthRadioOptions(queryModeGroup);
    expect(within(credentialsRow).getByDisplayValue("42")).toBeInTheDocument();
    expect(
      within(credentialsRow).getByRole("button", { name: "清除账户凭据" })
    ).toBeInTheDocument();
    expect(
      within(refreshRow).getByRole("switch", { name: "定时刷新账户用量" })
    ).toBeInTheDocument();
    expect(within(refreshRow).getByRole("spinbutton")).toBeInTheDocument();
    expect(refreshRow).not.toContainElement(screen.getByDisplayValue("42"));
  });

  it("renders explicit NewApi account mode, masked token, missing state, and clear action", () => {
    const setQueryMode = vi.fn();
    const setAccessToken = vi.fn();
    const clearCredentials = vi.fn();
    render(
      <ProviderAccountUsageSection
        form={makeForm({
          accountUsageAdapterKind: "newapi",
          accountUsageNewApiQueryMode: "account",
          setAccountUsageNewApiQueryMode: setQueryMode,
          accountUsageNewApiUserId: "42",
          accountUsageNewApiAccessToken: "SYNTHETIC_DRAFT",
          setAccountUsageNewApiAccessToken: setAccessToken,
          accountUsageCredentialsPresent: true,
          accountUsageCredentialsRequired: true,
          clearAccountUsageCredentials: clearCredentials,
        })}
      />
    );

    const { details, summary } = getDisclosure();
    expect(details.open).toBe(false);
    expect(within(summary).getByText("需配置账户凭据")).toBeInTheDocument();

    openDisclosure();
    expect(screen.getAllByText("需配置账户凭据")).toHaveLength(2);
    expect(screen.getByRole("radiogroup", { name: "NewApi 查询方式" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("radio", { name: "模型令牌额度" }));
    expect(setQueryMode).toHaveBeenCalledWith("billing");
    const token = screen.getByDisplayValue("SYNTHETIC_DRAFT");
    expect(token).toHaveAttribute("type", "password");
    fireEvent.click(screen.getByRole("button", { name: "显示系统访问令牌" }));
    expect(token).toHaveAttribute("type", "text");
    fireEvent.change(token, { target: { value: "SYNTHETIC_REPLACEMENT" } });
    expect(setAccessToken).toHaveBeenCalledWith("SYNTHETIC_REPLACEMENT");
    fireEvent.click(screen.getByRole("button", { name: "清除账户凭据" }));
    expect(clearCredentials).toHaveBeenCalledOnce();
  });

  it("edits and tests a custom JavaScript account-usage draft", () => {
    const setScript = vi.fn();
    const setAllowedOrigins = vi.fn();
    const setTimeoutSeconds = vi.fn();
    const setCustomEnabled = vi.fn();
    const testCustomScript = vi.fn();
    render(
      <ProviderAccountUsageSection
        form={makeForm({
          editingProviderId: 19,
          apiKeyConfigured: true,
          accountUsageAdapterKind: "custom",
          accountUsageCustomScript: "中😀",
          setAccountUsageCustomScript: setScript,
          accountUsageCustomAllowedOrigins: ["https://usage.example.invalid"],
          setAccountUsageCustomAllowedOrigins: setAllowedOrigins,
          accountUsageCustomAllowedOriginsCount: 1,
          accountUsageCustomTimeoutSeconds: 8,
          setAccountUsageCustomTimeoutSeconds: setTimeoutSeconds,
          setAccountUsageCustomEnabled: setCustomEnabled,
          testAccountUsageCustomScript: testCustomScript,
        })}
      />
    );

    openDisclosure();
    expect(screen.getByRole("radio", { name: "自定义 JS" })).toBeChecked();
    expect(
      screen.getByText(
        /脚本和全部目标服务都可读取或转发当前供应商 API Key；仅信任已核对的脚本、Base URL 和额外 HTTPS Origin。/
      )
    ).toBeInTheDocument();
    expect(screen.getByText("7/32768 字节")).toBeInTheDocument();

    const script = screen.getByRole("textbox", { name: "账户用量 JavaScript" });
    const origins = screen.getByRole("textbox", { name: "额外 HTTPS Origin，每行一个" });
    const timeout = screen.getByRole("spinbutton", {
      name: "自定义账户用量请求超时",
    });
    fireEvent.change(script, { target: { value: "({ request: 1, parse: 2 })" } });
    fireEvent.change(origins, {
      target: { value: "https://first.example.invalid\nhttps://second.example.invalid\n" },
    });
    fireEvent.change(timeout, { target: { value: "12" } });
    const enableSwitch = screen.getByRole("switch", { name: "启用自定义账户用量脚本" });
    const confirmationStatus = screen.getByRole("status", {
      name: "自定义账户用量确认状态",
    });
    expect(script).toHaveAccessibleDescription(
      /7\/32768 字节.*脚本和全部目标服务都可读取或转发当前供应商 API Key/
    );
    expect(origins).toHaveAccessibleDescription(
      /1\/16.*仅信任已核对的脚本、Base URL 和额外 HTTPS Origin/
    );
    expect(timeout).toHaveAccessibleDescription("2-15s");
    expect(enableSwitch).toHaveAccessibleDescription(
      /保存时弹出系统确认；脚本、Base URL 或额外 Origin 变更后需重新确认.*脚本和全部目标服务/
    );
    expect(confirmationStatus).toHaveAttribute("aria-live", "polite");
    expect(confirmationStatus).toHaveAttribute("aria-atomic", "true");
    expect(confirmationStatus).toHaveTextContent("未启用");
    fireEvent.click(enableSwitch);
    fireEvent.click(screen.getByRole("button", { name: "测试脚本" }));

    expect(setScript).toHaveBeenCalledWith("({ request: 1, parse: 2 })");
    expect(setAllowedOrigins).toHaveBeenCalledWith([
      "https://first.example.invalid",
      "https://second.example.invalid",
      "",
    ]);
    expect(setTimeoutSeconds).toHaveBeenCalledWith(12);
    expect(setCustomEnabled).toHaveBeenCalledWith(true);
    expect(testCustomScript).toHaveBeenCalledOnce();
    expect(timeout).toHaveAttribute("min", "2");
    expect(timeout).toHaveAttribute("max", "15");
  });

  it("requires a saved provider with a configured API key before testing", () => {
    const { rerender } = render(
      <ProviderAccountUsageSection
        form={makeForm({ accountUsageAdapterKind: "custom", editingProviderId: null })}
      />
    );

    openDisclosure();
    expect(screen.getByRole("button", { name: "测试脚本" })).toBeDisabled();
    expect(screen.getByText("保存供应商后可测试")).toBeInTheDocument();

    rerender(
      <ProviderAccountUsageSection
        form={makeForm({
          accountUsageAdapterKind: "custom",
          editingProviderId: 20,
          apiKeyConfigured: false,
        })}
      />
    );
    expect(screen.getByRole("button", { name: "测试脚本" })).toBeDisabled();
    expect(screen.getByText("需先保存 API Key")).toBeInTheDocument();
  });

  it("associates explicit origin validation errors and blocks unsafe actions", () => {
    render(
      <ProviderAccountUsageSection
        form={makeForm({
          editingProviderId: 20,
          apiKeyConfigured: true,
          accountUsageAdapterKind: "custom",
          accountUsageCustomScript: "({ request: () => ({}) })",
          accountUsageCustomAllowedOrigins: ["http://invalid.example/path"],
          accountUsageCustomAllowedOriginsError: "第 1 行必须是仅含协议、主机和端口的 HTTPS Origin",
        })}
      />
    );

    const { summary } = openDisclosure();
    expect(within(summary).getByText("Origin 配置有误")).toBeInTheDocument();
    const origins = screen.getByRole("textbox", { name: "额外 HTTPS Origin，每行一个" });
    expect(origins).toHaveAttribute("aria-invalid", "true");
    expect(origins).toHaveAccessibleDescription(/第 1 行必须是仅含协议、主机和端口的 HTTPS Origin/);
    expect(screen.getByRole("alert")).toHaveTextContent(
      "第 1 行必须是仅含协议、主机和端口的 HTTPS Origin"
    );
    expect(screen.getByRole("button", { name: "测试脚本" })).toBeDisabled();
    expect(screen.getByRole("switch", { name: "启用自定义账户用量脚本" })).toBeDisabled();
  });

  it("keeps test and enable controls disabled until the real test promise settles", () => {
    render(
      <ProviderAccountUsageSection
        form={makeForm({
          editingProviderId: 20,
          apiKeyConfigured: true,
          accountUsageAdapterKind: "custom",
          accountUsageCustomScript: "({ request: () => ({}) })",
          accountUsageCustomTestPending: false,
          accountUsageCustomTestInFlight: true,
        })}
      />
    );

    openDisclosure();
    const testButton = screen.getByRole("button", { name: "测试中…" });
    expect(testButton).toBeDisabled();
    expect(testButton).toHaveAttribute("aria-busy", "true");
    expect(testButton).toHaveAccessibleDescription(/请等待当前测试完成/);
    expect(screen.getByRole("switch", { name: "启用自定义账户用量脚本" })).toBeDisabled();
  });

  it("renders only normalized custom test results or sanitized errors", () => {
    const result = {
      adapter_kind: null,
      status: "available" as const,
      freshness: "fresh" as const,
      plan_name: "Synthetic",
      balance: 12.5,
      plan_remaining: null,
      used: 7.5,
      total: 20,
      unit: "USD",
      unit_note: null,
      daily_used: null,
      daily_total: null,
      weekly_used: null,
      weekly_total: null,
      monthly_used: null,
      monthly_total: null,
      expires_at: null,
      last_fetched_at: 1,
      message: "测试成功",
    };
    const { rerender } = render(
      <ProviderAccountUsageSection
        form={makeForm({
          accountUsageAdapterKind: "custom",
          accountUsageCustomTestResult: result,
        })}
      />
    );

    openDisclosure();
    const status = screen.getByRole("status", { name: "自定义账户用量测试结果" });
    expect(status).toHaveTextContent("测试结果：可用");
    expect(status).toHaveTextContent("套餐 Synthetic");
    expect(status).toHaveTextContent("余额 12.5 USD");
    expect(status).toHaveTextContent("测试成功");

    rerender(
      <ProviderAccountUsageSection
        form={makeForm({
          accountUsageAdapterKind: "custom",
          accountUsageCustomTestError: "脚本解析失败",
        })}
      />
    );
    expect(
      screen.queryByRole("status", { name: "自定义账户用量测试结果" })
    ).not.toBeInTheDocument();
    expect(screen.getByRole("alert")).toHaveTextContent("脚本解析失败");
  });

  it("does not render for non-API-key authentication", () => {
    render(<ProviderAccountUsageSection form={makeForm({ authMode: "oauth" })} />);

    expect(screen.queryByText("账户用量", { selector: "summary span" })).not.toBeInTheDocument();
    expect(screen.queryByRole("radiogroup", { name: "账户用量适配器" })).not.toBeInTheDocument();
  });
});
