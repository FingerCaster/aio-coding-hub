// Usage: Compact official plugin entry plus advanced custom market source loader.

import { useMemo, useReducer } from "react";
import { ChevronDown, Download, RefreshCw } from "lucide-react";
import type { PluginMarketListing, PluginSummary } from "../../services/plugins";
import { pluginParseMarketIndex } from "../../services/plugins";
import { formatUnknownError } from "../../utils/errors";
import { Button } from "../../ui/Button";
import {
  buildFeaturedMarketCards,
  buildMarketListingCards,
  toMarketInstallInput,
  type MarketInstallInput,
  type PluginMarketCardView,
} from "./pluginMarketModel";

type PluginMarketState = {
  indexUrl: string;
  indexJson: string;
  signature: string;
  listings: PluginMarketListing[];
  advancedOpen: boolean;
  loading: boolean;
  error: string | null;
};

type PluginMarketAction =
  | { type: "setIndexUrl"; value: string }
  | { type: "setIndexJson"; value: string }
  | { type: "setSignature"; value: string }
  | { type: "toggleAdvancedOpen" }
  | { type: "loadStarted" }
  | { type: "loadSucceeded"; listings: PluginMarketListing[] }
  | { type: "loadFailed"; error: string };

const initialPluginMarketState: PluginMarketState = {
  indexUrl: "",
  indexJson: "",
  signature: "",
  listings: [],
  advancedOpen: false,
  loading: false,
  error: null,
};

function pluginMarketReducer(
  state: PluginMarketState,
  action: PluginMarketAction
): PluginMarketState {
  switch (action.type) {
    case "setIndexUrl":
      return { ...state, indexUrl: action.value };
    case "setIndexJson":
      return { ...state, indexJson: action.value };
    case "setSignature":
      return { ...state, signature: action.value };
    case "toggleAdvancedOpen":
      return { ...state, advancedOpen: !state.advancedOpen };
    case "loadStarted":
      return { ...state, loading: true, error: null };
    case "loadSucceeded":
      return { ...state, loading: false, listings: action.listings };
    case "loadFailed":
      return { ...state, loading: false, error: action.error };
  }
}

export function PluginMarketPanel({
  plugins,
  busy,
  onInstall,
  onUpdate,
  onInstallOfficial,
  onSelectInstalled,
}: {
  plugins: readonly PluginSummary[];
  busy: boolean;
  onInstall: (input: MarketInstallInput) => Promise<unknown>;
  onUpdate: (input: MarketInstallInput) => Promise<unknown>;
  onInstallOfficial: (pluginId: string) => Promise<unknown>;
  onSelectInstalled: (pluginId: string) => void;
}) {
  const [{ indexUrl, indexJson, signature, listings, advancedOpen, loading, error }, dispatch] =
    useReducer(pluginMarketReducer, initialPluginMarketState);
  const featuredCards = useMemo(() => buildFeaturedMarketCards(plugins), [plugins]);
  const officialCard = featuredCards.find((card) => card.pluginId === "official.privacy-filter");
  const listingCards = useMemo(
    () => buildMarketListingCards(plugins, listings),
    [plugins, listings]
  );

  async function handleLoadMarket() {
    dispatch({ type: "loadStarted" });
    try {
      const trimmedIndexUrl = indexUrl.trim();
      const trimmedSignature = signature.trim();
      const next = await pluginParseMarketIndex(
        indexJson,
        trimmedIndexUrl ? trimmedIndexUrl : null,
        trimmedSignature ? trimmedSignature : null
      );
      dispatch({ type: "loadSucceeded", listings: next });
    } catch (error) {
      dispatch({ type: "loadFailed", error: formatUnknownError(error) });
    }
  }

  async function handleCardAction(card: PluginMarketCardView, origin: "featured" | "advanced") {
    if (card.action === "installed") {
      onSelectInstalled(card.pluginId);
      return;
    }

    if (
      origin === "featured" &&
      card.pluginId === "official.privacy-filter" &&
      card.action === "install"
    ) {
      await onInstallOfficial(card.pluginId);
      return;
    }

    const input = toMarketInstallInput(card);
    if (input) {
      if (card.action === "update") {
        await onUpdate(input);
      } else {
        await onInstall(input);
      }
    }
  }

  return (
    <section className="space-y-3 rounded-lg border border-border bg-card p-3">
      {officialCard ? (
        <OfficialPluginCard
          card={officialCard}
          busy={busy}
          onAction={() => handleCardAction(officialCard, "featured")}
        />
      ) : null}

      <div className="space-y-3 border-t border-border pt-3">
        <Button
          size="sm"
          variant="secondary"
          onClick={() => dispatch({ type: "toggleAdvancedOpen" })}
          aria-expanded={advancedOpen}
        >
          <ChevronDown
            className={`h-3.5 w-3.5 transition-transform ${advancedOpen ? "rotate-180" : ""}`}
          />
          高级来源
        </Button>

        {advancedOpen ? (
          <div className="space-y-3">
            <div className="grid gap-2 sm:grid-cols-2">
              <label className="grid gap-1 text-xs text-muted-foreground">
                市场索引 URL
                <input
                  className="rounded-md border border-border bg-background px-2 py-1.5 text-sm text-foreground"
                  value={indexUrl}
                  onChange={(event) => dispatch({ type: "setIndexUrl", value: event.target.value })}
                  placeholder="https://plugins.example/index.json"
                />
              </label>
              <label className="grid gap-1 text-xs text-muted-foreground">
                索引签名
                <input
                  className="rounded-md border border-border bg-background px-2 py-1.5 text-sm text-foreground"
                  value={signature}
                  onChange={(event) =>
                    dispatch({ type: "setSignature", value: event.target.value })
                  }
                  placeholder="可选"
                />
              </label>
            </div>

            <label className="grid gap-1 text-xs text-muted-foreground">
              市场索引 JSON
              <textarea
                className="min-h-24 rounded-md border border-border bg-background px-2 py-1.5 font-mono text-xs text-foreground"
                value={indexJson}
                onChange={(event) => dispatch({ type: "setIndexJson", value: event.target.value })}
                placeholder='{"plugins":[]}'
              />
            </label>

            <Button
              size="sm"
              variant="secondary"
              disabled={loading || busy}
              onClick={handleLoadMarket}
            >
              {loading ? <RefreshCw className="h-3.5 w-3.5 animate-spin" /> : null}
              加载高级来源
            </Button>

            {error ? (
              <div className="rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-sm text-destructive">
                市场加载失败：{error}
              </div>
            ) : null}

            {listingCards.length === 0 ? (
              <div className="rounded-md border border-dashed border-border px-3 py-4 text-sm text-muted-foreground">
                暂无高级来源条目
              </div>
            ) : (
              <div className="grid gap-2">
                {listingCards.map((card) => (
                  <MarketCard
                    key={card.pluginId}
                    card={card}
                    busy={busy}
                    onAction={() => handleCardAction(card, "advanced")}
                  />
                ))}
              </div>
            )}
          </div>
        ) : null}
      </div>
    </section>
  );
}

function OfficialPluginCard({
  card,
  busy,
  onAction,
}: {
  card: PluginMarketCardView;
  busy: boolean;
  onAction: () => void;
}) {
  return (
    <article className="rounded-md border border-border bg-background/40 px-3 py-2">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div className="min-w-0">
          <div className="text-xs text-muted-foreground">官方插件</div>
          <div className="mt-0.5 flex flex-wrap items-baseline gap-x-2 gap-y-1">
            <div className="truncate text-sm font-semibold text-foreground">{card.name}</div>
            <div className="font-mono text-xs text-muted-foreground">{card.pluginId}</div>
          </div>
          <div className="mt-1 text-sm text-muted-foreground">{card.summary}</div>
        </div>
        <Button size="sm" disabled={busy} onClick={onAction}>
          {card.action === "install" ? <Download className="h-3.5 w-3.5" /> : null}
          {card.actionLabel}
        </Button>
      </div>
    </article>
  );
}

function MarketCard({
  card,
  busy,
  onAction,
}: {
  card: PluginMarketCardView;
  busy: boolean;
  onAction: () => void;
}) {
  const disabled = busy || card.action === "example" || card.action === "unavailable";

  return (
    <article className="rounded-md border border-border px-3 py-2">
      <div className="flex flex-wrap items-start justify-between gap-2">
        <div className="min-w-0">
          <div className="truncate text-sm font-medium text-foreground">{card.name}</div>
          <div className="font-mono text-xs text-muted-foreground">{card.pluginId}</div>
        </div>
        <Button size="sm" disabled={disabled} onClick={onAction}>
          {card.action === "install" || card.action === "update" ? (
            <Download className="h-3.5 w-3.5" />
          ) : null}
          {card.actionLabel}
        </Button>
      </div>

      <div className="mt-2 text-sm text-foreground">{card.summary}</div>
      <div className="mt-2 flex flex-wrap gap-2 text-xs text-muted-foreground">
        <span>{card.sourceLabel}</span>
        <span>{card.trustLabel}</span>
        <span>{card.category}</span>
        {card.latestVersion ? <span>版本 {card.latestVersion}</span> : null}
        {card.installedVersion ? <span>已安装 {card.installedVersion}</span> : null}
      </div>
      <div className="mt-2 flex flex-wrap gap-1">
        {card.riskLabel.split("、").map((label) => (
          <span
            key={label}
            className="rounded-md border border-border px-2 py-0.5 text-[11px] text-muted-foreground"
          >
            {label}
          </span>
        ))}
      </div>
      {card.disabledReason ? (
        <div className="mt-2 text-xs text-destructive">{card.disabledReason}</div>
      ) : null}
    </article>
  );
}
