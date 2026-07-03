import { render } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { UsageProviderCacheRateTrendRowV1 } from "../../services/usage/usage";

vi.mock("../../hooks/useTheme", () => ({
  useTheme: () => ({ theme: "light", resolvedTheme: "light", setTheme: vi.fn() }),
}));

vi.mock("../charts/lazyRecharts", () => {
  const renderTooltipContent = (content: any, props: any) => {
    if (!content || typeof content.type !== "function") return null;
    const TooltipContent = content.type;
    return <TooltipContent {...content.props} {...props} />;
  };

  const warnPayload = Array.from({ length: 13 }, (_, index) => {
    const key = `warn-${index}`;
    return {
      dataKey: key,
      name: `Warn ${index}`,
      color: "#ef4444",
      value: 0.2 + index / 100,
      payload: {
        [`${key}_meta`]: {
          denomTokens: 1000 + index,
          cacheReadTokens: 200 + index,
          requestsSuccess: 10 + index,
        },
      },
    };
  });

  const okPayload = [
    {
      dataKey: "ok",
      name: "OK",
      color: "#22c55e",
      value: 0.9,
      payload: {
        ok_meta: {
          denomTokens: 5000,
          cacheReadTokens: 4500,
          requestsSuccess: 20,
        },
      },
    },
  ];

  return {
    CartesianGrid: () => <g data-testid="grid" />,
    Legend: () => <div data-testid="legend" />,
    Line: ({ dataKey }: any) => <path data-testid={`line-${dataKey}`} />,
    LineChart: ({ children, data }: any) => (
      <div data-testid="line-chart" data-points={data?.length ?? 0}>
        {children}
      </div>
    ),
    ReferenceArea: ({ x1, x2 }: any) => (
      <div data-testid="reference-area" data-x1={x1} data-x2={x2} />
    ),
    ReferenceLine: ({ y }: any) => <div data-testid="reference-line" data-y={y} />,
    ResponsiveContainer: ({ children }: any) => <div data-testid="responsive">{children}</div>,
    Tooltip: ({ content }: any) => (
      <div data-testid="tooltip">
        {renderTooltipContent(content, { active: false, payload: null, label: "empty" })}
        {renderTooltipContent(content, { active: true, payload: [], label: "empty-list" })}
        {renderTooltipContent(content, {
          active: true,
          label: "warn",
          payload: [
            { dataKey: "", value: 0.5, payload: {} },
            { dataKey: "missing-meta", value: 0.5, payload: {} },
            { dataKey: "nan", value: Number.NaN, payload: { nan_meta: {} } },
            ...warnPayload,
          ],
        })}
        {renderTooltipContent(content, { active: true, label: "ok", payload: okPayload })}
      </div>
    ),
    XAxis: ({ ticks }: any) => <div data-testid="x-axis" data-ticks={ticks?.join(",") ?? ""} />,
    YAxis: ({ ticks }: any) => <div data-testid="y-axis" data-ticks={ticks?.join(",") ?? ""} />,
  };
});

import { UsageProviderCacheRateTrendChart } from "../UsageProviderCacheRateTrendChart";

const sampleRow: UsageProviderCacheRateTrendRowV1 = {
  day: "2026-02-20",
  hour: null,
  key: "openai",
  name: "OpenAI",
  denom_tokens: 200,
  cache_read_input_tokens: 100,
  requests_success: 10,
};

describe("components/UsageProviderCacheRateTrendChart", () => {
  it("renders without data", () => {
    const { container } = render(
      <UsageProviderCacheRateTrendChart rows={[]} period="weekly" customApplied={null} />
    );
    expect(container).toBeTruthy();
  });

  it("renders with weekly data", () => {
    const rows: UsageProviderCacheRateTrendRowV1[] = [
      sampleRow,
      { ...sampleRow, day: "2026-02-21", cache_read_input_tokens: 200 },
      { ...sampleRow, key: "anthropic", name: "Anthropic", day: "2026-02-20" },
    ];
    const { container } = render(
      <UsageProviderCacheRateTrendChart rows={rows} period="weekly" customApplied={null} />
    );
    expect(container).toBeTruthy();
  });

  it("renders with daily (hourly) period", () => {
    const rows: UsageProviderCacheRateTrendRowV1[] = [
      { ...sampleRow, hour: 10 },
      { ...sampleRow, hour: 14 },
    ];
    const { container } = render(
      <UsageProviderCacheRateTrendChart rows={rows} period="daily" customApplied={null} />
    );
    expect(container).toBeTruthy();
  });

  it("renders with monthly period", () => {
    const { container } = render(
      <UsageProviderCacheRateTrendChart rows={[sampleRow]} period="monthly" customApplied={null} />
    );
    expect(container).toBeTruthy();
  });

  it("renders with allTime period", () => {
    const { container } = render(
      <UsageProviderCacheRateTrendChart rows={[sampleRow]} period="allTime" customApplied={null} />
    );
    expect(container).toBeTruthy();
  });

  it("renders with custom date range", () => {
    const { container } = render(
      <UsageProviderCacheRateTrendChart
        rows={[sampleRow]}
        period="custom"
        customApplied={{
          startDate: "2026-02-15",
          endDate: "2026-02-25",
          startTs: 1739577600,
          endTs: 1740441600,
        }}
      />
    );
    expect(container).toBeTruthy();
  });
});
