import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { createTestQueryClient } from "../../../test/utils/reactQuery";
import * as service from "../../../services/ompSettings";
import { confirmDesktopDialog } from "../../../services/desktop/confirm";
import { OmpSettingsPanel } from "../omp/OmpSettingsPanel";
import { ompSettingsKey } from "../../../query/ompSettings";
import { settingsPatches } from "../omp/ompSettingsForm";

vi.mock("../../../services/ompSettings", () => ({
  ompSettingsRead: vi.fn(),
  ompSettingsSave: vi.fn(),
  ompAgentRead: vi.fn(),
  ompAgentSave: vi.fn(),
}));
vi.mock("../../../services/desktop/confirm", () => ({ confirmDesktopDialog: vi.fn() }));
vi.mock("sonner", () => ({ toast: { success: vi.fn(), error: vi.fn() } }));

const snapshot: service.OmpSettingsSnapshot = {
  targetId: "target-omp",
  configPath: "C:/test/.omp/agent/config.yml",
  agentsDir: "C:/test/.omp/agent/agents",
  revision: "revision-1",
  writable: true,
  warnings: [],
  values: {
    modelRoles: { default: "native/old:high", custom: "native/future" },
    "task.agentModelOverrides": { scout: ["@smol", "@slow"] },
  },
  fields: [
    {
      key: "defaultThinkingLevel",
      label: "默认思考等级",
      group: "session",
      kind: "enum",
      defaultValue: "high",
      options: ["auto", "high", "medium"],
      min: null,
      max: null,
    },
    {
      key: "task.maxConcurrency",
      label: "最大并发 Agent（0 不限）",
      group: "tasks",
      kind: "number",
      defaultValue: 32,
      options: [],
      min: 0,
      max: 1024,
    },
    {
      key: "task.enableLsp",
      label: "子 Agent 使用 LSP",
      group: "tasks",
      kind: "boolean",
      defaultValue: false,
      options: [],
      min: null,
      max: null,
    },
  ],
  models: [
    {
      selector: "aio-channel/model",
      label: "AIO 聚合模型",
      source: "原生配置 / AIO 入口",
      thinkingLevels: ["low", "high"],
    },
    {
      selector: "native/old",
      label: "原生模型",
      source: "内置目录",
      thinkingLevels: ["high", "max"],
    },
  ],
  agents: [
    {
      name: "scout",
      description: "探索代码",
      source: "bundled",
      fileName: null,
      model: ["@smol"],
      thinking: "medium",
    },
    {
      name: "review-local",
      description: "自定义审查",
      source: "custom",
      fileName: "review.md",
      model: ["@slow"],
      thinking: null,
    },
  ],
};
function setup(targetId = snapshot.targetId) {
  const client = createTestQueryClient();
  const view = render(
    <QueryClientProvider client={client}>
      <OmpSettingsPanel targetId={targetId} />
    </QueryClientProvider>
  );
  return { ...view, client, user: userEvent.setup() };
}
beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(service.ompSettingsRead).mockResolvedValue(structuredClone(snapshot));
  vi.mocked(service.ompSettingsSave).mockResolvedValue({
    targetId: snapshot.targetId,
    revision: "revision-2",
    changed: true,
    backupPath: "C:/test/backup",
  });
  vi.mocked(confirmDesktopDialog).mockResolvedValue(false);
  vi.mocked(service.ompAgentSave).mockResolvedValue({
    targetId: snapshot.targetId,
    revision: "agent-revision-2",
    changed: true,
    backupPath: null,
  });
});

describe("OMP settings", () => {
  it("shows inherited model names and follows unsaved default changes without creating role patches", async () => {
    const { user } = setup();
    const picker = await screen.findByLabelText("默认模型");
    await user.click(screen.getByText("其他模型角色与自定义角色"));
    const fastPreview = screen.getByRole("group", { name: "快速模型 · smol配置解析" });
    expect(fastPreview).toHaveTextContent("原生模型");
    expect(fastPreview).toHaveTextContent("native/old:high");
    expect(fastPreview).toHaveTextContent("@smol → @default");
    expect(
      (screen.getByLabelText("快速模型 · smol") as HTMLSelectElement).selectedOptions[0].textContent
    ).toContain("原生模型");
    expect(service.ompSettingsSave).not.toHaveBeenCalled();
    await user.selectOptions(picker, "aio-channel/model");
    expect(fastPreview).toHaveTextContent("AIO 聚合模型");
    expect(fastPreview).not.toHaveTextContent("native/old");
    await user.click(screen.getByRole("button", { name: "保存 OMP 设置" }));
    expect(service.ompSettingsSave).toHaveBeenCalledWith(
      expect.objectContaining({
        patches: [{ path: ["modelRoles", "default"], value: "aio-channel/model" }],
      })
    );
  });
  it("explains automatic selection after clearing the default instead of retaining an old inherited name", async () => {
    const { user } = setup();
    await user.selectOptions(await screen.findByLabelText("默认模型"), "");
    await user.click(screen.getByText("其他模型角色与自定义角色"));
    const preview = screen.getByRole("group", { name: "快速模型 · smol配置解析" });
    expect(preview).toHaveTextContent("运行时自动选择");
    expect(preview).not.toHaveTextContent("native/old");
    expect(service.ompSettingsSave).not.toHaveBeenCalled();
  });
  it("only reads on mount and saves the changed default model with a revision", async () => {
    const { user } = setup();
    const picker = await screen.findByLabelText("默认模型");
    expect(picker).toHaveValue("native/old");
    expect(screen.getByLabelText("默认模型思考等级")).toHaveValue("high");
    expect(service.ompSettingsSave).not.toHaveBeenCalled();
    await user.selectOptions(picker, "aio-channel/model");
    await user.selectOptions(screen.getByLabelText("默认模型思考等级"), "low");
    await user.click(screen.getByRole("button", { name: "保存 OMP 设置" }));
    await waitFor(() =>
      expect(service.ompSettingsSave).toHaveBeenCalledExactlyOnceWith({
        targetId: snapshot.targetId,
        expectedRevision: "revision-1",
        patches: [{ path: ["modelRoles", "default"], value: "aio-channel/model:low" }],
      })
    );
    expect(screen.getByText(/上次备份/)).toBeInTheDocument();
  });
  it("resets a field by deletion, preserving unknown models and role members", async () => {
    const { user } = setup();
    await user.selectOptions(await screen.findByLabelText("默认模型"), "");
    await user.click(screen.getByRole("button", { name: "保存 OMP 设置" }));
    expect(service.ompSettingsSave).toHaveBeenCalledWith(
      expect.objectContaining({ patches: [{ path: ["modelRoles", "default"], value: null }] })
    );
  });
  it("keeps a dirty draft through query refresh and preserves it after a conflict", async () => {
    const { user, client } = setup();
    await user.selectOptions(await screen.findByLabelText("默认模型"), "aio-channel/model");
    act(() =>
      client.setQueryData(ompSettingsKey(snapshot.targetId), {
        ...snapshot,
        revision: "external",
        values: { modelRoles: { default: "external/model" } },
      })
    );
    expect(screen.getByLabelText("默认模型")).toHaveValue("aio-channel/model");
    vi.mocked(service.ompSettingsSave).mockRejectedValue(
      new Error("OMP_SETTINGS_CONFLICT: 文件已修改")
    );
    await user.click(screen.getByRole("button", { name: "保存 OMP 设置" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("OMP_SETTINGS_CONFLICT");
    expect(screen.getByLabelText("默认模型")).toHaveValue("aio-channel/model");
    expect(service.ompSettingsSave).toHaveBeenCalledWith(
      expect.objectContaining({ expectedRevision: "revision-1" })
    );
  });
  it("rejects invalid numeric input and supports zero and inherited defaults", async () => {
    const { user } = setup();
    await screen.findByLabelText("默认模型");
    await user.click(screen.getByRole("tab", { name: "子任务行为" }));
    const number = screen.getByLabelText("最大并发 Agent（0 不限）");
    await user.type(number, "-1");
    expect(screen.getByRole("button", { name: "保存 OMP 设置" })).toBeDisabled();
    await user.clear(number);
    await user.type(number, "0");
    await user.selectOptions(screen.getByLabelText("子 Agent 使用 LSP"), "true");
    await user.click(screen.getByRole("button", { name: "保存 OMP 设置" }));
    expect(service.ompSettingsSave).toHaveBeenCalledWith(
      expect.objectContaining({
        patches: [
          { path: ["task", "maxConcurrency"], value: 0 },
          { path: ["task", "enableLsp"], value: true },
        ],
      })
    );
  });
  it("updates one Agent member without flattening its siblings or model fallback list", async () => {
    const { user } = setup();
    await screen.findByLabelText("默认模型");
    await user.click(screen.getByRole("tab", { name: "Agent 配置" }));
    await user.click(screen.getByText("scout", { selector: "span" }));
    await user.click(screen.getByRole("switch", { name: "启用 scout" }));
    await user.selectOptions(screen.getByLabelText("scout 顾问 Agent"), "off");
    await user.click(screen.getByRole("button", { name: "保存 OMP 设置" }));
    const saved = vi.mocked(service.ompSettingsSave).mock.calls[0][0];
    expect(saved.patches).toEqual(
      expect.arrayContaining([
        { path: ["task", "disabledAgents"], value: ["scout"] },
        { path: ["task", "agentAdvisor", "scout"], value: "off" },
      ])
    );
    expect(saved.patches).toHaveLength(2);
  });
  it("does not discard drafts when a refresh confirmation is cancelled", async () => {
    const { user } = setup();
    await user.selectOptions(await screen.findByLabelText("默认模型"), "aio-channel/model");
    await user.click(screen.getByRole("button", { name: "重新读取设置" }));
    expect(confirmDesktopDialog).toHaveBeenCalledTimes(1);
    expect(service.ompSettingsRead).toHaveBeenCalledTimes(1);
    expect(screen.getByLabelText("默认模型")).toHaveValue("aio-channel/model");
  });
  it("renders read-only targets without allowing saves", async () => {
    vi.mocked(service.ompSettingsRead).mockResolvedValue({
      ...snapshot,
      writable: false,
      warnings: ["旧版设置只读"],
    });
    setup();
    expect(await screen.findByLabelText("默认模型")).toBeDisabled();
    expect(screen.getByRole("button", { name: "保存 OMP 设置" })).toBeDisabled();
    expect(screen.getByText("旧版设置只读")).toBeInTheDocument();
  });
  it("handles failed reads without inventing an editable empty configuration", async () => {
    vi.mocked(service.ompSettingsRead).mockRejectedValue(new Error("重复的 YAML 字段"));
    setup();
    expect(await screen.findByRole("alert")).toHaveTextContent("重复的 YAML 字段");
    expect(screen.queryByRole("button", { name: "保存 OMP 设置" })).not.toBeInTheDocument();
  });
  it("creates a custom Agent only on an explicit save", async () => {
    const { user } = setup();
    await screen.findByLabelText("默认模型");
    await user.click(screen.getByRole("tab", { name: "Agent 配置" }));
    await user.click(screen.getByRole("button", { name: "创建自定义 Agent" }));
    expect(service.ompAgentSave).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "保存 Agent 定义" }));
    await waitFor(() =>
      expect(service.ompAgentSave).toHaveBeenCalledWith(
        expect.objectContaining({
          targetId: snapshot.targetId,
          expectedRevision: "missing",
          fileName: "my-agent.md",
          content: expect.stringContaining("thinking-level: auto"),
        })
      )
    );
  });
  it("does not carry a draft or a late save result across configuration targets", async () => {
    let complete!: (value: Awaited<ReturnType<typeof service.ompSettingsSave>>) => void;
    vi.mocked(service.ompSettingsSave).mockImplementation(
      () =>
        new Promise((resolve) => {
          complete = resolve;
        })
    );
    const { user, client, rerender } = setup();
    await user.selectOptions(await screen.findByLabelText("默认模型"), "aio-channel/model");
    await user.click(screen.getByRole("button", { name: "保存 OMP 设置" }));
    vi.mocked(service.ompSettingsRead).mockResolvedValue({
      ...snapshot,
      targetId: "another-profile",
      revision: "profile-rev",
      values: { modelRoles: { default: "profile/model" } },
    });
    rerender(
      <QueryClientProvider client={client}>
        <OmpSettingsPanel targetId="another-profile" />
      </QueryClientProvider>
    );
    await waitFor(() => expect(screen.getByLabelText("默认模型")).toHaveValue("profile/model"));
    await act(async () =>
      complete({
        targetId: snapshot.targetId,
        revision: "old-save",
        changed: true,
        backupPath: null,
      })
    );
    expect(screen.getByLabelText("默认模型")).toHaveValue("profile/model");
    expect(screen.getByRole("button", { name: "保存 OMP 设置" })).toBeDisabled();
    expect(service.ompSettingsSave).toHaveBeenCalledTimes(1);
  });
  it("serializes a repeated save click while the first write is pending", async () => {
    let complete!: (value: Awaited<ReturnType<typeof service.ompSettingsSave>>) => void;
    vi.mocked(service.ompSettingsSave).mockImplementation(
      () =>
        new Promise((resolve) => {
          complete = resolve;
        })
    );
    const { user } = setup();
    await user.selectOptions(await screen.findByLabelText("默认模型"), "aio-channel/model");
    await user.dblClick(screen.getByRole("button", { name: "保存 OMP 设置" }));
    expect(service.ompSettingsSave).toHaveBeenCalledTimes(1);
    expect(screen.getByLabelText("默认模型")).toBeDisabled();
    await act(async () =>
      complete({ targetId: snapshot.targetId, revision: "r2", changed: true, backupPath: null })
    );
  });
  it("loads Agent source only on edit and retains the full draft after a conflict", async () => {
    vi.mocked(service.ompAgentRead).mockResolvedValue({
      targetId: snapshot.targetId,
      fileName: "review.md",
      revision: "agent-r1",
      content:
        "---\nname: review-local\ndescription: Review\nfuture: { untouched: true }\n---\nOriginal prompt\n",
    });
    vi.mocked(service.ompAgentSave).mockRejectedValue(new Error("OMP_SETTINGS_CONFLICT"));
    const { user } = setup();
    await screen.findByLabelText("默认模型");
    await user.click(screen.getByRole("tab", { name: "Agent 配置" }));
    expect(service.ompAgentRead).not.toHaveBeenCalled();
    await user.click(screen.getByText("review-local", { selector: "span" }));
    await user.click(screen.getByRole("button", { name: "编辑定义与提示词" }));
    const editor = await screen.findByLabelText("Agent 定义（Markdown）");
    await user.type(editor, "Additional instruction");
    await user.click(screen.getByRole("button", { name: "保存 Agent 定义" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("OMP_SETTINGS_CONFLICT");
    expect((editor as HTMLTextAreaElement).value).toContain("Additional instruction");
    expect(service.ompAgentSave).toHaveBeenCalledWith(
      expect.objectContaining({
        expectedRevision: "agent-r1",
        content: expect.stringContaining("future: { untouched: true }"),
      })
    );
  });
  it("keeps dots in custom role names as literal map keys", () => {
    expect(
      settingsPatches(
        { modelRoles: { "review.v2": "a/b" } },
        { modelRoles: { "review.v2": "a/c" } }
      )
    ).toEqual([{ path: ["modelRoles", "review.v2"], value: "a/c" }]);
  });
});
