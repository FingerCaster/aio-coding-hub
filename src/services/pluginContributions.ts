import { commands, type ActiveContributionSnapshot } from "../generated/bindings";
import { invokeGeneratedIpc } from "./generatedIpc";

export type { ActiveContributionSnapshot };

export async function pluginActiveContributions(): Promise<ActiveContributionSnapshot> {
  return invokeGeneratedIpc<ActiveContributionSnapshot>({
    title: "读取插件扩展点失败",
    cmd: "plugin_active_contributions",
    invoke: async () => commands.pluginActiveContributions(),
  });
}
