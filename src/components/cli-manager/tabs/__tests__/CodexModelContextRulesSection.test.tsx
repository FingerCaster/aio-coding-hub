import type { ComponentProps } from "react";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { createTestAppSettings } from "../../../../test/fixtures/settings";
import type { CodexModelContextRule } from "../../../../services/settings/codexModelContextRules";
import { CodexModelContextRulesSection } from "../CodexModelContextRulesSection";

const INITIAL_RULES: CodexModelContextRule[] = [
  { model_id: "b-model", context_window: 272000, enabled: true },
  { model_id: "delete-model", context_window: 128000, enabled: false },
];

function settingsWithRules(rules: CodexModelContextRule[]) {
  return createTestAppSettings({ codex_model_context_rules: rules });
}

function createCandidates() {
  return {
    status: "ready" as const,
    issue: null,
    snapshot: {
      config_path: "/home/test/.codex/config.toml",
      executable_path: "/bin/codex",
      cli_version: "1.0.0",
    },
    models: [
      {
        model_id: "gpt-5.6-sol",
        display_name: "GPT-5.6 Sol",
        hidden: false,
        base_context_window: 272000,
        base_max_context_window: 272000,
      },
      {
        model_id: "gpt-5.6-terra",
        display_name: "GPT-5.6 Terra",
        hidden: true,
        base_context_window: 272000,
        base_max_context_window: 300000,
      },
    ],
  };
}

function renderSection(
  overrides: Partial<ComponentProps<typeof CodexModelContextRulesSection>> = {}
) {
  const persistRules = vi.fn(async (rules: CodexModelContextRule[]) => ({
    status: "confirmed" as const,
    settings: settingsWithRules(rules),
  }));
  const props: ComponentProps<typeof CodexModelContextRulesSection> = {
    rules: INITIAL_RULES,
    candidates: createCandidates(),
    candidatesLoading: false,
    candidatesError: false,
    settingsReadOnly: false,
    settingsReadErrorMessage: null,
    controlsDisabled: false,
    saving: false,
    retrySettings: vi.fn(),
    retryCandidates: vi.fn(),
    persistRules,
    ...overrides,
  };
  return { ...render(<CodexModelContextRulesSection {...props} />), props, persistRules };
}

describe("CodexModelContextRulesSection", () => {
  it("reopens for a new same-route auto-open request after the user closes it", () => {
    const view = renderSection({ autoOpenRequestKey: "request-1" });
    expect(screen.getByRole("dialog", { name: "Codex 模型上下文规则" })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "取消" }));
    expect(screen.queryByRole("dialog", { name: "Codex 模型上下文规则" })).not.toBeInTheDocument();

    view.rerender(<CodexModelContextRulesSection {...view.props} autoOpenRequestKey="request-1" />);
    expect(screen.queryByRole("dialog", { name: "Codex 模型上下文规则" })).not.toBeInTheDocument();

    view.rerender(<CodexModelContextRulesSection {...view.props} autoOpenRequestKey="request-2" />);
    expect(screen.getByRole("dialog", { name: "Codex 模型上下文规则" })).toBeInTheDocument();
  });

  it("keeps CRUD local and applies one sorted whole collection", async () => {
    const { persistRules } = renderSection();
    expect(screen.getByText("启用 1 / 共 2 条")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "管理规则" }));

    fireEvent.click(screen.getByRole("switch", { name: "b-model启用状态" }));
    fireEvent.change(screen.getAllByLabelText("精确模型 ID")[0], {
      target: { value: "z-model" },
    });
    fireEvent.change(screen.getAllByLabelText("上下文 tokens")[0], {
      target: { value: "300000" },
    });
    fireEvent.click(screen.getByRole("button", { name: "删除delete-model" }));
    fireEvent.click(screen.getByRole("button", { name: "添加规则" }));
    fireEvent.change(screen.getAllByLabelText("精确模型 ID")[1], {
      target: { value: "a-model" },
    });
    fireEvent.change(screen.getAllByLabelText("上下文 tokens")[1], {
      target: { value: "2048" },
    });

    expect(persistRules).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "应用更改" }));

    await waitFor(() =>
      expect(persistRules).toHaveBeenCalledWith([
        { model_id: "a-model", context_window: 2048, enabled: true },
        { model_id: "z-model", context_window: 300000, enabled: false },
      ])
    );
    expect(persistRules).toHaveBeenCalledTimes(1);
    await waitFor(() =>
      expect(screen.queryByRole("dialog", { name: "Codex 模型上下文规则" })).not.toBeInTheDocument()
    );
  });

  it("adds the exact 372K preset, keeps hidden candidates out of suggestions, and cancels with zero writes", () => {
    const { persistRules } = renderSection({ rules: [] });
    fireEvent.click(screen.getByRole("button", { name: "管理规则" }));
    fireEvent.click(screen.getByRole("button", { name: "添加 GPT-5.6 372K 预设" }));

    expect(screen.getAllByLabelText("精确模型 ID")).toHaveLength(3);
    expect(screen.getAllByDisplayValue("372000")).toHaveLength(3);
    expect(screen.getByText("基础 272,000 -> 目标 372,000")).toBeInTheDocument();
    expect(screen.getAllByText(/不会增加模型或 Provider 的真实能力/).length).toBeGreaterThan(0);
    const options = Array.from(document.querySelectorAll("datalist option")).map(
      (option) => (option as HTMLOptionElement).value
    );
    expect(options).toEqual(["gpt-5.6-sol"]);

    fireEvent.click(screen.getByRole("button", { name: "取消" }));
    expect(persistRules).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "管理规则" }));
    expect(screen.getByText("暂无规则")).toBeInTheDocument();
  });

  it("resets invalid drafts to canonical without invoking the writer", () => {
    const { persistRules } = renderSection();
    fireEvent.click(screen.getByRole("button", { name: "管理规则" }));
    fireEvent.change(screen.getAllByLabelText("精确模型 ID")[0], {
      target: { value: "   " },
    });
    fireEvent.click(screen.getByRole("button", { name: "应用更改" }));

    expect(persistRules).not.toHaveBeenCalled();
    expect(screen.getByDisplayValue("b-model")).toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent("CODEX_MODEL_CONTEXT_RULE_INVALID");
  });

  it("allows manual editing when candidates fail but blocks all edits after a settings read failure", () => {
    const retryCandidates = vi.fn();
    const retrySettings = vi.fn();
    const view = renderSection({
      candidates: null,
      candidatesError: true,
      retryCandidates,
      retrySettings,
    });
    fireEvent.click(screen.getByRole("button", { name: "管理规则" }));
    expect(screen.getAllByLabelText("精确模型 ID")[0]).toBeEnabled();
    fireEvent.click(screen.getByRole("button", { name: "重试候选读取" }));
    expect(retryCandidates).toHaveBeenCalledTimes(1);

    view.rerender(
      <CodexModelContextRulesSection
        {...view.props}
        candidates={null}
        candidatesError={true}
        settingsReadOnly={true}
        settingsReadErrorMessage="设置读取失败"
        retrySettings={retrySettings}
      />
    );
    expect(screen.getAllByLabelText("精确模型 ID")[0]).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "重试设置读取" }));
    expect(retrySettings).toHaveBeenCalledTimes(1);
  });

  it("prevents duplicate submit and dialog closure while saving", async () => {
    let resolveSave!: (value: {
      status: "confirmed";
      settings: ReturnType<typeof settingsWithRules>;
    }) => void;
    const persistRules = vi.fn(
      () =>
        new Promise<{
          status: "confirmed";
          settings: ReturnType<typeof settingsWithRules>;
        }>((resolve) => {
          resolveSave = resolve;
        })
    );
    renderSection({ persistRules });
    fireEvent.click(screen.getByRole("button", { name: "管理规则" }));
    fireEvent.change(screen.getAllByLabelText("上下文 tokens")[0], {
      target: { value: "300000" },
    });
    fireEvent.click(screen.getByRole("button", { name: "应用更改" }));

    await waitFor(() => expect(screen.getByRole("button", { name: "应用中…" })).toBeDisabled());
    fireEvent.click(screen.getByRole("button", { name: "应用中…" }));
    fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
    fireEvent.click(document.querySelector(".bg-black\\/30") as HTMLElement);
    expect(persistRules).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("dialog")).toBeInTheDocument();

    resolveSave({
      status: "confirmed",
      settings: settingsWithRules([
        { model_id: "b-model", context_window: 300000, enabled: true },
        INITIAL_RULES[1],
      ]),
    });
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
  });

  it.each(["reverted", "blocked"] as const)(
    "restores the returned canonical collection after a %s save result",
    async (status) => {
      const canonical = [{ model_id: "server-winner", context_window: 400000, enabled: false }];
      const persistRules = vi.fn().mockResolvedValue({
        status,
        settings: settingsWithRules(canonical),
      });
      renderSection({ persistRules });
      fireEvent.click(screen.getByRole("button", { name: "管理规则" }));
      fireEvent.change(screen.getAllByLabelText("上下文 tokens")[0], {
        target: { value: "300000" },
      });
      fireEvent.click(screen.getByRole("button", { name: "应用更改" }));

      expect(await screen.findByDisplayValue("server-winner")).toBeInTheDocument();
      expect(screen.getByRole("status")).toHaveTextContent(
        status === "reverted" ? "已恢复为最新 canonical 规则" : "已进入只读保护"
      );
    }
  );

  it("drops a stale open draft when canonical rules change externally", () => {
    const view = renderSection();
    fireEvent.click(screen.getByRole("button", { name: "管理规则" }));
    fireEvent.change(screen.getAllByLabelText("精确模型 ID")[0], {
      target: { value: "stale-draft" },
    });

    view.rerender(
      <CodexModelContextRulesSection
        {...view.props}
        rules={[{ model_id: "external-winner", context_window: 500000, enabled: true }]}
      />
    );

    expect(screen.getByDisplayValue("external-winner")).toBeInTheDocument();
    expect(screen.queryByDisplayValue("stale-draft")).not.toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent("规则已更新");
  });
});
