import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";
import * as nativeChannels from "../../../../services/nativeChannels";
import * as nativeCli from "../../../../services/nativeCli";
import {
  channelBinding,
  channelDiscovery,
  channelCatalog,
  channelPreview,
  channelProvider,
  channelSource,
  deferred,
  nativeList,
  nativeModel,
  nativeSummary,
} from "../../../../test/fixtures/native";
import { createQueryWrapper, createTestQueryClient } from "../../../../test/utils/reactQuery";
import { NativeChannelImportDialog } from "../NativeChannelImportDialog";
import { NativeChannelModelsDialog } from "../NativeChannelModelsDialog";
import { NativeProvidersPanel } from "../NativeProvidersPanel";
import { NativeGatewayEntries } from "../NativeGatewayEntries";

vi.mock("../../../../services/nativeChannels", async (original) => {
  const actual = await original<typeof nativeChannels>();
  return {
    ...actual,
    nativeChannelCatalogPreview: vi.fn(),
    nativeChannelPreview: vi.fn(),
    nativeChannelApply: vi.fn(),
    nativeChannelModelsGet: vi.fn(),
    nativeChannelModelsDiscover: vi.fn(),
    nativeChannelModelsSet: vi.fn(),
  };
});

vi.mock("../../../../services/nativeCli", async (original) => {
  const actual = await original<typeof nativeCli>();
  return {
    ...actual,
    nativeCliProvidersList: vi.fn(),
    nativeCliProviderSave: vi.fn(),
    nativeCliProviderApply: vi.fn(),
    nativeCliProviderRemove: vi.fn(),
    nativeCliProviderDelete: vi.fn(),
  };
});

vi.mock("../../../../query/requestLogs", async () => {
  const { useQuery } = await import("@tanstack/react-query");
  return {
    useRequestLogsListAllQuery: () =>
      useQuery({ queryKey: ["test-request-logs"], queryFn: async () => [], initialData: [] }),
  };
});

function createWrapper(queryClient = createTestQueryClient()) {
  const QueryWrapper = createQueryWrapper(queryClient);
  return function Wrapper({ children }: { children: React.ReactNode }) {
    return (
      <MemoryRouter>
        <QueryWrapper>{children}</QueryWrapper>
      </MemoryRouter>
    );
  };
}

const defaultCatalog = () => channelCatalog();

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(nativeChannels.nativeChannelModelsDiscover).mockResolvedValue(channelDiscovery());
  vi.mocked(nativeChannels.nativeChannelCatalogPreview).mockResolvedValue(defaultCatalog());
  vi.mocked(nativeChannels.nativeChannelPreview).mockResolvedValue(channelPreview());
  vi.mocked(nativeChannels.nativeChannelApply).mockResolvedValue({
    targetId: "pi:local:a",
    revision: "file-2",
    changed: true,
    backupPath: null,
    bindings: [channelBinding()],
  });
  vi.mocked(nativeCli.nativeCliProvidersList).mockResolvedValue(
    nativeList({
      providers: [
        nativeSummary({
          nativeKey: "aio-channel-codex-openai-responses",
          displayName: "AIO · Codex",
          managed: true,
          api: "openai-responses",
          modelCount: 1,
        }),
      ],
    })
  );
});

describe("NativeChannelFlows - unified channel import dialog", () => {
  it("multi-select sources and explicit models -> preview -> confirm apply", async () => {
    const onApplied = vi.fn();
    const onClose = vi.fn();

    render(
      <NativeChannelImportDialog
        client="pi"
        targetId="pi:local:a"
        onClose={onClose}
        onApplied={onApplied}
      />,
      { wrapper: createWrapper() }
    );

    // Wait for sources to load
    await screen.findByText("AIO · Claude Code");
    await screen.findByText("AIO · Codex");
    expect(screen.getByText("Pi → AIO 聚合网关 → 渠道可用上游")).toBeInTheDocument();

    // Select Claude Code
    const claudeCheck = screen.getByLabelText("选择 Claude Code 渠道");
    fireEvent.click(claudeCheck);

    // Select Codex
    const codexCheck = screen.getByLabelText("选择 Codex 渠道");
    fireEvent.click(codexCheck);

    // Preview button should now be enabled
    const previewBtn = screen.getByRole("button", { name: "预览入口变更" });
    expect(previewBtn).not.toBeDisabled();

    fireEvent.click(previewBtn);

    await waitFor(() => {
      expect(nativeChannels.nativeChannelPreview).toHaveBeenCalledWith(
        expect.objectContaining({
          targetId: "pi:local:a",
          expectedRevision: "file-1",
          catalogRevision: "cat-1",
          selections: expect.arrayContaining([
            {
              sourceChannel: "claude",
              protocol: "anthropic-messages",
              modelIds: ["claude-3-7-sonnet"],
            },
            {
              sourceChannel: "codex",
              protocol: "openai-responses",
              modelIds: ["gpt-4o"],
            },
          ]),
          removeBindingIds: [],
        })
      );
    });

    // Preview dialog should appear
    await screen.findByText("确认发布统一渠道入口");
    expect(screen.getByText("aio-channel-codex-openai-responses")).toBeInTheDocument();

    // Confirm apply
    const confirmBtn = screen.getByRole("button", { name: "确认写入并接入" });
    expect(confirmBtn).not.toBeDisabled();
    fireEvent.click(confirmBtn);

    await waitFor(() => {
      expect(nativeChannels.nativeChannelApply).toHaveBeenCalledWith(
        expect.objectContaining({
          targetId: "pi:local:a",
          expectedRevision: "file-1",
          catalogRevision: "cat-1",
          selections: expect.any(Array),
          removeBindingIds: [],
        })
      );
      expect(onApplied).toHaveBeenCalledOnce();
      expect(onClose).toHaveBeenCalledOnce();
    });
  });

  it("cancel in main dialog closes without writing files", async () => {
    const onClose = vi.fn();
    render(<NativeChannelImportDialog client="pi" targetId="pi:local:a" onClose={onClose} />, {
      wrapper: createWrapper(),
    });

    await screen.findByText("AIO · Claude Code");

    // Click Cancel in main dialog
    fireEvent.click(screen.getByRole("button", { name: "取消" }));
    expect(onClose).toHaveBeenCalledOnce();
    expect(nativeChannels.nativeChannelApply).not.toHaveBeenCalled();
  });

  it("cancel in preview dialog closes preview without writing files", async () => {
    const onClose = vi.fn();
    render(<NativeChannelImportDialog client="pi" targetId="pi:local:a" onClose={onClose} />, {
      wrapper: createWrapper(),
    });

    await screen.findByText("AIO · Codex");
    const codexCheck = screen.getByLabelText("选择 Codex 渠道");
    fireEvent.click(codexCheck);
    fireEvent.click(screen.getByRole("button", { name: "预览入口变更" }));

    await screen.findByText("确认发布统一渠道入口");
    fireEvent.click(screen.getByRole("button", { name: "取消" }));

    // Verify preview closed and apply was never called
    expect(screen.queryByText("确认发布统一渠道入口")).not.toBeInTheDocument();
    expect(nativeChannels.nativeChannelApply).not.toHaveBeenCalled();
  });

  it("detects stale revision if catalog changes during preview and disables confirmation", async () => {
    vi.mocked(nativeChannels.nativeChannelPreview).mockResolvedValue(
      channelPreview({
        revision: "stale-file-rev",
        catalogRevision: "cat-1",
      })
    );

    render(<NativeChannelImportDialog client="pi" targetId="pi:local:a" onClose={vi.fn()} />, {
      wrapper: createWrapper(),
    });

    await screen.findByText("AIO · Codex");
    fireEvent.click(screen.getByLabelText("选择 Codex 渠道"));
    fireEvent.click(screen.getByRole("button", { name: "预览入口变更" }));

    await screen.findByText("确认发布统一渠道入口");
    // Should warn about stale revision
    expect(
      screen.getByText("目标配置或统一渠道目录已在外部发生变动，请返回重新预览。")
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "确认写入并接入" })).toBeDisabled();
    expect(nativeChannels.nativeChannelApply).not.toHaveBeenCalled();
  });

  it("in-flow capability supplement saves model spec declaration without requiring API Key", async () => {
    const catalogWithoutModels = channelCatalog({
      sources: [
        channelSource({
          sourceChannel: "codex",
          protocol: "openai-responses",
          providers: [
            channelProvider({
              providerId: 101,
              providerUuid: "prov-codex",
              name: "Codex Upstream",
              modelIds: ["gpt-4o"],
            }),
          ],
          models: [],
        }),
      ],
    });
    vi.mocked(nativeChannels.nativeChannelCatalogPreview).mockResolvedValue(catalogWithoutModels);
    vi.mocked(nativeChannels.nativeChannelModelsGet).mockResolvedValue({
      providerId: 101,
      providerUuid: "prov-codex",
      revision: "rev-1",
      stale: false,
      models: [],
    });
    vi.mocked(nativeChannels.nativeChannelModelsSet).mockResolvedValue({
      providerId: 101,
      providerUuid: "prov-codex",
      revision: "rev-2",
      stale: false,
      models: [nativeModel({ requestModelId: "gpt-4o" })],
    });

    render(<NativeChannelImportDialog client="pi" targetId="pi:local:a" onClose={vi.fn()} />, {
      wrapper: createWrapper(),
    });

    await screen.findByText("AIO · Codex");
    fireEvent.click(screen.getByLabelText("选择 Codex 渠道"));

    // Shows missing capability warning
    await screen.findByText("缺少完整模型与工具能力声明，当前无法直接发布。");
    const supplementBtn = screen.getByRole("button", { name: "在流程内补齐模型声明" });
    fireEvent.click(supplementBtn);

    // Dialog opens
    await screen.findByText("来源上游模型能力声明");
    expect(screen.queryByLabelText(/API Key/i)).not.toBeInTheDocument();

    // Switch to JSON tab to verify JSON entry still functions
    const jsonTab = await screen.findByRole("tab", { name: /高级 JSON 编辑/ });
    fireEvent.click(jsonTab);

    // Fill valid JSON model spec
    const textarea = screen.getByLabelText("模型能力声明 JSON");
    const validModelSpec = [
      {
        requestModelId: "gpt-4o",
        displayName: "GPT-4o",
        input: ["text"],
        contextWindow: 128000,
        maxTokens: 4096,
        reasoning: false,
        thinking: null,
        supportsTools: true,
      },
    ];
    fireEvent.change(textarea, { target: { value: JSON.stringify(validModelSpec, null, 2) } });

    // Wait until save button is enabled
    const saveBtn = screen.getByRole("button", { name: "保存模型声明" });
    await waitFor(() => expect(saveBtn).not.toBeDisabled());

    // Save model spec
    fireEvent.click(saveBtn);

    await waitFor(() => {
      expect(nativeChannels.nativeChannelModelsSet).toHaveBeenCalledWith(
        "pi:local:a",
        101,
        "prov-codex",
        "openai-responses",
        "rev-1",
        expect.any(Array)
      );
    });
  });

  it("distinguishes Gemini standard API, unverified enterprise OAuth, and retired personal OAuth", async () => {
    render(<NativeChannelImportDialog client="pi" targetId="pi:local:a" onClose={vi.fn()} />, {
      wrapper: createWrapper(),
    });

    await screen.findByText("AIO · Gemini");

    // Inspect the AIO candidate pool; importing still selects a channel, not an upstream.
    const detailButtons = screen.getAllByRole("button", { name: "AIO 调度池" });
    fireEvent.click(detailButtons[detailButtons.length - 1]);

    // Check that enterprise OAuth blocked reason is shown
    expect(screen.getByText("[企业 Code Assist OAuth 需核实许可与适配]")).toBeInTheDocument();

    // Check that retired personal OAuth migration warning is shown
    expect(
      screen.getByText("[个人 Code Assist 旧服务已于 2026-06-18 退役，请迁往 Antigravity]")
    ).toBeInTheDocument();

    // Standard API is available
    expect(screen.getByText("Gemini API Upstream")).toBeInTheDocument();

    // Antigravity guidance note is displayed
    expect(
      screen.getByText(/Antigravity（反重力）当前未适配为 AIO 统一渠道，不可作为可选来源/)
    ).toBeInTheDocument();
  });

  it("update dialog preserves existing binding published models and does not withdraw unselected sources", async () => {
    const catalogWithBinding = channelCatalog({
      sources: [
        channelSource({
          sourceChannel: "codex",
          protocol: "openai-responses",
          models: [
            nativeModel({ requestModelId: "gpt-4o", displayName: "GPT-4o" }),
            nativeModel({ requestModelId: "o3-mini", displayName: "o3-mini" }),
          ],
        }),
      ],
      bindings: [
        channelBinding({
          sourceChannel: "codex",
          protocol: "openai-responses",
          models: [nativeModel({ requestModelId: "gpt-4o", displayName: "GPT-4o" })],
        }),
      ],
    });
    vi.mocked(nativeChannels.nativeChannelCatalogPreview).mockResolvedValue(catalogWithBinding);

    render(
      <NativeChannelImportDialog
        client="pi"
        targetId="pi:local:a"
        initialSourceChannel="codex"
        initialProtocol="openai-responses"
        onClose={vi.fn()}
      />,
      { wrapper: createWrapper() }
    );

    await screen.findByText("AIO · Codex");

    // Check that gpt-4o (already bound) is checked, while o3-mini (not bound) is unchecked
    const gpt4oLabel = screen.getByText("GPT-4o");
    const gpt4oCheckbox = gpt4oLabel
      .closest("label")!
      .querySelector("input[type='checkbox']") as HTMLInputElement;
    expect(gpt4oCheckbox.checked).toBe(true);

    const o3MiniLabel = screen.getByText("o3-mini");
    const o3MiniCheckbox = o3MiniLabel
      .closest("label")!
      .querySelector("input[type='checkbox']") as HTMLInputElement;
    expect(o3MiniCheckbox.checked).toBe(false);

    // Preview apply
    fireEvent.click(screen.getByRole("button", { name: "预览入口变更" }));

    await waitFor(() => {
      expect(nativeChannels.nativeChannelPreview).toHaveBeenCalledWith(
        expect.objectContaining({
          targetId: "pi:local:a",
          selections: [
            expect.objectContaining({
              sourceChannel: "codex",
              protocol: "openai-responses",
              modelIds: ["gpt-4o"], // only preserved published model
            }),
          ],
          removeBindingIds: [], // unchecked sources are not withdrawn
        })
      );
    });
  });

  it("discards an in-flight old preview without blocking or overwriting the new target", async () => {
    const oldPreview = deferred<ReturnType<typeof channelPreview>>();
    vi.mocked(nativeChannels.nativeChannelCatalogPreview).mockImplementation(async (targetId) =>
      channelCatalog({ targetId })
    );
    vi.mocked(nativeChannels.nativeChannelPreview)
      .mockReturnValueOnce(oldPreview.promise)
      .mockResolvedValueOnce(channelPreview({ targetId: "pi:local:b" }));
    const { rerender } = render(
      <NativeChannelImportDialog client="pi" targetId="pi:local:a" onClose={vi.fn()} />,
      { wrapper: createWrapper() }
    );
    await screen.findByText("AIO · Codex");
    fireEvent.click(screen.getByLabelText("选择 Codex 渠道"));
    fireEvent.click(screen.getByRole("button", { name: "预览入口变更" }));
    await waitFor(() => expect(nativeChannels.nativeChannelPreview).toHaveBeenCalledTimes(1));

    rerender(<NativeChannelImportDialog client="pi" targetId="pi:local:b" onClose={vi.fn()} />);
    await waitFor(() =>
      expect(nativeChannels.nativeChannelCatalogPreview).toHaveBeenCalledWith("pi:local:b")
    );
    fireEvent.click(await screen.findByLabelText("选择 Codex 渠道"));
    const newPreviewButton = screen.getByRole("button", { name: "预览入口变更" });
    await waitFor(() => expect(newPreviewButton).toBeEnabled());
    fireEvent.click(newPreviewButton);
    await screen.findByText("确认发布统一渠道入口");
    await act(async () => {
      oldPreview.resolve(channelPreview());
    });
    expect(screen.getByText("确认发布统一渠道入口")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "确认写入并接入" }));
    await waitFor(() =>
      expect(nativeChannels.nativeChannelApply).toHaveBeenCalledWith(
        expect.objectContaining({ targetId: "pi:local:b" })
      )
    );
    expect(nativeChannels.nativeChannelApply).toHaveBeenCalledTimes(1);
  });

  it("target switching prevents stale preview from being applied to new target", async () => {
    const { rerender } = render(
      <NativeChannelImportDialog client="pi" targetId="pi:local:a" onClose={vi.fn()} />,
      { wrapper: createWrapper() }
    );

    await screen.findByText("AIO · Codex");
    fireEvent.click(screen.getByLabelText("选择 Codex 渠道"));
    fireEvent.click(screen.getByRole("button", { name: "预览入口变更" }));

    await screen.findByText("确认发布统一渠道入口");

    // Rerender with different targetId
    rerender(<NativeChannelImportDialog client="pi" targetId="pi:local:b" onClose={vi.fn()} />);

    // Stale preview dialog must be dismissed/closed
    await waitFor(() => {
      expect(screen.queryByText("确认发布统一渠道入口")).not.toBeInTheDocument();
    });
    expect(nativeChannels.nativeChannelApply).not.toHaveBeenCalled();
  });
});

describe("NativeChannelModelsDialog - form capabilities and candidate models", () => {
  it("creates model without numerical guessing and candidate fills ID only", async () => {
    vi.mocked(nativeChannels.nativeChannelModelsGet).mockResolvedValue({
      providerId: 201,
      providerUuid: "prov-omp",
      revision: "rev-omp-1",
      stale: false,
      models: [],
    });
    vi.mocked(nativeChannels.nativeChannelModelsSet).mockResolvedValue({
      providerId: 201,
      providerUuid: "prov-omp",
      revision: "rev-omp-2",
      stale: false,
      models: [],
    });

    const provider = channelProvider({
      providerId: 201,
      providerUuid: "prov-omp",
      name: "OMP Provider",
      modelIds: ["custom-model-x"],
    });

    const onSaved = vi.fn();
    const onClose = vi.fn();

    render(
      <NativeChannelModelsDialog
        client="omp"
        targetId="omp:local:a"
        provider={provider}
        protocol="openai-responses"
        onClose={onClose}
        onSaved={onSaved}
      />,
      { wrapper: createWrapper() }
    );

    // Wait for query data to load
    await screen.findByText("已配置 0 个模型声明");

    fireEvent.click(screen.getByLabelText("选择来源模型 custom-model-x"));
    fireEvent.click(screen.getByRole("button", { name: "添加所选模型（1）" }));

    // Model item should be added with ID and Name filled, but contextWindow and maxTokens EMPTY
    const idInput = screen.getByPlaceholderText(/如 gpt-4o/);
    expect((idInput as HTMLInputElement).value).toBe("custom-model-x");

    const ctxInput = screen.getByPlaceholderText(/必填，如 128000/);
    expect((ctxInput as HTMLInputElement).value).toBe(""); // NO GUESSING!

    const maxInput = screen.getByPlaceholderText(/必填，如 4096/);
    expect((maxInput as HTMLInputElement).value).toBe(""); // NO GUESSING!

    // Clicking save without numbers shows error
    const saveBtn = screen.getByRole("button", { name: "保存模型声明" });
    fireEvent.click(saveBtn);
    await screen.findByText(/上下文窗口必须填写正整数容量/);

    // Fill valid numbers and save
    fireEvent.change(ctxInput, { target: { value: "64000" } });
    fireEvent.change(maxInput, { target: { value: "4096" } });

    // OMP tools radio must be selected
    const toolsSelect = screen.getByLabelText("模型 1 工具调用");
    expect(toolsSelect).toHaveValue("");
    fireEvent.click(saveBtn);
    await screen.findByText(/工具调用必须明确为 true 或 false/);
    expect(nativeChannels.nativeChannelModelsSet).not.toHaveBeenCalled();
    fireEvent.change(toolsSelect, { target: { value: "true" } });

    fireEvent.click(saveBtn);
    await screen.findByText(/推理能力必须明确选择/);
    expect(nativeChannels.nativeChannelModelsSet).not.toHaveBeenCalled();
    fireEvent.change(screen.getByLabelText("模型 1 推理能力"), { target: { value: "false" } });
    fireEvent.change(ctxInput, { target: { value: "64000.5" } });
    fireEvent.click(saveBtn);
    await screen.findByText(/上下文窗口必须填写正整数容量/);
    expect(nativeChannels.nativeChannelModelsSet).not.toHaveBeenCalled();
    fireEvent.change(ctxInput, { target: { value: "64000" } });
    fireEvent.click(saveBtn);

    await waitFor(() => {
      expect(nativeChannels.nativeChannelModelsSet).toHaveBeenCalledWith(
        "omp:local:a",
        201,
        "prov-omp",
        "openai-responses",
        "rev-omp-1",
        [
          expect.objectContaining({
            requestModelId: "custom-model-x",
            displayName: "custom-model-x",
            contextWindow: 64000,
            maxTokens: 4096,
            supportsTools: true, // boolean, never null
            reasoning: false,
            thinking: null,
          }),
        ]
      );
      expect(onSaved).toHaveBeenCalledOnce();
      expect(onClose).toHaveBeenCalledOnce();
    });
  });

  it("OMP mode forbids null supportsTools in JSON view", async () => {
    vi.mocked(nativeChannels.nativeChannelModelsGet).mockResolvedValue({
      providerId: 202,
      providerUuid: "prov-omp-2",
      revision: "rev-1",
      stale: false,
      models: [],
    });

    const provider = channelProvider({
      providerId: 202,
      providerUuid: "prov-omp-2",
      name: "OMP Provider",
    });

    render(
      <NativeChannelModelsDialog
        client="omp"
        targetId="omp:local:a"
        provider={provider}
        protocol="openai-responses"
        onClose={vi.fn()}
        onSaved={vi.fn()}
      />,
      { wrapper: createWrapper() }
    );

    // Wait for query to resolve
    await screen.findByText("已配置 0 个模型声明");

    // Switch to JSON tab
    fireEvent.click(screen.getByRole("tab", { name: /高级 JSON 编辑/ }));
    const textarea = screen.getByLabelText("模型能力声明 JSON");

    // Enter spec with supportsTools: null
    const invalidOmpSpec = [
      {
        requestModelId: "test-omp-model",
        displayName: "Test OMP",
        input: ["text"],
        contextWindow: 32000,
        maxTokens: 2048,
        reasoning: false,
        thinking: null,
        supportsTools: null, // Null is forbidden for OMP!
      },
    ];
    fireEvent.change(textarea, { target: { value: JSON.stringify(invalidOmpSpec, null, 2) } });

    // Click save
    fireEvent.click(screen.getByRole("button", { name: "保存模型声明" }));

    // Should display error message
    await screen.findByText(/OMP 发布模型 tools 必须明确为 true 或 false，不可为 null。/);
    expect(nativeChannels.nativeChannelModelsSet).not.toHaveBeenCalled();
  });
});

describe("NativeProvidersPanel & NativeGatewayEntries - managed cards and precise withdrawal", () => {
  it("NativeProvidersPanel displays AIO managed card with source badge and supports withdrawal", async () => {
    const catalogWithBinding = channelCatalog({
      bindings: [
        channelBinding({
          bindingId: "bind-codex-1",
          sourceChannel: "codex",
          protocol: "openai-responses",
          nativeKey: "aio-channel-codex-openai-responses",
        }),
      ],
    });
    vi.mocked(nativeChannels.nativeChannelCatalogPreview).mockResolvedValue(catalogWithBinding);

    render(<NativeProvidersPanel client="pi" targetId="pi:local:a" onImport={vi.fn()} />, {
      wrapper: createWrapper(),
    });

    // Displays AIO 托管 · Codex badge
    await screen.findByText("AIO 托管 · Codex");
    expect(screen.getByText("管理来源渠道")).toBeInTheDocument();
    expect(screen.getByText("更新入口")).toBeInTheDocument();

    // Click 撤回接入
    const withdrawBtn = screen.getByRole("button", { name: "撤回接入" });
    fireEvent.click(withdrawBtn);

    // Confirmation dialog appears
    const dialog = await screen.findByRole("dialog", { name: "确认撤回 AIO · Codex 入口" });
    expect(within(dialog).getByText("aio-channel-codex-openai-responses")).toBeInTheDocument();

    // Confirm withdrawal
    fireEvent.click(within(dialog).getByRole("button", { name: "确认撤回" }));

    await waitFor(() => {
      expect(nativeChannels.nativeChannelApply).toHaveBeenCalledWith(
        expect.objectContaining({
          targetId: "pi:local:a",
          expectedRevision: "file-1",
          catalogRevision: "cat-1",
          selections: [],
          removeBindingIds: ["bind-codex-1"],
        })
      );
    });
  });

  it("NativeGatewayEntries displays unified channel section and toolbar button", async () => {
    const catalogWithBinding = channelCatalog({
      bindings: [
        channelBinding({
          bindingId: "bind-claude-1",
          sourceChannel: "claude",
          protocol: "anthropic-messages",
          nativeKey: "aio-channel-claude-anthropic-messages",
        }),
      ],
    });
    vi.mocked(nativeChannels.nativeChannelCatalogPreview).mockResolvedValue(catalogWithBinding);

    render(<NativeGatewayEntries client="pi" targetId="pi:local:a" />, {
      wrapper: createWrapper(),
    });

    // Verify toolbar has "从 AIO 渠道接入"
    await screen.findByRole("button", { name: "从 AIO 渠道接入" });

    // Verify channel section shows binding
    await screen.findByText("从 AIO 统一渠道接入 (1)");
    expect(screen.getByText("AIO · Claude Code")).toBeInTheDocument();
    expect(screen.getByText("aio-channel-claude-anthropic-messages")).toBeInTheDocument();
  });
});
