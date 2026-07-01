import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { SettingsDataSyncCard } from "../SettingsDataSyncCard";

function renderCard(overrides: Record<string, unknown> = {}) {
  const props = {
    about: { run_mode: "installed" } as any,
    modelPricesAvailable: "available" as const,
    modelPricesCount: 12,
    lastModelPricesSyncError: null,
    lastModelPricesSyncReport: null,
    lastModelPricesSyncTime: null,
    openModelPriceAliasesDialog: vi.fn(),
    todayRequestsAvailable: "available" as const,
    todayRequestsTotal: 9,
    syncingModelPrices: false,
    syncModelPrices: vi.fn().mockResolvedValue(undefined),
    ...overrides,
  };

  return {
    ...render(<SettingsDataSyncCard {...props} />),
    props,
  };
}

describe("pages/settings/SettingsDataSyncCard", () => {
  it("renders checking state, opens alias config, and triggers normal/forced sync", () => {
    const { props } = renderCard({
      about: null,
      modelPricesAvailable: "checking",
      todayRequestsAvailable: "checking",
    });

    expect(screen.getAllByText("加载中…")).toHaveLength(2);
    expect(screen.getByRole("button", { name: "配置" })).toBeDisabled();

    fireEvent.click(screen.getByRole("button", { name: "同步" }));
    fireEvent.click(screen.getByRole("button", { name: "强制" }));

    expect(props.syncModelPrices).toHaveBeenNthCalledWith(1, false);
    expect(props.syncModelPrices).toHaveBeenNthCalledWith(2, true);
    expect(props.openModelPriceAliasesDialog).not.toHaveBeenCalled();
  });

  it("renders unavailable/error states and all relative time buckets", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-03-24T12:00:00Z"));
    const now = Date.now();

    const { rerender } = render(
      <SettingsDataSyncCard
        about={{ run_mode: "installed" } as any}
        modelPricesAvailable="unavailable"
        modelPricesCount={null}
        lastModelPricesSyncError="boom"
        lastModelPricesSyncReport={null}
        lastModelPricesSyncTime={now - 30_000}
        openModelPriceAliasesDialog={vi.fn()}
        todayRequestsAvailable="unavailable"
        todayRequestsTotal={null}
        syncingModelPrices={false}
        syncModelPrices={vi.fn().mockResolvedValue(undefined)}
      />
    );

    expect(screen.getAllByText("—").length).toBeGreaterThanOrEqual(2);
    expect(screen.getByText("失败")).toBeInTheDocument();
    expect(screen.getByText("刚刚 同步")).toBeInTheDocument();

    rerender(
      <SettingsDataSyncCard
        about={{ run_mode: "installed" } as any}
        modelPricesAvailable="unavailable"
        modelPricesCount={null}
        lastModelPricesSyncError="boom"
        lastModelPricesSyncReport={null}
        lastModelPricesSyncTime={now - 5 * 60_000}
        openModelPriceAliasesDialog={vi.fn()}
        todayRequestsAvailable="unavailable"
        todayRequestsTotal={null}
        syncingModelPrices={false}
        syncModelPrices={vi.fn().mockResolvedValue(undefined)}
      />
    );
    expect(screen.getByText("5 分钟前 同步")).toBeInTheDocument();

    rerender(
      <SettingsDataSyncCard
        about={{ run_mode: "installed" } as any}
        modelPricesAvailable="unavailable"
        modelPricesCount={null}
        lastModelPricesSyncError="boom"
        lastModelPricesSyncReport={null}
        lastModelPricesSyncTime={now - 2 * 60 * 60_000}
        openModelPriceAliasesDialog={vi.fn()}
        todayRequestsAvailable="unavailable"
        todayRequestsTotal={null}
        syncingModelPrices={false}
        syncModelPrices={vi.fn().mockResolvedValue(undefined)}
      />
    );
    expect(screen.getByText("2 小时前 同步")).toBeInTheDocument();

    rerender(
      <SettingsDataSyncCard
        about={{ run_mode: "installed" } as any}
        modelPricesAvailable="unavailable"
        modelPricesCount={null}
        lastModelPricesSyncError="boom"
        lastModelPricesSyncReport={null}
        lastModelPricesSyncTime={now - 3 * 24 * 60 * 60_000}
        openModelPriceAliasesDialog={vi.fn()}
        todayRequestsAvailable="unavailable"
        todayRequestsTotal={null}
        syncingModelPrices={false}
        syncModelPrices={vi.fn().mockResolvedValue(undefined)}
      />
    );
    expect(screen.getByText("3 天前 同步")).toBeInTheDocument();

    vi.useRealTimers();
  });

  it("renders synced counts, update summaries, and today request fallback values", () => {
    const { rerender } = render(
      <SettingsDataSyncCard
        about={{ run_mode: "installed" } as any}
        modelPricesAvailable="available"
        modelPricesCount={0}
        lastModelPricesSyncError={null}
        lastModelPricesSyncReport={{ status: "not_modified", inserted: 0, updated: 0 } as any}
        lastModelPricesSyncTime={null}
        openModelPriceAliasesDialog={vi.fn()}
        todayRequestsAvailable="available"
        todayRequestsTotal={null}
        syncingModelPrices={false}
        syncModelPrices={vi.fn().mockResolvedValue(undefined)}
      />
    );

    expect(screen.getByText("未同步")).toBeInTheDocument();
    expect(screen.getByText("最新")).toBeInTheDocument();
    expect(screen.getByText("0")).toBeInTheDocument();

    rerender(
      <SettingsDataSyncCard
        about={{ run_mode: "installed" } as any}
        modelPricesAvailable="available"
        modelPricesCount={12}
        lastModelPricesSyncError={null}
        lastModelPricesSyncReport={{ status: "updated", inserted: 3, updated: 4 } as any}
        lastModelPricesSyncTime={null}
        openModelPriceAliasesDialog={vi.fn()}
        todayRequestsAvailable="available"
        todayRequestsTotal={18}
        syncingModelPrices
        syncModelPrices={vi.fn().mockResolvedValue(undefined)}
      />
    );

    expect(screen.getByText("12 条")).toBeInTheDocument();
    expect(screen.getByText("+3 / ~4")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "同步中" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "强制" })).toBeDisabled();
    expect(screen.getByText("18")).toBeInTheDocument();
  });
});
