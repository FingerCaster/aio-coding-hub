import { render, screen, within } from "@testing-library/react";
import { createElement } from "react";
import { describe, expect, it, vi } from "vitest";
import { UsageTokensChart } from "../UsageTokensChart";
import type { UsageHourlyRow } from "../../services/usage/usage";

vi.mock("../charts/lazyRecharts", async () => {
  const actual =
    await vi.importActual<typeof import("../charts/lazyRecharts")>("../charts/lazyRecharts");
  return {
    ...actual,
    ResponsiveContainer: ({ children }: any) => (
      <div data-testid="responsive-container">
        {typeof children === "function" ? children({ width: 400, height: 300 }) : children}
      </div>
    ),
    AreaChart: ({ children, data }: any) => (
      <div data-testid="area-chart" data-points={data.length}>
        {children}
      </div>
    ),
    CartesianGrid: (props: any) => <div data-testid="grid" data-stroke={props.stroke} />,
    XAxis: (props: any) => <div data-testid="x-axis" data-ticks={(props.ticks ?? []).join("|")} />,
    YAxis: (props: any) => (
      <div data-testid="y-axis" data-ticks={(props.ticks ?? []).join("|")}>
        {props.tickFormatter?.(1_500_000)}
      </div>
    ),
    Tooltip: ({ content }: any) => (
      <div data-testid="tooltip">
        {createElement(content.type, {
          active: true,
          payload: [{ value: 1_500_000 }],
          label: "03/12",
        })}
        <div data-testid="empty-tooltip">
          {createElement(content.type, { active: false, payload: [], label: "03/12" })}
        </div>
      </div>
    ),
    Area: (props: any) => (
      <div data-testid="area" data-key={props.dataKey} data-animation={props.animationDuration} />
    ),
  };
});

function makeHourlyRow(overrides: Partial<UsageHourlyRow> = {}): UsageHourlyRow {
  return {
    day: "2026-03-17",
    hour: 0,
    requests_total: 1,
    requests_with_usage: 1,
    requests_success: 1,
    requests_failed: 0,
    total_tokens: 1_000_000,
    ...overrides,
  };
}

describe("UsageTokensChart rendering", () => {
  it("renders aggregated token data through the chart primitives", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-03-18T12:00:00Z"));

    const rows: UsageHourlyRow[] = [
      makeHourlyRow({ day: "2026-03-17", hour: 0, total_tokens: 1_000_000 }),
      makeHourlyRow({ day: "2026-03-17", hour: 1, total_tokens: "500000" as unknown as number }),
      makeHourlyRow({ day: "2026-03-18", hour: 1, total_tokens: null as unknown as number }),
      makeHourlyRow({ day: "", hour: 1, total_tokens: 99_999 }),
    ];

    render(<UsageTokensChart rows={rows} days={7} className="custom-chart" />);

    const root = screen.getByTestId("responsive-container").parentElement!;
    expect(root).toHaveClass("custom-chart");
    expect(screen.getByTestId("area-chart")).toBeInTheDocument();
    expect(screen.getByTestId("area-chart")).toHaveAttribute("data-points", "7");
    expect(screen.getByTestId("x-axis").dataset.ticks).toContain("03/18");
    expect(screen.getByTestId("y-axis").textContent).toContain("1.5M");
    expect(screen.getByTestId("area")).toHaveAttribute("data-key", "tokens");
    expect(within(screen.getByTestId("tooltip")).getByText("03/12")).toBeInTheDocument();
    expect(within(screen.getByTestId("tooltip")).getByText("Tokens")).toBeInTheDocument();
    expect(screen.getByTestId("empty-tooltip")).toBeEmptyDOMElement();

    vi.useRealTimers();
  });
});
