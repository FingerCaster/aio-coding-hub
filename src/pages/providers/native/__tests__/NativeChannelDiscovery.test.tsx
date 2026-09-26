import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import * as service from "../../../../services/nativeChannels";
import {
  channelDiscovery,
  channelProvider,
  deferred,
  modelSuggestion,
  nativeModel,
} from "../../../../test/fixtures/native";
import { createQueryWrapper, createTestQueryClient } from "../../../../test/utils/reactQuery";
import { NativeChannelModelsDialog } from "../NativeChannelModelsDialog";
import { parseGatewayModels } from "../nativeGatewayModelDraft";

vi.mock("../../../../services/nativeChannels", async (original) => ({
  ...(await original<typeof service>()),
  nativeChannelModelsDiscover: vi.fn(),
  nativeChannelModelsGet: vi.fn(),
  nativeChannelModelsSet: vi.fn(),
}));
const rich = () =>
  modelSuggestion({
    contextWindow: 200000,
    maxTokens: 12000,
    input: ["text", "image"],
    supportsTools: true,
    reasoning: true,
    reasoningEfforts: ["low", "high"],
    defaultReasoningEffort: "high",
  });
const props = {
  client: "omp" as const,
  targetId: "omp:local:a",
  provider: channelProvider(),
  protocol: "openai-responses" as const,
  onClose: vi.fn(),
  onSaved: vi.fn(),
};
function mount(overrides: Partial<Parameters<typeof NativeChannelModelsDialog>[0]> = {}) {
  return render(<NativeChannelModelsDialog {...props} {...overrides} />, {
    wrapper: createQueryWrapper(createTestQueryClient()),
  });
}
function selectModel(id = "gpt-4o") {
  fireEvent.click(screen.getByLabelText("选择来源模型 " + id));
  fireEvent.click(screen.getByRole("button", { name: "添加所选模型（1）" }));
}
beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(service.nativeChannelModelsGet).mockResolvedValue({
    providerId: 101,
    providerUuid: "prov-101",
    revision: "rev-1",
    models: [],
    stale: false,
  });
  vi.mocked(service.nativeChannelModelsSet).mockResolvedValue({
    providerId: 101,
    providerUuid: "prov-101",
    revision: "rev-2",
    models: [],
    stale: false,
  });
  vi.mocked(service.nativeChannelModelsDiscover).mockResolvedValue(
    channelDiscovery({ targetId: props.targetId, models: [rich()] })
  );
});

describe("native channel automatic capability drafts", () => {
  it("refreshes automatic defaults from explicit capabilities while keeping manual overrides", async () => {
    vi.mocked(service.nativeChannelModelsDiscover)
      .mockResolvedValueOnce(channelDiscovery({ models: [rich()] }))
      .mockResolvedValueOnce(
        channelDiscovery({
          models: [
            {
              ...rich(),
              contextWindow: 100000,
              maxTokens: 8192,
              reasoning: false,
              reasoningEfforts: null,
              defaultReasoningEffort: null,
            },
          ],
        })
      );
    mount();
    await screen.findByText(/已获取 1 个模型/);
    selectModel();
    fireEvent.change(screen.getByLabelText("模型 1 最大输出 Token"), { target: { value: "4000" } });
    fireEvent.click(screen.getByRole("button", { name: "刷新模型与能力" }));
    await waitFor(() => expect(screen.getByLabelText("模型 1 上下文窗口")).toHaveValue(100000));
    expect(screen.getByLabelText("模型 1 最大输出 Token")).toHaveValue(4000);
    expect(screen.getByLabelText("模型 1 推理能力")).toHaveValue("false");
    expect(screen.queryByLabelText("模型 1 默认思考等级")).not.toBeInTheDocument();
  });
  it("adds multiple filtered selections, fills every model and saves one batch", async () => {
    vi.mocked(service.nativeChannelModelsDiscover).mockResolvedValue(
      channelDiscovery({
        models: [
          rich(),
          {
            ...rich(),
            modelId: "second-model",
            contextWindow: 64000,
            maxTokens: 8000,
            defaultReasoningEffort: null,
          },
          modelSuggestion({ modelId: "unknown-model" }),
        ],
      })
    );
    mount();
    await screen.findByText(/已获取 3 个模型/);
    fireEvent.change(screen.getByLabelText("搜索来源模型"), { target: { value: "gpt" } });
    fireEvent.click(screen.getByRole("button", { name: "全选当前结果" }));
    fireEvent.change(screen.getByLabelText("搜索来源模型"), { target: { value: "second" } });
    fireEvent.click(screen.getByRole("button", { name: "全选当前结果" }));
    fireEvent.click(screen.getByRole("button", { name: "添加所选模型（2）" }));
    expect(screen.getByLabelText("模型 1 上下文窗口")).toHaveValue(200000);
    expect(screen.getByLabelText("模型 2 上下文窗口")).toHaveValue(64000);
    expect(screen.getByLabelText("模型 2 默认思考等级")).toHaveValue("low");
    fireEvent.change(screen.getByLabelText("模型 2 最大输出 Token"), { target: { value: "4096" } });
    fireEvent.click(screen.getByRole("button", { name: "保存模型声明" }));
    await waitFor(() => expect(service.nativeChannelModelsSet).toHaveBeenCalledTimes(1));
    const saved = vi.mocked(service.nativeChannelModelsSet).mock.calls[0][5];
    expect(saved.map((model) => model.requestModelId)).toEqual(["gpt-4o", "second-model"]);
    expect(saved[1]).toMatchObject({ maxTokens: 4096, thinking: { defaultLevel: "low" } });
  });
  it("clears multi-selection and does not duplicate saved or already added models", async () => {
    vi.mocked(service.nativeChannelModelsGet).mockResolvedValue({
      providerId: 101,
      providerUuid: props.provider.providerUuid,
      revision: "rev-1",
      stale: false,
      models: [nativeModel({ requestModelId: "gpt-4o" })],
    });
    vi.mocked(service.nativeChannelModelsDiscover).mockResolvedValue(
      channelDiscovery({ models: [rich(), { ...rich(), modelId: "second-model" }] })
    );
    mount();
    await screen.findByText(/已获取 2 个模型/);
    expect(screen.queryByLabelText("选择来源模型 gpt-4o")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "全选当前结果" }));
    fireEvent.click(screen.getByRole("button", { name: "清空选择" }));
    expect(screen.getByRole("button", { name: "添加所选模型（0）" })).toBeDisabled();
    selectModel("second-model");
    expect(screen.queryByLabelText("选择来源模型 second-model")).not.toBeInTheDocument();
    expect(screen.getByText("已配置 2 个模型声明")).toBeInTheDocument();
  });
  it("uses native catalog mappings and editable default effort without manual setup", async () => {
    vi.mocked(service.nativeChannelModelsDiscover).mockResolvedValue(
      channelDiscovery({
        models: [
          {
            ...rich(),
            sources: ["omp_catalog:18.3.2:xai"],
            reasoningEfforts: ["low", "high"],
            nativeThinking: {
              client: "omp",
              mode: "effort",
              efforts: ["minimal", "high"],
              defaultLevel: "minimal",
              effortMap: { minimal: "low", high: "high" },
              supportsDisplay: false,
              requiresEffort: true,
            },
          },
        ],
      })
    );
    mount();
    await screen.findByText(/已获取 1 个模型/);
    selectModel();
    expect(screen.getByLabelText("模型 1 minimal 对应上游等级")).toHaveValue("low");
    expect(screen.getByLabelText("模型 1 默认思考等级")).toHaveValue("minimal");
    fireEvent.click(screen.getByRole("button", { name: "保存模型声明" }));
    await waitFor(() => expect(service.nativeChannelModelsSet).toHaveBeenCalledTimes(1));
    expect(vi.mocked(service.nativeChannelModelsSet).mock.calls[0][5][0].thinking).toMatchObject({
      effortMap: { minimal: "low" },
      defaultLevel: "minimal",
    });
  });
  it("automatically fetches and fills OMP capabilities, then saves dropdown choices", async () => {
    mount();
    await screen.findByText(/已获取 1 个模型/);
    expect(service.nativeChannelModelsDiscover).toHaveBeenCalledWith(
      "omp:local:a",
      101,
      props.provider.providerUuid,
      "openai-responses"
    );
    selectModel();
    expect(screen.getByLabelText("模型 1 上下文窗口")).toHaveValue(200000);
    expect(screen.getByLabelText("模型 1 最大输出 Token")).toHaveValue(12000);
    expect(screen.getByLabelText("模型 1 输入类型")).toHaveValue("image");
    expect(screen.getByLabelText("模型 1 工具调用")).toHaveValue("true");
    expect(screen.getByLabelText("模型 1 思考模式")).toHaveValue("effort");
    expect(screen.getByLabelText("模型 1 默认思考等级")).toHaveValue("high");
    fireEvent.change(screen.getByLabelText("模型 1 默认思考等级"), { target: { value: "low" } });
    expect(service.nativeChannelModelsSet).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "保存模型声明" }));
    await waitFor(() =>
      expect(service.nativeChannelModelsSet).toHaveBeenCalledWith(
        "omp:local:a",
        101,
        props.provider.providerUuid,
        "openai-responses",
        "rev-1",
        [
          expect.objectContaining({
            contextWindow: 200000,
            maxTokens: 12000,
            thinking: expect.objectContaining({
              client: "omp",
              mode: "effort",
              efforts: ["low", "high"],
              defaultLevel: "low",
            }),
          }),
        ]
      )
    );
  });
  it("keeps a manually edited or cleared field when a late result arrives", async () => {
    const delayed = deferred<service.ChannelModelDiscovery>();
    vi.mocked(service.nativeChannelModelsDiscover).mockReturnValue(delayed.promise);
    mount();
    await screen.findByText("已配置 0 个模型声明");
    selectModel();
    fireEvent.change(screen.getByLabelText("模型 1 上下文窗口"), { target: { value: "64000" } });
    fireEvent.change(screen.getByLabelText("模型 1 最大输出 Token"), { target: { value: "100" } });
    fireEvent.change(screen.getByLabelText("模型 1 最大输出 Token"), { target: { value: "" } });
    await act(async () => {
      delayed.resolve(channelDiscovery({ models: [rich()] }));
    });
    await waitFor(() => expect(screen.getByLabelText("模型 1 工具调用")).toHaveValue("true"));
    expect(screen.getByLabelText("模型 1 上下文窗口")).toHaveValue(64000);
    expect(screen.getByLabelText("模型 1 最大输出 Token")).toHaveValue(null);
    expect(screen.getByLabelText("模型 1 工具调用")).toHaveValue("true");
  });
  it("clears only automatically filled capabilities when routing now requires confirmation", async () => {
    vi.mocked(service.nativeChannelModelsDiscover)
      .mockResolvedValueOnce(channelDiscovery({ models: [rich()] }))
      .mockResolvedValueOnce(
        channelDiscovery({ models: [modelSuggestion({ sources: ["routing_confirmation"] })] })
      );
    mount();
    await screen.findByText(/已获取 1 个模型/);
    selectModel();
    fireEvent.change(screen.getByLabelText("模型 1 上下文窗口"), { target: { value: "64000" } });
    fireEvent.click(screen.getByRole("button", { name: "刷新模型与能力" }));
    await screen.findByText(/模型或思考等级存在路由改写/);
    await waitFor(() => expect(screen.getByLabelText("模型 1 工具调用")).toHaveValue(""));
    expect(screen.getByLabelText("模型 1 上下文窗口")).toHaveValue(64000);
    expect(screen.getByLabelText("模型 1 最大输出 Token")).toHaveValue(null);
  });
  it("keeps JSON drafts and saved declarations unchanged during discovery", async () => {
    const delayed = deferred<service.ChannelModelDiscovery>();
    vi.mocked(service.nativeChannelModelsDiscover).mockReturnValue(delayed.promise);
    vi.mocked(service.nativeChannelModelsGet).mockResolvedValue({
      providerId: 101,
      providerUuid: "prov-101",
      revision: "rev-1",
      stale: false,
      models: [nativeModel({ contextWindow: 32000, maxTokens: 4000 })],
    });
    mount();
    await screen.findByText("已配置 1 个模型声明");
    fireEvent.click(screen.getByRole("tab", { name: "高级 JSON 编辑" }));
    const textarea = screen.getByLabelText("模型能力声明 JSON");
    fireEvent.change(textarea, { target: { value: "[{ unfinished draft" } });
    await act(async () => {
      delayed.resolve(channelDiscovery({ models: [rich()] }));
    });
    expect(textarea).toHaveValue("[{ unfinished draft");
    expect(service.nativeChannelModelsSet).not.toHaveBeenCalled();
  });
  it("keeps candidates and form edits after a failed refresh", async () => {
    vi.mocked(service.nativeChannelModelsDiscover)
      .mockResolvedValueOnce(
        channelDiscovery({ models: [rich(), modelSuggestion({ modelId: "new-model" })] })
      )
      .mockResolvedValueOnce(
        channelDiscovery({
          models: [],
          discovery: { status: "error", code: "timeout", http_status: null },
        })
      );
    mount();
    await screen.findByText(/已获取 2 个模型/);
    selectModel();
    fireEvent.change(screen.getByLabelText("模型 1 上下文窗口"), { target: { value: "64000" } });
    fireEvent.click(screen.getByRole("button", { name: "刷新模型与能力" }));
    await screen.findByText(/自动获取失败，已保留/);
    expect(screen.getByLabelText("模型 1 上下文窗口")).toHaveValue(64000);
    expect(screen.getByLabelText("选择来源模型 new-model")).toBeInTheDocument();
  });
  it("never leaks late suggestions across target or provider identity changes", async () => {
    const delayed = deferred<service.ChannelModelDiscovery>();
    vi.mocked(service.nativeChannelModelsDiscover)
      .mockReturnValueOnce(delayed.promise)
      .mockResolvedValueOnce(
        channelDiscovery({
          targetId: "omp:local:b",
          models: [modelSuggestion({ modelId: "new-target" })],
        })
      );
    const { rerender } = mount();
    await waitFor(() => expect(service.nativeChannelModelsDiscover).toHaveBeenCalledTimes(1));
    rerender(<NativeChannelModelsDialog {...props} targetId="omp:local:b" />);
    await waitFor(() => expect(service.nativeChannelModelsDiscover).toHaveBeenCalledTimes(2));
    await act(async () => {
      delayed.resolve(channelDiscovery({ models: [modelSuggestion({ modelId: "old-only" })] }));
    });
    expect(screen.queryByLabelText("选择来源模型 old-only")).not.toBeInTheDocument();
    expect(screen.getByLabelText("选择来源模型 new-target")).toBeInTheDocument();
  });
  it("does not fill suggestions obtained for a different source revision", async () => {
    vi.mocked(service.nativeChannelModelsDiscover).mockResolvedValue(
      channelDiscovery({ revision: "changed", models: [rich()] })
    );
    mount();
    await screen.findByText(/模型声明或来源配置已变化/);
    selectModel();
    expect(screen.getByLabelText("模型 1 上下文窗口")).toHaveValue(null);
    expect(screen.getByLabelText("模型 1 工具调用")).toHaveValue("");
  });
  it("creates an explicit Pi level map with no invented supported levels", async () => {
    mount({ client: "pi", targetId: "pi:local:a" });
    await screen.findByText(/已获取 1 个模型/);
    selectModel();
    expect(screen.getByLabelText("模型 1 medium 对应上游等级")).toHaveValue("");
    expect(screen.getByLabelText("模型 1 high 对应上游等级")).toHaveValue("high");
    fireEvent.click(screen.getByRole("button", { name: "保存模型声明" }));
    await waitFor(() =>
      expect(service.nativeChannelModelsSet).toHaveBeenCalledWith(
        "pi:local:a",
        101,
        props.provider.providerUuid,
        "openai-responses",
        "rev-1",
        [
          expect.objectContaining({
            thinking: {
              client: "pi",
              levelMap: {
                off: null,
                minimal: null,
                low: "low",
                medium: null,
                high: "high",
                xhigh: null,
                max: null,
              },
            },
          }),
        ]
      )
    );
  });
  it("changes model identity without inheriting capacities from the previous model", async () => {
    mount();
    await screen.findByText(/已获取 1 个模型/);
    selectModel();
    fireEvent.change(screen.getByLabelText("模型 1 模型 ID"), {
      target: { value: "custom-unknown" },
    });
    expect(screen.getByLabelText("模型 1 上下文窗口")).toHaveValue(null);
    expect(screen.getByLabelText("模型 1 工具调用")).toHaveValue("");
    expect(screen.queryByLabelText("模型 1 默认思考等级")).not.toBeInTheDocument();
  });
  it("allows a complete manual thinking declaration using dropdowns", async () => {
    vi.mocked(service.nativeChannelModelsDiscover).mockResolvedValue(
      channelDiscovery({ discovery: { status: "unsupported", reason: "oauth" } })
    );
    mount();
    await screen.findByText(/该来源暂未提供自动发现接口/);
    selectModel();
    fireEvent.change(screen.getByLabelText("模型 1 上下文窗口"), { target: { value: "64000" } });
    fireEvent.change(screen.getByLabelText("模型 1 最大输出 Token"), { target: { value: "4000" } });
    fireEvent.change(screen.getByLabelText("模型 1 工具调用"), { target: { value: "true" } });
    fireEvent.change(screen.getByLabelText("模型 1 推理能力"), { target: { value: "true" } });
    fireEvent.change(screen.getByLabelText("模型 1 思考模式"), { target: { value: "effort" } });
    fireEvent.change(screen.getByLabelText("模型 1 high 对应上游等级"), {
      target: { value: "high" },
    });
    fireEvent.change(screen.getByLabelText("模型 1 默认思考等级"), { target: { value: "high" } });
    fireEvent.click(screen.getByRole("button", { name: "保存模型声明" }));
    await waitFor(() => expect(service.nativeChannelModelsSet).toHaveBeenCalledTimes(1));
  });
  it("rejects invalid OMP modes, defaults and mappings before IPC", () => {
    const base = nativeModel({
      reasoning: true,
      thinking: {
        client: "omp",
        mode: "effort",
        efforts: ["high"],
        defaultLevel: "high",
        effortMap: { high: "high" },
        supportsDisplay: null,
        requiresEffort: null,
      },
    });
    expect(() => parseGatewayModels(JSON.stringify([base]), "omp")).not.toThrow();
    for (const patch of [
      { mode: "thought" },
      { defaultLevel: "low" },
      { effortMap: { low: "low" } },
      { efforts: ["high", "high"] },
    ]) {
      expect(() =>
        parseGatewayModels(
          JSON.stringify([{ ...base, thinking: { ...base.thinking, ...patch } }]),
          "omp"
        )
      ).toThrow(/OMP 思考/);
    }
  });
});
