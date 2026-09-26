import { parseRequestLogSpecialSettings } from "./requestLogSpecialSettings";

const SOURCE_LABELS: Record<string, string> = {
  claude: "Claude Code",
  codex: "Codex",
  grok: "Grok",
  gemini: "Gemini",
};

export function resolveNativeChannelRoute(
  cliKey: string,
  specialSettingsJson: string | null | undefined
): { consumerLabel: string; sourceLabel: string; bindingId: string } | null {
  if (cliKey !== "pi" && cliKey !== "omp") return null;
  const marker = parseRequestLogSpecialSettings(specialSettingsJson).find(
    (setting) =>
      setting.type === "channel_binding" &&
      setting.scope === "request" &&
      setting.consumerCli === cliKey &&
      typeof setting.sourceChannel === "string" &&
      Object.prototype.hasOwnProperty.call(SOURCE_LABELS, setting.sourceChannel) &&
      typeof setting.bindingId === "string" &&
      /^[a-f0-9]{64}$/.test(setting.bindingId)
  );
  if (!marker) return null;
  return {
    consumerLabel: cliKey === "pi" ? "Pi" : "OMP",
    sourceLabel: SOURCE_LABELS[marker.sourceChannel as string],
    bindingId: marker.bindingId as string,
  };
}
