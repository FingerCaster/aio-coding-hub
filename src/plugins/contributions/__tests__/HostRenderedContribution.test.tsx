import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { ActiveUiContribution } from "../../../generated/bindings";
import { HostRenderedContribution } from "../HostRenderedContribution";

function makeContribution(partial: Partial<ActiveUiContribution> = {}): ActiveUiContribution {
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
        {
          type: "text",
          key: "route",
          label: "路由策略",
          placeholder: "quality",
        },
        {
          type: "boolean",
          key: "fallbackEnabled",
          label: "启用兜底",
        },
      ],
    },
    ...partial,
  };
}

describe("plugins/contributions/HostRenderedContribution", () => {
  it("renders text and boolean fields and reports changed values by key", () => {
    const onChange = vi.fn();

    render(
      <HostRenderedContribution
        contribution={makeContribution()}
        values={{ route: "", fallbackEnabled: false }}
        onChange={onChange}
        onCommand={vi.fn()}
      />
    );

    fireEvent.change(screen.getByLabelText("路由策略"), { target: { value: "quality" } });
    fireEvent.click(screen.getByRole("switch", { name: "启用兜底" }));

    expect(onChange).toHaveBeenCalledWith("route", "quality");
    expect(onChange).toHaveBeenCalledWith("fallbackEnabled", true);
  });

  it("renders a warning panel for invalid schemas without throwing", () => {
    expect(() => {
      render(
        <HostRenderedContribution
          contribution={makeContribution({
            schema: { type: "unknown" },
          })}
          values={{}}
          onChange={vi.fn()}
          onCommand={vi.fn()}
        />
      );
    }).not.toThrow();

    expect(screen.getByText("插件界面无法渲染")).toBeInTheDocument();
  });

  it("invokes button commands with plugin and contribution context", () => {
    const onCommand = vi.fn();

    render(
      <HostRenderedContribution
        contribution={makeContribution({
          schema: {
            type: "panel",
            fields: [{ type: "button", key: "export", label: "导出", command: "debug.export" }],
          },
        })}
        values={{}}
        onChange={vi.fn()}
        onCommand={onCommand}
      />
    );

    fireEvent.click(within(screen.getByText("OpenRouter 路由").closest("div")!).getByText("导出"));

    expect(onCommand).toHaveBeenCalledWith("debug.export", {
      pluginId: "acme.openrouter",
      contributionId: "openrouter-routing",
    });
  });
});
