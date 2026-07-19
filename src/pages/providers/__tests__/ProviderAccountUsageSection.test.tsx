import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { ProviderAccountUsageSection } from "../ProviderAccountUsageSection";
import type { UseProviderEditorFormReturn } from "../useProviderEditorForm";

function makeForm(partial: Partial<UseProviderEditorFormReturn> = {}): UseProviderEditorFormReturn {
  return {
    authMode: "api_key",
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

const summaryCases: Array<[string, Partial<UseProviderEditorFormReturn>, string]> = [
  ["关闭", {}, "关闭"],
  ["sub2api", { accountUsageAdapterKind: "sub2api" }, "sub2api"],
  [
    "NewAPI 模型令牌额度",
    { accountUsageAdapterKind: "newapi", accountUsageNewApiQueryMode: "billing" },
    "NewAPI · 模型令牌额度",
  ],
  [
    "NewAPI 用户账户余额",
    { accountUsageAdapterKind: "newapi", accountUsageNewApiQueryMode: "account" },
    "NewAPI · 用户账户余额",
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
    fireEvent.click(screen.getByRole("radio", { name: "NewAPI" }));
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
    expect(within(getDisclosure().summary).getByText("NewAPI · 模型令牌额度")).toBeInTheDocument();

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
    expect(within(getDisclosure().summary).getByText("NewAPI · 用户账户余额")).toBeInTheDocument();
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
    fireEvent.click(screen.getByRole("switch", { name: "定时刷新账户用量" }));
    fireEvent.change(screen.getByRole("spinbutton"), { target: { value: "180" } });

    expect(setTimedRefreshEnabled).toHaveBeenCalledWith(false);
    expect(setRefreshIntervalSeconds).toHaveBeenCalledWith(180);
    expect(screen.getByRole("spinbutton")).toHaveAttribute("min", "60");
    expect(screen.getByRole("spinbutton")).toHaveAttribute("max", "300");
  });

  it("renders explicit NewAPI account mode, masked token, missing state, and clear action", () => {
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
    expect(screen.getByRole("radiogroup", { name: "NewAPI 查询方式" })).toBeInTheDocument();
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

  it("does not render for non-API-key authentication", () => {
    render(<ProviderAccountUsageSection form={makeForm({ authMode: "oauth" })} />);

    expect(screen.queryByText("账户用量", { selector: "summary span" })).not.toBeInTheDocument();
    expect(screen.queryByRole("radiogroup", { name: "账户用量适配器" })).not.toBeInTheDocument();
  });
});
