import { fireEvent, render, screen } from "@testing-library/react";
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

describe("ProviderAccountUsageSection", () => {
  it("hides timed refresh controls while account usage is disabled", () => {
    render(<ProviderAccountUsageSection form={makeForm()} />);

    expect(screen.getByRole("radiogroup", { name: "账户用量适配器" })).toBeInTheDocument();
    expect(screen.queryByRole("switch", { name: "定时刷新账户用量" })).not.toBeInTheDocument();
    expect(screen.queryByRole("spinbutton")).not.toBeInTheDocument();
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

    expect(screen.getByText("需配置账户凭据")).toBeInTheDocument();
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
});
