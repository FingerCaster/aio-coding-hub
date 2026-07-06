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
    accountUsageNewApiUserId: "",
    setAccountUsageNewApiUserId: vi.fn(),
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
});
