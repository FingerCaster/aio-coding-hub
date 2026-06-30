# aio-coding-hub Plugin Marketplace Productization Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn the current developer-oriented plugin market index panel into a concise user-facing featured marketplace with advanced custom sources folded away.

**Architecture:** Keep all security and install decisions in the existing Rust host commands. Add a frontend-only market card view model that normalizes featured catalog entries and parsed market listings into one UI contract. Refactor the plugin page market surface into a default featured marketplace and a collapsed advanced source loader, without changing Plugin API v1, generated bindings, database schema, or runtime capabilities.

**Tech Stack:** React 19, TypeScript, TanStack Query, Vitest, Testing Library, existing Tauri command bindings, Markdown docs.

---

## Scope Boundaries

- Do not change Plugin API v1 manifest shape.
- Do not add Plugin API v2.
- Do not add Provider Plugin API.
- Do not add `plugin.storage`, `network.fetch`, `file.read`, `file.write`, or `secret.read`.
- Do not enable JS/TS/WebView/browser plugin runtimes.
- Do not enable marketplace WASM execution by default.
- Do not add backend DB tables or market source persistence in this phase.
- Do not add ratings, reviews, payments, accounts, recommendation algorithms, or remote marketplace administration.
- Do not bypass host install checks. GUI disables obvious invalid actions, but `plugin_install_remote` and `plugin_install_official` remain the real boundary.

## File Structure

- Create: `src/pages/plugins/pluginMarketModel.ts`
  - Own frontend-only featured catalog, market card state derivation, risk/trust/source labels, and action input shaping.
- Create: `src/pages/plugins/__tests__/pluginMarketModel.test.ts`
  - Unit tests for the market card view model.
- Modify: `src/pages/plugins/PluginMarketPanel.tsx`
  - Render featured plugin cards by default.
  - Keep custom market index URL/signature/JSON inside a collapsed advanced source section.
  - Reuse the same card component for featured and advanced listings.
- Modify: `src/pages/PluginsPage.tsx`
  - Pass installed plugin summaries, official install handler, market install handler, and select-installed handler into `PluginMarketPanel`.
  - Remove the separate example guidance section once featured cards carry that information.
- Modify: `src/pages/__tests__/PluginsPage.test.tsx`
  - Cover default featured market visibility, hidden advanced JSON fields, advanced source loading, official install action, market install action, installed state, revoked/incompatible blocks.
- Modify: `docs/plugins/developer-guide.md`
  - Explain that the default marketplace is a curated GUI entry, while custom market sources are advanced.
- Modify: `docs/plugins/reference/publishing.md`
  - Explain how market index entries appear in the simplified GUI and that host checks remain authoritative.
- Modify: `scripts/check-plugin-system-docs.mjs`
  - Add documentation anchors for “精选插件” and “高级来源” so the product boundary does not regress.

## Data Model

Use this exact frontend-only model in `src/pages/plugins/pluginMarketModel.ts`:

```ts
import type { PluginMarketListing, PluginSummary } from "../../services/plugins";

export type PluginMarketCardState =
  | "installable"
  | "installed"
  | "updateAvailable"
  | "incompatible"
  | "revoked"
  | "missingTrustData"
  | "exampleOnly";

export type PluginMarketCardAction = "install" | "update" | "installed" | "unavailable" | "example";

export type PluginFeaturedCatalogItem = {
  pluginId: string;
  name: string;
  summary: string;
  category: "privacy" | "prompt" | "safety" | "developer";
  source: "official" | "example" | "market";
  riskLabels: string[];
  listing?: PluginMarketListing;
};

export type PluginMarketCardView = {
  pluginId: string;
  name: string;
  summary: string;
  category: string;
  latestVersion: string | null;
  installedVersion: string | null;
  state: PluginMarketCardState;
  action: PluginMarketCardAction;
  actionLabel: string;
  disabledReason: string | null;
  riskLabel: string;
  trustLabel: string;
  sourceLabel: string;
  listing: PluginMarketListing | null;
};
```

Initial featured catalog:

```ts
export const FEATURED_PLUGIN_CATALOG: PluginFeaturedCatalogItem[] = [
  {
    pluginId: "official.privacy-filter",
    name: "Privacy Filter",
    summary: "发送前脱敏敏感信息，并在日志保存前做不可逆脱敏。",
    category: "privacy",
    source: "official",
    riskLabels: ["读取请求内容", "修改请求内容", "日志脱敏"],
  },
  {
    pluginId: "examples/prompt-helper",
    name: "Prompt Helper",
    summary: "示例：请求发送前补充提示词约束，覆盖 Claude 和 Codex 请求形态。",
    category: "prompt",
    source: "example",
    riskLabels: ["读取请求内容", "修改请求内容"],
  },
  {
    pluginId: "examples/redactor",
    name: "Redactor",
    summary: "示例：用声明式规则对请求和日志做脱敏。",
    category: "privacy",
    source: "example",
    riskLabels: ["读取请求内容", "修改请求内容", "日志脱敏"],
  },
  {
    pluginId: "examples/response-guard",
    name: "Response Guard",
    summary: "示例：响应返回前做轻量检查、告警或阻断。",
    category: "safety",
    source: "example",
    riskLabels: ["读取响应内容", "修改响应内容"],
  },
];
```

## Task 1: Add Market Card View Model

**Files:**
- Create: `src/pages/plugins/pluginMarketModel.ts`
- Create: `src/pages/plugins/__tests__/pluginMarketModel.test.ts`

- [ ] **Step 1: Write failing model tests**

Create `src/pages/plugins/__tests__/pluginMarketModel.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import type { PluginMarketListing, PluginSummary } from "../../../services/plugins";
import {
  FEATURED_PLUGIN_CATALOG,
  buildFeaturedMarketCards,
  buildMarketListingCards,
  toMarketInstallInput,
} from "../pluginMarketModel";

function summary(overrides: Partial<PluginSummary> = {}): PluginSummary {
  return {
    id: 1,
    plugin_id: "official.privacy-filter",
    name: "Privacy Filter",
    current_version: "1.0.0",
    status: "enabled",
    runtime: "native:privacyFilter",
    permission_risk: "high",
    update_available: false,
    last_error: null,
    created_at: 1,
    updated_at: 2,
    ...overrides,
  };
}

function listing(overrides: Partial<PluginMarketListing> = {}): PluginMarketListing {
  return {
    pluginId: "community.safe-helper",
    name: "Safe Helper",
    latestVersion: "1.0.0",
    downloadUrl: "https://plugins.example.test/safe-helper.aio-plugin",
    checksum: "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    signature: "signed-safe",
    riskLabels: ["request.body.read"],
    revoked: false,
    compatible: true,
    updateAvailable: false,
    installBlockReason: null,
    ...overrides,
  };
}

describe("pluginMarketModel", () => {
  it("builds featured cards without requiring market index input", () => {
    const cards = buildFeaturedMarketCards([], FEATURED_PLUGIN_CATALOG);

    expect(cards.map((card) => card.pluginId)).toEqual([
      "official.privacy-filter",
      "examples/prompt-helper",
      "examples/redactor",
      "examples/response-guard",
    ]);
    expect(cards[0]).toMatchObject({
      state: "installable",
      action: "install",
      actionLabel: "安装",
      sourceLabel: "官方来源",
    });
    expect(cards[1]).toMatchObject({
      state: "exampleOnly",
      action: "example",
      actionLabel: "示例",
      disabledReason: "示例插件暂未发布为可安装包",
    });
  });

  it("marks featured official plugins as installed when versions are present", () => {
    const cards = buildFeaturedMarketCards([summary()], FEATURED_PLUGIN_CATALOG);
    const privacyFilter = cards.find((card) => card.pluginId === "official.privacy-filter");

    expect(privacyFilter).toMatchObject({
      installedVersion: "1.0.0",
      state: "installed",
      action: "installed",
      actionLabel: "已安装",
    });
  });

  it("maps parsed market listings to concise install states", () => {
    const cards = buildMarketListingCards([], [
      listing(),
      listing({
        pluginId: "community.revoked",
        name: "Revoked Helper",
        revoked: true,
        compatible: false,
        installBlockReason: "revoked",
      }),
      listing({
        pluginId: "community.future",
        name: "Future Helper",
        compatible: false,
        installBlockReason: "incompatible",
      }),
      listing({
        pluginId: "community.missing",
        name: "Missing Trust",
        checksum: null,
      }),
    ]);

    expect(cards.map((card) => [card.pluginId, card.state, card.actionLabel])).toEqual([
      ["community.safe-helper", "installable", "安装"],
      ["community.revoked", "revoked", "已撤销"],
      ["community.future", "incompatible", "不兼容"],
      ["community.missing", "missingTrustData", "不可安装"],
    ]);
    expect(cards[1].disabledReason).toBe("插件已被市场撤销");
    expect(cards[2].disabledReason).toBe("当前宿主版本不兼容");
    expect(cards[3].disabledReason).toBe("缺少下载地址或校验信息");
  });

  it("marks parsed market listings as updateable when installed and update is available", () => {
    const cards = buildMarketListingCards(
      [summary({ plugin_id: "community.safe-helper", current_version: "0.9.0" })],
      [listing({ updateAvailable: true, latestVersion: "1.0.0" })]
    );

    expect(cards[0]).toMatchObject({
      installedVersion: "0.9.0",
      state: "updateAvailable",
      action: "update",
      actionLabel: "更新",
    });
  });

  it("creates remote install input only for installable market cards", () => {
    const [card] = buildMarketListingCards([], [listing()]);

    expect(toMarketInstallInput(card)).toEqual({
      pluginId: "community.safe-helper",
      downloadUrl: "https://plugins.example.test/safe-helper.aio-plugin",
      checksum: "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
      signature: "signed-safe",
      publicKey: null,
      source: "market",
    });

    const [revoked] = buildMarketListingCards([], [listing({ revoked: true })]);
    expect(toMarketInstallInput(revoked)).toBeNull();
  });
});
```

- [ ] **Step 2: Run the model test and verify it fails**

Run:

```bash
pnpm test:unit src/pages/plugins/__tests__/pluginMarketModel.test.ts
```

Expected: FAIL because `pluginMarketModel.ts` does not exist.

- [ ] **Step 3: Implement the market model**

Create `src/pages/plugins/pluginMarketModel.ts`:

```ts
import type { PluginMarketListing, PluginSummary } from "../../services/plugins";

export type PluginMarketCardState =
  | "installable"
  | "installed"
  | "updateAvailable"
  | "incompatible"
  | "revoked"
  | "missingTrustData"
  | "exampleOnly";

export type PluginMarketCardAction = "install" | "update" | "installed" | "unavailable" | "example";

export type PluginFeaturedCatalogItem = {
  pluginId: string;
  name: string;
  summary: string;
  category: "privacy" | "prompt" | "safety" | "developer";
  source: "official" | "example" | "market";
  riskLabels: string[];
  listing?: PluginMarketListing;
};

export type PluginMarketCardView = {
  pluginId: string;
  name: string;
  summary: string;
  category: string;
  latestVersion: string | null;
  installedVersion: string | null;
  state: PluginMarketCardState;
  action: PluginMarketCardAction;
  actionLabel: string;
  disabledReason: string | null;
  riskLabel: string;
  trustLabel: string;
  sourceLabel: string;
  listing: PluginMarketListing | null;
};

export type MarketInstallInput = {
  pluginId: string;
  downloadUrl: string;
  checksum: string;
  signature?: string | null;
  publicKey?: string | null;
  source: "market";
};

export const FEATURED_PLUGIN_CATALOG: PluginFeaturedCatalogItem[] = [
  {
    pluginId: "official.privacy-filter",
    name: "Privacy Filter",
    summary: "发送前脱敏敏感信息，并在日志保存前做不可逆脱敏。",
    category: "privacy",
    source: "official",
    riskLabels: ["读取请求内容", "修改请求内容", "日志脱敏"],
  },
  {
    pluginId: "examples/prompt-helper",
    name: "Prompt Helper",
    summary: "示例：请求发送前补充提示词约束，覆盖 Claude 和 Codex 请求形态。",
    category: "prompt",
    source: "example",
    riskLabels: ["读取请求内容", "修改请求内容"],
  },
  {
    pluginId: "examples/redactor",
    name: "Redactor",
    summary: "示例：用声明式规则对请求和日志做脱敏。",
    category: "privacy",
    source: "example",
    riskLabels: ["读取请求内容", "修改请求内容", "日志脱敏"],
  },
  {
    pluginId: "examples/response-guard",
    name: "Response Guard",
    summary: "示例：响应返回前做轻量检查、告警或阻断。",
    category: "safety",
    source: "example",
    riskLabels: ["读取响应内容", "修改响应内容"],
  },
];

function installedById(installed: readonly PluginSummary[]) {
  return new Map(installed.map((plugin) => [plugin.plugin_id, plugin]));
}

function riskLabel(labels: readonly string[]) {
  if (labels.length === 0) return "低风险";
  if (labels.some((label) => label.includes("write") || label.includes("修改"))) return "高风险";
  if (labels.some((label) => label.includes("read") || label.includes("读取"))) return "中风险";
  return "低风险";
}

function sourceLabel(source: PluginFeaturedCatalogItem["source"] | "custom") {
  if (source === "official") return "官方来源";
  if (source === "example") return "示例";
  if (source === "market") return "市场来源";
  return "自定义来源";
}

function trustLabel(listing: PluginMarketListing | null, source: PluginFeaturedCatalogItem["source"] | "custom") {
  if (source === "official") return "官方来源";
  if (source === "example") return "示例未发布";
  if (!listing) return "未签名";
  return listing.signature ? "已提供签名" : "未签名";
}

function installedVersion(plugin: PluginSummary | undefined) {
  return plugin?.current_version ?? null;
}

function listingState(
  listing: PluginMarketListing,
  installed: PluginSummary | undefined
): Pick<PluginMarketCardView, "state" | "action" | "actionLabel" | "disabledReason"> {
  if (listing.revoked) {
    return {
      state: "revoked",
      action: "unavailable",
      actionLabel: "已撤销",
      disabledReason: "插件已被市场撤销",
    };
  }
  if (!listing.compatible || listing.installBlockReason === "incompatible") {
    return {
      state: "incompatible",
      action: "unavailable",
      actionLabel: "不兼容",
      disabledReason: "当前宿主版本不兼容",
    };
  }
  if (!listing.downloadUrl || !listing.checksum) {
    return {
      state: "missingTrustData",
      action: "unavailable",
      actionLabel: "不可安装",
      disabledReason: "缺少下载地址或校验信息",
    };
  }
  if (installed && listing.updateAvailable) {
    return {
      state: "updateAvailable",
      action: "update",
      actionLabel: "更新",
      disabledReason: null,
    };
  }
  if (installed) {
    return {
      state: "installed",
      action: "installed",
      actionLabel: "已安装",
      disabledReason: null,
    };
  }
  return {
    state: "installable",
    action: "install",
    actionLabel: "安装",
    disabledReason: null,
  };
}

export function buildFeaturedMarketCards(
  installed: readonly PluginSummary[],
  catalog: readonly PluginFeaturedCatalogItem[] = FEATURED_PLUGIN_CATALOG
): PluginMarketCardView[] {
  const installedMap = installedById(installed);
  return catalog.map((item) => {
    const installedPlugin = installedMap.get(item.pluginId);
    if (item.source === "example" && !item.listing) {
      return {
        pluginId: item.pluginId,
        name: item.name,
        summary: item.summary,
        category: item.category,
        latestVersion: null,
        installedVersion: installedVersion(installedPlugin),
        state: "exampleOnly",
        action: "example",
        actionLabel: "示例",
        disabledReason: "示例插件暂未发布为可安装包",
        riskLabel: riskLabel(item.riskLabels),
        trustLabel: trustLabel(null, item.source),
        sourceLabel: sourceLabel(item.source),
        listing: null,
      };
    }

    if (item.listing) {
      return marketListingCard(item.listing, installedPlugin, item.summary, item.category, item.source);
    }

    return {
      pluginId: item.pluginId,
      name: item.name,
      summary: item.summary,
      category: item.category,
      latestVersion: installedPlugin?.current_version ?? null,
      installedVersion: installedVersion(installedPlugin),
      state: installedPlugin ? "installed" : "installable",
      action: installedPlugin ? "installed" : "install",
      actionLabel: installedPlugin ? "已安装" : "安装",
      disabledReason: null,
      riskLabel: riskLabel(item.riskLabels),
      trustLabel: trustLabel(null, item.source),
      sourceLabel: sourceLabel(item.source),
      listing: null,
    };
  });
}

function marketListingCard(
  listing: PluginMarketListing,
  installed: PluginSummary | undefined,
  summary: string,
  category: string,
  source: PluginFeaturedCatalogItem["source"] | "custom"
): PluginMarketCardView {
  const state = listingState(listing, installed);
  return {
    pluginId: listing.pluginId,
    name: listing.name,
    summary,
    category,
    latestVersion: listing.latestVersion,
    installedVersion: installedVersion(installed),
    ...state,
    riskLabel: riskLabel(listing.riskLabels),
    trustLabel: trustLabel(listing, source),
    sourceLabel: sourceLabel(source),
    listing,
  };
}

export function buildMarketListingCards(
  installed: readonly PluginSummary[],
  listings: readonly PluginMarketListing[]
): PluginMarketCardView[] {
  const installedMap = installedById(installed);
  return listings.map((listing) =>
    marketListingCard(
      listing,
      installedMap.get(listing.pluginId),
      "来自自定义市场源的插件。",
      "developer",
      "custom"
    )
  );
}

export function toMarketInstallInput(card: PluginMarketCardView): MarketInstallInput | null {
  if ((card.action !== "install" && card.action !== "update") || !card.listing) return null;
  if (!card.listing.downloadUrl || !card.listing.checksum) return null;
  return {
    pluginId: card.pluginId,
    downloadUrl: card.listing.downloadUrl,
    checksum: card.listing.checksum,
    signature: card.listing.signature,
    publicKey: null,
    source: "market",
  };
}
```

- [ ] **Step 4: Run the model test and verify it passes**

Run:

```bash
pnpm test:unit src/pages/plugins/__tests__/pluginMarketModel.test.ts
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/pages/plugins/pluginMarketModel.ts src/pages/plugins/__tests__/pluginMarketModel.test.ts
git commit -m "feat(plugins): add market card view model"
```

## Task 2: Refactor Marketplace UI Into Featured And Advanced Sections

**Files:**
- Modify: `src/pages/plugins/PluginMarketPanel.tsx`
- Modify: `src/pages/PluginsPage.tsx`
- Modify: `src/pages/__tests__/PluginsPage.test.tsx`

- [ ] **Step 1: Write failing page tests for the new marketplace UX**

In `src/pages/__tests__/PluginsPage.test.tsx`, replace the current market test with these tests. Keep existing imports and mocks for `pluginParseMarketIndex`, `usePluginInstallRemoteMutation`, and `usePluginInstallOfficialMutation`.

```tsx
it("shows featured plugins by default without exposing advanced market source fields", () => {
  vi.mocked(usePluginsListQuery).mockReturnValue({
    data: [summary()],
    isLoading: false,
    isFetching: false,
    error: null,
  } as any);

  renderWithProviders(<PluginsPage />);

  expect(screen.getByText("精选插件")).toBeInTheDocument();
  expect(screen.getByText("Privacy Filter")).toBeInTheDocument();
  expect(screen.getByText("Prompt Helper")).toBeInTheDocument();
  expect(screen.getByText("Redactor")).toBeInTheDocument();
  expect(screen.getByText("Response Guard")).toBeInTheDocument();
  expect(screen.queryByLabelText("市场索引 JSON")).not.toBeInTheDocument();
  expect(screen.queryByLabelText("市场索引 URL")).not.toBeInTheDocument();
});

it("installs the official featured privacy filter through the official install path", async () => {
  const installOfficialMutation = mutation();
  vi.mocked(usePluginInstallOfficialMutation).mockReturnValue(installOfficialMutation as any);
  vi.mocked(usePluginsListQuery).mockReturnValue({
    data: [],
    isLoading: false,
    isFetching: false,
    error: null,
  } as any);

  renderWithProviders(<PluginsPage />);

  const privacyCard = screen.getByText("Privacy Filter").closest("article");
  expect(privacyCard).not.toBeNull();
  fireEvent.click(within(privacyCard as HTMLElement).getByRole("button", { name: "安装" }));

  await waitFor(() => {
    expect(installOfficialMutation.mutateAsync).toHaveBeenCalledWith("official.privacy-filter");
    expect(toast.success).toHaveBeenCalledWith("安装官方插件成功");
  });
});

it("marks featured examples as example-only instead of pretending they can be installed", () => {
  vi.mocked(usePluginsListQuery).mockReturnValue({
    data: [],
    isLoading: false,
    isFetching: false,
    error: null,
  } as any);

  renderWithProviders(<PluginsPage />);

  const promptHelperCard = screen.getByText("Prompt Helper").closest("article");
  expect(promptHelperCard).not.toBeNull();
  expect(within(promptHelperCard as HTMLElement).getByRole("button", { name: "示例" })).toBeDisabled();
  expect(screen.getAllByText("示例插件暂未发布为可安装包").length).toBeGreaterThan(0);
});

it("keeps advanced market source fields collapsed until requested", async () => {
  vi.mocked(usePluginsListQuery).mockReturnValue({
    data: [summary()],
    isLoading: false,
    isFetching: false,
    error: null,
  } as any);

  renderWithProviders(<PluginsPage />);
  fireEvent.click(screen.getByRole("button", { name: "高级来源" }));

  expect(screen.getByLabelText("市场索引 JSON")).toBeInTheDocument();
  expect(screen.getByLabelText("市场索引 URL")).toBeInTheDocument();
  expect(screen.getByLabelText("索引签名")).toBeInTheDocument();
});

it("loads advanced source listings with the same card states and install action", async () => {
  const installRemoteMutation = mutation();
  vi.mocked(usePluginInstallRemoteMutation).mockReturnValue(installRemoteMutation as any);
  vi.mocked(pluginParseMarketIndex).mockResolvedValue([
    {
      pluginId: "community.safe-helper",
      name: "Safe Helper",
      latestVersion: "1.0.0",
      downloadUrl: "https://plugins.example.test/safe-helper.aio-plugin",
      checksum: "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
      signature: "signed-safe",
      riskLabels: ["request.body.read"],
      revoked: false,
      compatible: true,
      updateAvailable: false,
      installBlockReason: null,
    },
    {
      pluginId: "community.revoked",
      name: "Revoked Helper",
      latestVersion: "1.0.0",
      downloadUrl: "https://plugins.example.test/revoked.aio-plugin",
      checksum: "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
      signature: null,
      riskLabels: ["request.body.write"],
      revoked: true,
      compatible: false,
      updateAvailable: false,
      installBlockReason: "revoked",
    },
    {
      pluginId: "community.future",
      name: "Future Helper",
      latestVersion: "2.0.0",
      downloadUrl: "https://plugins.example.test/future.aio-plugin",
      checksum: "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
      signature: null,
      riskLabels: ["response.body.write"],
      revoked: false,
      compatible: false,
      updateAvailable: false,
      installBlockReason: "incompatible",
    },
  ]);
  vi.mocked(usePluginsListQuery).mockReturnValue({
    data: [summary()],
    isLoading: false,
    isFetching: false,
    error: null,
  } as any);

  renderWithProviders(<PluginsPage />);
  fireEvent.click(screen.getByRole("button", { name: "高级来源" }));
  fireEvent.change(screen.getByLabelText("市场索引 JSON"), {
    target: { value: '{"plugins":[]}' },
  });
  fireEvent.change(screen.getByLabelText("市场索引 URL"), {
    target: { value: "https://plugins.example.test/index.json" },
  });
  fireEvent.click(screen.getByRole("button", { name: "加载高级来源" }));

  const safeCard = await screen.findByText("Safe Helper");
  const revokedCard = screen.getByText("Revoked Helper").closest("article");
  const futureCard = screen.getByText("Future Helper").closest("article");

  expect(screen.getByText("插件已被市场撤销")).toBeInTheDocument();
  expect(screen.getByText("当前宿主版本不兼容")).toBeInTheDocument();
  expect(within(revokedCard as HTMLElement).getByRole("button", { name: "已撤销" })).toBeDisabled();
  expect(within(futureCard as HTMLElement).getByRole("button", { name: "不兼容" })).toBeDisabled();

  fireEvent.click(within(safeCard.closest("article") as HTMLElement).getByRole("button", { name: "安装" }));

  await waitFor(() => {
    expect(installRemoteMutation.mutateAsync).toHaveBeenCalledWith({
      pluginId: "community.safe-helper",
      downloadUrl: "https://plugins.example.test/safe-helper.aio-plugin",
      checksum: "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
      signature: "signed-safe",
      publicKey: null,
      source: "market",
    });
    expect(toast.success).toHaveBeenCalledWith("安装市场插件成功");
  });
});
```

- [ ] **Step 2: Run the page tests and verify they fail**

Run:

```bash
pnpm test:unit src/pages/__tests__/PluginsPage.test.tsx
```

Expected: FAIL because the current market panel exposes advanced fields by default and does not render featured cards with these states.

- [ ] **Step 3: Refactor `PluginMarketPanel.tsx`**

Replace the current implementation of `src/pages/plugins/PluginMarketPanel.tsx` with this component:

```tsx
// Usage: Productized plugin marketplace with featured cards and folded advanced sources.

import { useMemo, useState } from "react";
import { ChevronDown, ChevronRight, Download, RefreshCw } from "lucide-react";
import type { PluginMarketListing, PluginSummary } from "../../services/plugins";
import { pluginParseMarketIndex } from "../../services/plugins";
import { formatUnknownError } from "../../utils/errors";
import { Button } from "../../ui/Button";
import {
  buildFeaturedMarketCards,
  buildMarketListingCards,
  type MarketInstallInput,
  type PluginMarketCardView,
  toMarketInstallInput,
} from "./pluginMarketModel";

function MarketCard({
  card,
  busy,
  onOfficialInstall,
  onMarketInstall,
  onSelectInstalled,
}: {
  card: PluginMarketCardView;
  busy: boolean;
  onOfficialInstall: (pluginId: string) => Promise<unknown>;
  onMarketInstall: (input: MarketInstallInput) => Promise<unknown>;
  onSelectInstalled: (pluginId: string) => void;
}) {
  const disabled =
    busy ||
    card.action === "example" ||
    card.action === "unavailable" ||
    (card.action === "installed" && !card.installedVersion);

  async function handleAction() {
    if (card.action === "installed") {
      onSelectInstalled(card.pluginId);
      return;
    }
    if (card.pluginId === "official.privacy-filter" && card.action === "install") {
      await onOfficialInstall(card.pluginId);
      return;
    }
    const input = toMarketInstallInput(card);
    if (!input) return;
    await onMarketInstall(input);
  }

  return (
    <article className="rounded-md border border-border bg-card px-3 py-3">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            <h3 className="text-sm font-semibold text-foreground">{card.name}</h3>
            <span className="rounded-md border border-border px-2 py-0.5 text-xs text-muted-foreground">
              {card.actionLabel}
            </span>
          </div>
          <div className="mt-1 text-sm text-muted-foreground">{card.summary}</div>
        </div>
        <Button size="sm" disabled={disabled} onClick={() => void handleAction()}>
          <Download className="h-3.5 w-3.5" />
          {card.actionLabel}
        </Button>
      </div>

      <div className="mt-3 flex flex-wrap gap-2 text-xs text-muted-foreground">
        <span>{card.sourceLabel}</span>
        <span>{card.riskLabel}</span>
        <span>{card.trustLabel}</span>
        <span>版本 {card.latestVersion ?? card.installedVersion ?? "-"}</span>
      </div>

      <div className="mt-2 font-mono text-[11px] text-muted-foreground">{card.pluginId}</div>

      {card.disabledReason ? (
        <div className="mt-2 text-xs text-destructive">{card.disabledReason}</div>
      ) : null}
    </article>
  );
}

export function PluginMarketPanel({
  plugins,
  busy,
  onInstall,
  onInstallOfficial,
  onSelectInstalled,
}: {
  plugins: readonly PluginSummary[];
  busy: boolean;
  onInstall: (input: MarketInstallInput) => Promise<unknown>;
  onInstallOfficial: (pluginId: string) => Promise<unknown>;
  onSelectInstalled: (pluginId: string) => void;
}) {
  const [advancedOpen, setAdvancedOpen] = useState(false);
  const [indexUrl, setIndexUrl] = useState("");
  const [indexJson, setIndexJson] = useState("");
  const [signature, setSignature] = useState("");
  const [listings, setListings] = useState<PluginMarketListing[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const featuredCards = useMemo(() => buildFeaturedMarketCards(plugins), [plugins]);
  const advancedCards = useMemo(() => buildMarketListingCards(plugins, listings), [plugins, listings]);

  async function handleLoadMarket() {
    setLoading(true);
    setError(null);
    try {
      const next = await pluginParseMarketIndex(
        indexJson,
        indexUrl.trim() ? indexUrl : null,
        signature.trim() ? signature : null
      );
      setListings(next);
    } catch (error) {
      setError(formatUnknownError(error));
    } finally {
      setLoading(false);
    }
  }

  return (
    <section className="space-y-3 rounded-lg border border-border bg-card p-3">
      <div>
        <h2 className="text-sm font-semibold text-foreground">精选插件</h2>
        <div className="text-xs text-muted-foreground">
          直接安装官方插件，或查看推荐社区插件方向。
        </div>
      </div>

      <div className="grid gap-2 md:grid-cols-2">
        {featuredCards.map((card) => (
          <MarketCard
            key={card.pluginId}
            card={card}
            busy={busy}
            onOfficialInstall={onInstallOfficial}
            onMarketInstall={onInstall}
            onSelectInstalled={onSelectInstalled}
          />
        ))}
      </div>

      <div className="border-t border-border pt-3">
        <Button
          type="button"
          size="sm"
          variant="ghost"
          className="px-0"
          onClick={() => setAdvancedOpen((open) => !open)}
        >
          {advancedOpen ? <ChevronDown className="h-3.5 w-3.5" /> : <ChevronRight className="h-3.5 w-3.5" />}
          高级来源
        </Button>

        {advancedOpen ? (
          <div className="mt-3 space-y-3">
            <div className="grid gap-2 sm:grid-cols-2">
              <label className="grid gap-1 text-xs text-muted-foreground">
                市场索引 URL
                <input
                  className="rounded-md border border-border bg-background px-2 py-1.5 text-sm text-foreground"
                  value={indexUrl}
                  onChange={(event) => setIndexUrl(event.target.value)}
                  placeholder="https://plugins.example/index.json"
                />
              </label>
              <label className="grid gap-1 text-xs text-muted-foreground">
                索引签名
                <input
                  className="rounded-md border border-border bg-background px-2 py-1.5 text-sm text-foreground"
                  value={signature}
                  onChange={(event) => setSignature(event.target.value)}
                  placeholder="可选"
                />
              </label>
            </div>

            <label className="grid gap-1 text-xs text-muted-foreground">
              市场索引 JSON
              <textarea
                className="min-h-24 rounded-md border border-border bg-background px-2 py-1.5 font-mono text-xs text-foreground"
                value={indexJson}
                onChange={(event) => setIndexJson(event.target.value)}
                placeholder='{"plugins":[]}'
              />
            </label>

            <Button size="sm" variant="secondary" disabled={loading || busy} onClick={handleLoadMarket}>
              {loading ? <RefreshCw className="h-3.5 w-3.5 animate-spin" /> : null}
              加载高级来源
            </Button>

            {error ? (
              <div className="rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-sm text-destructive">
                市场加载失败：{error}
              </div>
            ) : null}

            {advancedCards.length > 0 ? (
              <div className="grid gap-2 md:grid-cols-2">
                {advancedCards.map((card) => (
                  <MarketCard
                    key={card.pluginId}
                    card={card}
                    busy={busy}
                    onOfficialInstall={onInstallOfficial}
                    onMarketInstall={onInstall}
                    onSelectInstalled={onSelectInstalled}
                  />
                ))}
              </div>
            ) : (
              <div className="rounded-md border border-dashed border-border px-3 py-4 text-sm text-muted-foreground">
                暂无高级来源条目
              </div>
            )}
          </div>
        ) : null}
      </div>
    </section>
  );
}
```

- [ ] **Step 4: Wire `PluginsPage.tsx` into the new panel props**

Update the `PluginMarketPanel` usage in `src/pages/PluginsPage.tsx`:

```tsx
<PluginMarketPanel
  plugins={plugins}
  busy={busy}
  onInstall={(input) =>
    runAction("安装市场插件", () => installRemoteMutation.mutateAsync(input))
  }
  onInstallOfficial={(pluginId) =>
    runAction("安装官方插件", () => installOfficialMutation.mutateAsync(pluginId))
  }
  onSelectInstalled={(pluginId) => setSelectedPluginId(pluginId)}
/>
```

Remove the separate `EXAMPLE_PLUGINS` constant and the adjacent “示例插件” section from `PluginsPage.tsx`, because featured market cards now carry that content.

- [ ] **Step 5: Run page tests and fix only mechanical TypeScript issues**

Run:

```bash
pnpm test:unit src/pages/__tests__/PluginsPage.test.tsx src/pages/plugins/__tests__/pluginMarketModel.test.ts
```

Expected: PASS after mechanical import/type fixes. Do not change the intended product behavior to satisfy old assertions.

- [ ] **Step 6: Run typecheck and lint**

Run:

```bash
pnpm typecheck
pnpm lint
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add src/pages/plugins/PluginMarketPanel.tsx src/pages/PluginsPage.tsx src/pages/__tests__/PluginsPage.test.tsx
git commit -m "feat(plugins): productize marketplace panel"
```

## Task 3: Sync Marketplace Product Documentation

**Files:**
- Modify: `docs/plugins/developer-guide.md`
- Modify: `docs/plugins/reference/publishing.md`
- Modify: `scripts/check-plugin-system-docs.mjs`

- [ ] **Step 1: Write failing docs contract checks**

Modify the `docs/plugins/developer-guide.md` entry in `scripts/check-plugin-system-docs.mjs` to require:

```js
"精选插件",
"高级来源",
```

Modify the `docs/plugins/reference/publishing.md` entry to require:

```js
"默认市场视图",
"自定义 market index 属于高级来源",
```

- [ ] **Step 2: Run docs checks and verify they fail**

Run:

```bash
pnpm check:plugin-system-docs
```

Expected: FAIL because the docs do not yet contain the new product boundary phrases.

- [ ] **Step 3: Update developer guide**

In `docs/plugins/developer-guide.md`, add this subsection after the quick-start install paragraph:

```md
## 插件市场入口

Plugins 页面默认展示“精选插件”，面向普通用户提供简洁安装入口。用户不需要理解 market index JSON、signature 或 trusted public key，就可以看到官方 Privacy Filter 和推荐社区示例方向。

“高级来源”用于插件开发者或自定义源用户。它保留 market index URL、index JSON 和索引签名输入，但默认折叠。高级来源加载出的条目仍然走同一套安装卡片和宿主安装校验。
```

- [ ] **Step 4: Update publishing reference**

In `docs/plugins/reference/publishing.md`, add this paragraph to the `Market Index` section:

```md
默认市场视图会把精选插件展示成用户可读卡片，只显示用途、版本、风险、信任和安装状态。完整 checksum、signature 和 raw index fields 不应占据默认视图。

自定义 market index 属于高级来源。高级来源可以加载临时 URL 或粘贴 JSON，但它只是发布者和高级用户入口；真实安装仍由宿主重新执行 checksum、signature、compatibility、runtime policy 和 revoked checks。
```

- [ ] **Step 5: Run docs checks**

Run:

```bash
pnpm check:plugin-system-docs
pnpm check:spec-links
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add docs/plugins/developer-guide.md docs/plugins/reference/publishing.md scripts/check-plugin-system-docs.mjs
git commit -m "docs(plugins): document marketplace product boundary"
```

## Final Verification

Run all commands:

```bash
pnpm test:unit src/pages/plugins/__tests__/pluginMarketModel.test.ts src/pages/__tests__/PluginsPage.test.tsx
pnpm test:unit src/services/__tests__/plugins.test.ts src/query/__tests__/plugins.test.tsx src/pages/plugins/__tests__/pluginProductCopy.test.ts
pnpm check:plugin-system-docs
pnpm check:spec-links
pnpm typecheck
pnpm lint
```

Expected: all commands pass.

## Acceptance Checklist

- [ ] Plugins page shows featured plugins without requiring market JSON input.
- [ ] Default marketplace view does not expose market index JSON, signature text, or full checksum.
- [ ] Featured cards show one-sentence purpose, state, risk/trust summary, and clear primary action.
- [ ] Installed, updateable, unavailable, revoked, incompatible, missing trust data, and example-only states are represented in the view model.
- [ ] Advanced source is collapsed by default and still supports URL/signature/JSON parsing.
- [ ] Advanced listings reuse the same card component and install logic as featured cards.
- [ ] Official install uses `usePluginInstallOfficialMutation`.
- [ ] Market install/update uses `usePluginInstallRemoteMutation`.
- [ ] No Plugin API v1, generated binding, Rust DTO, database schema, or runtime capability changes.
- [ ] Docs explain the default curated marketplace and advanced custom source boundary.
