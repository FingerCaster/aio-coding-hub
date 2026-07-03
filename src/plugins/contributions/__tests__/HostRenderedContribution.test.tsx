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

  it("renders every supported field type and disables interactive controls", () => {
    const onChange = vi.fn();
    const onCommand = vi.fn();

    render(
      <HostRenderedContribution
        contribution={makeContribution({
          title: undefined,
          schema: {
            type: "section",
            fields: [
              {
                type: "password",
                key: "secret",
                label: "密钥",
                placeholder: "sk-...",
                required: true,
              },
              { type: "number", key: "limit", label: "限制", min: 1, max: 9, step: 2 },
              {
                type: "select",
                key: "mode",
                label: "模式",
                options: [
                  { value: "fast", label: "快速" },
                  { value: "safe", label: "稳妥" },
                ],
              },
              { type: "textarea", key: "notes", label: "备注", rows: 5 },
              { type: "info", key: "hint", label: "提示", value: "只读信息" },
              { type: "button", key: "run", label: "运行", command: "plugin.run" },
            ],
          },
        })}
        values={{ secret: 123, limit: 3, mode: "safe", notes: "hello" }}
        onChange={onChange}
        onCommand={onCommand}
        disabled
      />
    );

    expect(screen.getByLabelText("密钥")).toHaveAttribute("type", "password");
    expect(screen.getByLabelText("密钥")).toHaveAttribute("required");
    expect(screen.getByLabelText("限制")).toHaveAttribute("min", "1");
    expect(screen.getByLabelText("限制")).toHaveAttribute("max", "9");
    expect(screen.getByLabelText("限制")).toHaveAttribute("step", "2");
    expect(screen.getByLabelText("模式")).toHaveValue("safe");
    expect(screen.getByLabelText("备注")).toHaveAttribute("rows", "5");
    expect(screen.getByText("只读信息")).toBeInTheDocument();

    for (const control of [
      screen.getByLabelText("密钥"),
      screen.getByLabelText("限制"),
      screen.getByLabelText("模式"),
      screen.getByLabelText("备注"),
      screen.getByRole("button", { name: "运行" }),
    ]) {
      expect(control).toBeDisabled();
    }

    fireEvent.click(screen.getByRole("button", { name: "运行" }));
    expect(onChange).not.toHaveBeenCalled();
    expect(onCommand).not.toHaveBeenCalled();
  });

  it("coerces string, number, empty number, select, and textarea values", () => {
    const onChange = vi.fn();

    render(
      <HostRenderedContribution
        contribution={makeContribution({
          schema: {
            type: "section",
            fields: [
              { type: "text", key: "route", label: "路由策略" },
              { type: "number", key: "limit", label: "限制" },
              {
                type: "select",
                key: "mode",
                label: "模式",
                options: [
                  { value: "fast", label: "快速" },
                  { value: "safe", label: "稳妥" },
                ],
              },
              { type: "textarea", key: "notes", label: "备注" },
            ],
          },
        })}
        values={{ route: 7, limit: "5", mode: false, notes: 99 }}
        onChange={onChange}
      />
    );

    expect(screen.getByLabelText("路由策略")).toHaveValue("7");
    expect(screen.getByLabelText("限制")).toHaveValue(5);
    expect(screen.getByLabelText("模式")).toHaveValue("fast");
    expect(screen.getByLabelText("备注")).toHaveValue("99");

    fireEvent.change(screen.getByLabelText("限制"), { target: { value: "42" } });
    fireEvent.change(screen.getByLabelText("限制"), { target: { value: "" } });
    fireEvent.change(screen.getByLabelText("模式"), { target: { value: "fast" } });
    fireEvent.change(screen.getByLabelText("备注"), { target: { value: "memo" } });

    expect(onChange).toHaveBeenCalledWith("limit", 42);
    expect(onChange).toHaveBeenCalledWith("limit", "");
    expect(onChange).toHaveBeenCalledWith("mode", "fast");
    expect(onChange).toHaveBeenCalledWith("notes", "memo");
  });

  it.each([
    ["success", "已启用"],
    ["warning", "需关注"],
    ["danger", "失败"],
    ["neutral", "未知"],
    ["custom", "默认"],
  ])("renders %s badge schemas", (tone, label) => {
    render(
      <HostRenderedContribution
        contribution={makeContribution({
          schema: { type: "badge", label, tone },
        })}
      />
    );

    expect(screen.getByText(label)).toBeInTheDocument();
  });

  it.each([
    null,
    [],
    { type: 1 },
    { type: "section" },
    { type: "section", fields: [{ type: "text", key: 1, label: "坏字段" }] },
    { type: "section", fields: [{ type: "number", key: "n", label: "数字", min: "0" }] },
    { type: "section", fields: [{ type: "select", key: "s", label: "选择" }] },
    {
      type: "section",
      fields: [
        {
          type: "select",
          key: "s",
          label: "选择",
          options: [{ value: "ok", label: 1 }],
        },
      ],
    },
    { type: "section", fields: [{ type: "textarea", key: "t", label: "文本", rows: "3" }] },
    { type: "section", fields: [{ type: "info", key: "i", label: "信息" }] },
    { type: "section", fields: [{ type: "button", key: "b", label: "按钮" }] },
    { type: "badge" },
  ])("renders warning for invalid schema %#", (schema) => {
    render(
      <HostRenderedContribution
        contribution={makeContribution({
          schema,
        })}
      />
    );

    expect(screen.getByRole("alert")).toHaveTextContent("插件界面无法渲染");
  });
});
