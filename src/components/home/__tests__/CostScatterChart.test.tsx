import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { CostScatterCliProviderModelRowV1 } from "../../../services/usage/cost";
import type { ScatterPoint } from "../useCostFilters";

const renderTooltipContent = (content: any, props: any) => {
  if (!content || typeof content.type !== "function") return null;
  const TooltipContent = content.type;
  return <TooltipContent {...content.props} {...props} />;
};

vi.mock("../../charts/lazyRecharts", () => ({
  CartesianGrid: ({ stroke }: any) => <div data-testid="grid" data-stroke={stroke} />,
  LabelList: ({ dataKey, style }: any) => (
    <div data-testid="label-list" data-key={dataKey} data-fill={style?.fill} />
  ),
  ResponsiveContainer: ({ children, width, height }: any) => (
    <div data-testid="responsive-container" data-width={width} data-height={height}>
      {children}
    </div>
  ),
  Scatter: ({ children, data, fill, name }: any) => (
    <div data-testid={`scatter-${name}`} data-fill={fill} data-points={data?.length ?? 0}>
      {children}
    </div>
  ),
  ScatterChart: ({ children, margin }: any) => (
    <div data-testid="scatter-chart" data-margin-right={margin?.right}>
      {children}
    </div>
  ),
  Tooltip: ({ content }: any) => (
    <div data-testid="tooltip">
      <div data-testid="inactive-tooltip">
        {renderTooltipContent(content, { active: false, payload: null })}
      </div>
      <div data-testid="empty-tooltip">
        {renderTooltipContent(content, { active: true, payload: [] })}
      </div>
      <div data-testid="missing-point-tooltip">
        {renderTooltipContent(content, { active: true, payload: [{}] })}
      </div>
      <div data-testid="tooltip-with-data">
        {renderTooltipContent(content, {
          active: true,
          payload: [
            {
              payload: {
                cli: "claude",
                meta: {
                  cli_key: "claude",
                  provider_name: "  ",
                  model: "Unknown",
                  requests_success: Number.NaN,
                  total_cost_usd: 4,
                  total_duration_ms: 1500,
                },
              },
            },
          ],
        })}
      </div>
      <div data-testid="tooltip-with-average">
        {renderTooltipContent(content, {
          active: true,
          payload: [
            {
              payload: {
                cli: "gemini",
                meta: {
                  cli_key: "gemini",
                  provider_name: " Gemini Provider ",
                  model: "gemini-pro",
                  requests_success: 2,
                  total_cost_usd: 6,
                  total_duration_ms: 3000,
                },
              },
            },
          ],
        })}
      </div>
    </div>
  ),
  XAxis: ({ axisLine, tickFormatter }: any) => (
    <div data-testid="x-axis" data-axis-stroke={axisLine?.stroke}>
      {tickFormatter?.(1.25)}
    </div>
  ),
  YAxis: ({ axisLine, tickFormatter, width }: any) => (
    <div data-testid="y-axis" data-axis-line={String(axisLine)} data-width={width}>
      {tickFormatter?.(1500)}
    </div>
  ),
  ZAxis: ({ range }: any) => <div data-testid="z-axis" data-range={range?.join(",")} />,
}));

import { CostScatterChartCard } from "../CostScatterChart";

function makeMeta(
  overrides: Partial<CostScatterCliProviderModelRowV1> = {}
): CostScatterCliProviderModelRowV1 {
  return {
    cli_key: "claude",
    provider_name: "Claude Provider",
    model: "claude-3",
    total_cost_usd: 2,
    total_duration_ms: 1200,
    requests_success: 4,
    ...overrides,
  };
}

function makePoint(overrides: Partial<ScatterPoint> = {}): ScatterPoint {
  const meta = overrides.meta ?? makeMeta({ cli_key: overrides.cli ?? "claude" });
  return {
    name: "Claude Provider / claude-3",
    shortLabel: "Claude",
    x: 2,
    y: 1200,
    z: 4,
    cli: "claude",
    meta,
    ...overrides,
  };
}

describe("components/home/CostScatterChartCard", () => {
  it("renders loading and empty states without chart primitives", () => {
    const onChange = vi.fn();

    const { rerender } = render(
      <CostScatterChartCard
        scatterChartData={{ data: [], activeClis: [] }}
        scatterRows={[]}
        isDark={false}
        loading={true}
        fetching={false}
        scatterCliFilter="all"
        onScatterCliFilterChange={onChange}
      />
    );

    expect(screen.getByText("加载中…")).toBeInTheDocument();
    expect(screen.queryByTestId("scatter-chart")).not.toBeInTheDocument();

    rerender(
      <CostScatterChartCard
        scatterChartData={{ data: [], activeClis: [] }}
        scatterRows={[]}
        isDark={false}
        loading={false}
        fetching={false}
        scatterCliFilter="all"
        onScatterCliFilterChange={onChange}
      />
    );

    expect(screen.getByText("暂无可展示的数据。")).toBeInTheDocument();
    expect(screen.queryByTestId("scatter-chart")).not.toBeInTheDocument();
  });

  it("renders grouped scatter data, tooltip fallbacks, legend, and filter actions", () => {
    const onChange = vi.fn();
    const points = [
      makePoint(),
      makePoint({
        cli: "codex",
        shortLabel: "Codex",
        meta: makeMeta({ cli_key: "codex", provider_name: "Codex Provider", model: "gpt-5" }),
      }),
      makePoint({
        cli: "gemini",
        shortLabel: "Gemini",
        meta: makeMeta({
          cli_key: "gemini",
          provider_name: "Gemini Provider",
          model: "gemini-pro",
        }),
      }),
      makePoint({
        cli: "unknown" as any,
        shortLabel: "Other",
        meta: makeMeta({ cli_key: "unknown" as any }),
      }),
    ];

    render(
      <CostScatterChartCard
        scatterChartData={{ data: points, activeClis: ["claude", "codex", "gemini"] }}
        scatterRows={[{ cli_key: "claude" }, { cli_key: "codex" }, { cli_key: "gemini" }]}
        isDark={true}
        loading={false}
        fetching={false}
        scatterCliFilter="codex"
        onScatterCliFilterChange={onChange}
      />
    );

    expect(screen.getByText("总成本 × 总耗时")).toBeInTheDocument();
    expect(screen.getAllByText("Claude")).not.toHaveLength(0);
    expect(screen.getAllByText("Codex")).not.toHaveLength(0);
    expect(screen.getAllByText("Gemini")).not.toHaveLength(0);
    expect(screen.getByTestId("grid")).toHaveAttribute("data-stroke", "rgba(100, 150, 255, 0.1)");
    expect(screen.getByTestId("x-axis")).toHaveTextContent("$1.25");
    expect(screen.getByTestId("y-axis")).toHaveTextContent("1.5s");
    expect(screen.getByTestId("z-axis")).toHaveAttribute("data-range", "60,400");
    expect(screen.getByTestId("scatter-Claude")).toHaveAttribute("data-points", "1");
    expect(screen.getByTestId("scatter-Codex")).toHaveAttribute("data-points", "1");
    expect(screen.getByTestId("scatter-Gemini")).toHaveAttribute("data-points", "1");
    expect(screen.getAllByTestId("label-list")).toHaveLength(3);

    expect(screen.getByTestId("inactive-tooltip")).toBeEmptyDOMElement();
    expect(screen.getByTestId("empty-tooltip")).toBeEmptyDOMElement();
    expect(screen.getByTestId("missing-point-tooltip")).toBeEmptyDOMElement();
    expect(
      within(screen.getByTestId("tooltip-with-data")).getByText("Claude · 未知 · 未知")
    ).toBeInTheDocument();
    expect(
      within(screen.getByTestId("tooltip-with-data")).getByText("请求数：0")
    ).toBeInTheDocument();
    expect(
      within(screen.getByTestId("tooltip-with-data")).getByText("均值：—")
    ).toBeInTheDocument();
    expect(
      within(screen.getByTestId("tooltip-with-average")).getByText(
        "Gemini · Gemini Provider · gemini-pro"
      )
    ).toBeInTheDocument();
    expect(
      within(screen.getByTestId("tooltip-with-average")).getByText("均值：$3.000000 / 1.50s")
    ).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "全部" }));
    fireEvent.click(screen.getByRole("button", { name: "Claude" }));
    expect(onChange).toHaveBeenNthCalledWith(1, "all");
    expect(onChange).toHaveBeenNthCalledWith(2, "claude");
  });

  it("disables filter buttons while fetching and suppresses single-cli legend", () => {
    const onChange = vi.fn();

    render(
      <CostScatterChartCard
        scatterChartData={{ data: [makePoint()], activeClis: ["claude"] }}
        scatterRows={[{ cli_key: "claude" }]}
        isDark={false}
        loading={false}
        fetching={true}
        scatterCliFilter="all"
        onScatterCliFilterChange={onChange}
      />
    );

    const root = screen.getByTestId("home-cost-scatter-chart");
    const buttons = within(root).getAllByRole("button");
    expect(buttons.every((button) => button.hasAttribute("disabled"))).toBe(true);
    expect(within(root).getAllByText("Claude")).toHaveLength(1);

    fireEvent.click(screen.getByRole("button", { name: "Codex" }));
    expect(onChange).not.toHaveBeenCalled();
    expect(screen.getByTestId("grid")).toHaveAttribute("data-stroke", "rgba(15,23,42,0.08)");
  });
});
