import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { UsageFilters } from "../UsageFilters";

describe("components/usage/UsageFilters", () => {
  const baseProps = {
    cliKey: "all" as const,
    onCliKeyChange: vi.fn(),
    period: "custom" as const,
    onPeriodChange: vi.fn(),
    loading: false,
    showCustomForm: true,
    customStartDate: "2026-04-01",
    customEndDate: "2026-04-15",
    onCustomStartDateChange: vi.fn(),
    onCustomEndDateChange: vi.fn(),
    customApplied: null,
    onApplyCustomRange: vi.fn(),
    onClearCustomRange: vi.fn(),
  };

  it("renders the applied custom date range and wires custom form controls", () => {
    const onCliKeyChange = vi.fn();
    const onPeriodChange = vi.fn();
    const onCustomStartDateChange = vi.fn();
    const onCustomEndDateChange = vi.fn();
    const onApplyCustomRange = vi.fn();
    const onClearCustomRange = vi.fn();

    render(
      <UsageFilters
        {...baseProps}
        onCliKeyChange={onCliKeyChange}
        onPeriodChange={onPeriodChange}
        onCustomStartDateChange={onCustomStartDateChange}
        onCustomEndDateChange={onCustomEndDateChange}
        customApplied={{
          startDate: "2026-04-01",
          endDate: "2026-04-15",
          startTs: 1_775_174_400,
          endTs: 1_776_384_000,
        }}
        onApplyCustomRange={onApplyCustomRange}
        onClearCustomRange={onClearCustomRange}
      />
    );

    fireEvent.change(screen.getByLabelText("开始日期"), {
      target: { value: "2026-04-02" },
    });
    fireEvent.change(screen.getByLabelText("结束日期"), {
      target: { value: "2026-04-16" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Claude" }));
    fireEvent.click(screen.getByRole("button", { name: "近 7 天" }));
    fireEvent.click(screen.getByRole("button", { name: "应用" }));
    fireEvent.click(screen.getByRole("button", { name: "清空" }));

    expect(screen.getByText("2026-04-01 → 2026-04-15")).toBeInTheDocument();
    expect(onCliKeyChange).toHaveBeenCalledWith("claude");
    expect(onPeriodChange).toHaveBeenCalledWith("weekly");
    expect(onCustomStartDateChange).toHaveBeenCalledWith("2026-04-02");
    expect(onCustomEndDateChange).toHaveBeenCalledWith("2026-04-16");
    expect(onApplyCustomRange).toHaveBeenCalledTimes(1);
    expect(onClearCustomRange).toHaveBeenCalledTimes(1);
  });

  it("omits the applied custom date range before one is applied", () => {
    render(<UsageFilters {...baseProps} />);

    expect(screen.queryByText("2026-04-01 → 2026-04-15")).not.toBeInTheDocument();
  });

  it("hides the custom date controls when the custom form is closed", () => {
    render(<UsageFilters {...baseProps} showCustomForm={false} />);

    expect(screen.queryByLabelText("开始日期")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("结束日期")).not.toBeInTheDocument();
  });
});
