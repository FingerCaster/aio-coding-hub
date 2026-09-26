import { resolveNativeChannelRoute } from "../../services/gateway/nativeChannelRoute";

export function NativeChannelRouteBadge({
  cliKey,
  specialSettingsJson,
}: {
  cliKey: string;
  specialSettingsJson: string | null | undefined;
}) {
  const route = resolveNativeChannelRoute(cliKey, specialSettingsJson);
  if (!route) return null;
  return (
    <span
      className="inline-flex shrink-0 items-center rounded-md border border-border/60 bg-muted/50 px-2 py-0.5 text-[11px] font-medium text-foreground"
      title={`来源由 AIO 管理；统计归属 ${route.consumerLabel}。绑定：${route.bindingId}`}
    >
      {route.consumerLabel} → {route.sourceLabel}
    </span>
  );
}
