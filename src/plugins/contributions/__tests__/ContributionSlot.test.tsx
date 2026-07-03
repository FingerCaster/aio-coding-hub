import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { ActiveUiContribution } from "../../../generated/bindings";
import { ContributionSlot } from "../ContributionSlot";
import { useContributionsForSlot } from "../useActiveContributions";
import { usePluginExecuteCommandMutation } from "../../../query/plugins";

vi.mock("../useActiveContributions", () => ({
  useContributionsForSlot: vi.fn(),
}));

vi.mock("../../../query/plugins", () => ({
  usePluginExecuteCommandMutation: vi.fn(),
}));

function contribution(): ActiveUiContribution {
  return {
    pluginId: "acme.openrouter",
    contributionId: "openrouter-routing",
    providerExtensionNamespace: null,
    slotId: "providers.editor.sections",
    title: "OpenRouter 路由",
    order: 10,
    schema: {
      type: "section",
      fields: [
        { type: "text", key: "route", label: "路由策略" },
        { type: "button", key: "refresh", label: "刷新", command: "acme.openrouter.refresh" },
      ],
    },
  };
}

describe("plugins/contributions/ContributionSlot", () => {
  it("executes contribution button commands with slot context and current values", () => {
    const mutateAsync = vi.fn().mockResolvedValue({ ok: true });
    vi.mocked(useContributionsForSlot).mockReturnValue({
      contributions: [contribution()],
    } as any);
    vi.mocked(usePluginExecuteCommandMutation).mockReturnValue({
      mutateAsync,
      isPending: false,
    } as any);

    render(
      <ContributionSlot
        slotId="providers.editor.sections"
        valuesByContributionKey={{
          "acme.openrouter\u0000openrouter-routing": { route: "quality" },
        }}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: "刷新" }));

    expect(mutateAsync).toHaveBeenCalledWith({
      command: "acme.openrouter.refresh",
      args: {
        pluginId: "acme.openrouter",
        contributionId: "openrouter-routing",
        slotId: "providers.editor.sections",
        values: { route: "quality" },
      },
    });
  });
});
