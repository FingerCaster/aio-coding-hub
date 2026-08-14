import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { toast } from "sonner";
import { CliManagerCx2ccTab } from "../Cx2ccTab";
import type { AppSettings } from "../../../../services/settings/settings";
import { createDefaultCx2ccReasoningEffortMappings } from "../../../../constants/cx2cc";

vi.mock("sonner", () => ({ toast: vi.fn() }));

function createAppSettings(overrides: Partial<AppSettings> = {}): AppSettings {
  return {
    ui_locale: "zh-CN",
    ui_theme: "system",
    cx2cc_fallback_model_opus: "gpt-5.4-opus",
    cx2cc_fallback_model_sonnet: "gpt-5.4-sonnet",
    cx2cc_fallback_model_haiku: "gpt-5.4-haiku",
    cx2cc_fallback_model_main: "gpt-5.4-main",
    cx2cc_reasoning_effort_mappings: createDefaultCx2ccReasoningEffortMappings(),
    cx2cc_service_tier: "fast",
    cx2cc_model_reasoning_effort: "medium",
    cx2cc_disable_response_storage: true,
    cx2cc_enable_reasoning_to_thinking: true,
    cx2cc_drop_stop_sequences: true,
    cx2cc_clean_schema: true,
    cx2cc_filter_batch_tool: true,
    // Other required fields with defaults
    gateway_provider_cooldown_secs: 30,
    gateway_upstream_first_byte_timeout_secs: 30,
    gateway_upstream_stream_idle_timeout_secs: 60,
    gateway_upstream_request_timeout_non_streaming_secs: 120,
    gateway_max_attempts_per_provider: 3,
    gateway_abort_stuck_stream_threshold_secs: 300,
    gateway_verbose_provider_error: false,
    gateway_enable_response_fixer: true,
    gateway_enable_thinking_signature_rectifier: true,
    gateway_enable_thinking_budget_rectifier: true,
    gateway_circuit_breaker_failure_threshold: 5,
    gateway_circuit_breaker_open_duration_secs: 30,
    ...overrides,
  } as AppSettings;
}

describe("components/cli-manager/tabs/Cx2ccTab", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("renders all setting sections", () => {
    const persistSettings = vi.fn().mockResolvedValue(null);
    render(
      <CliManagerCx2ccTab
        appSettings={createAppSettings()}
        commonSettingsSaving={false}
        onPersistCommonSettings={persistSettings}
      />
    );

    expect(screen.getByText("模型 Fallback 映射")).toBeInTheDocument();
    expect(screen.getByText("思考强度转换")).toBeInTheDocument();
    expect(screen.getByText("上游请求注入")).toBeInTheDocument();
    expect(screen.getByText("转换行为开关")).toBeInTheDocument();
  });

  it("renders all fallback model inputs with initial values", () => {
    const persistSettings = vi.fn().mockResolvedValue(null);
    render(
      <CliManagerCx2ccTab
        appSettings={createAppSettings()}
        commonSettingsSaving={false}
        onPersistCommonSettings={persistSettings}
      />
    );

    expect(screen.getByDisplayValue("gpt-5.4-opus")).toBeInTheDocument();
    expect(screen.getByDisplayValue("gpt-5.4-sonnet")).toBeInTheDocument();
    expect(screen.getByDisplayValue("gpt-5.4-haiku")).toBeInTheDocument();
    expect(screen.getByDisplayValue("gpt-5.4-main")).toBeInTheDocument();
  });

  it("renders the default reasoning effort mappings and passthrough behavior", () => {
    render(
      <CliManagerCx2ccTab
        appSettings={createAppSettings()}
        commonSettingsSaving={false}
        onPersistCommonSettings={vi.fn().mockResolvedValue(null)}
      />
    );

    expect(screen.getByLabelText("规则 5 来源强度")).toHaveValue("max");
    expect(screen.getByLabelText("规则 5 目标强度")).toHaveValue("max");
    expect(screen.getByLabelText("规则 6 来源强度")).toHaveValue("ultra");
    expect(screen.getByLabelText("规则 6 目标强度")).toHaveValue("max");
    expect(screen.getByText("按来源强度精确匹配；未命中时原样透传。")).toBeInTheDocument();
  });

  it("adds, edits and saves a reasoning effort mapping", async () => {
    const persistSettings = vi.fn(async (patch: Partial<AppSettings>) => createAppSettings(patch));
    render(
      <CliManagerCx2ccTab
        appSettings={createAppSettings()}
        commonSettingsSaving={false}
        onPersistCommonSettings={persistSettings}
      />
    );

    fireEvent.change(screen.getByLabelText("规则 5 目标强度"), {
      target: { value: "xhigh" },
    });
    fireEvent.click(screen.getByRole("button", { name: "添加思考强度转换规则" }));
    fireEvent.change(screen.getByLabelText("规则 7 来源强度"), {
      target: { value: " future " },
    });
    fireEvent.change(screen.getByLabelText("规则 7 目标强度"), {
      target: { value: " xhigh " },
    });
    fireEvent.click(screen.getByRole("button", { name: "保存思考强度转换规则" }));

    await waitFor(() =>
      expect(persistSettings).toHaveBeenCalledWith({
        cx2cc_reasoning_effort_mappings: [
          ...createDefaultCx2ccReasoningEffortMappings().slice(0, 4),
          { source: "max", target: "xhigh" },
          { source: "ultra", target: "max" },
          { source: "future", target: "xhigh" },
        ],
      })
    );
  });

  it("deletes a reasoning effort mapping from the saved array", async () => {
    const persistSettings = vi.fn(async (patch: Partial<AppSettings>) => createAppSettings(patch));
    render(
      <CliManagerCx2ccTab
        appSettings={createAppSettings()}
        commonSettingsSaving={false}
        onPersistCommonSettings={persistSettings}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: "删除思考强度转换规则 1" }));
    fireEvent.click(screen.getByRole("button", { name: "保存思考强度转换规则" }));

    await waitFor(() =>
      expect(persistSettings).toHaveBeenCalledWith({
        cx2cc_reasoning_effort_mappings: createDefaultCx2ccReasoningEffortMappings().slice(1),
      })
    );
  });

  it("restores and persists the complete default reasoning effort mapping", async () => {
    const persistSettings = vi.fn(async (patch: Partial<AppSettings>) => createAppSettings(patch));
    render(
      <CliManagerCx2ccTab
        appSettings={createAppSettings({
          cx2cc_reasoning_effort_mappings: [{ source: "ultra", target: "high" }],
        })}
        commonSettingsSaving={false}
        onPersistCommonSettings={persistSettings}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: "恢复默认思考强度转换规则" }));

    await waitFor(() =>
      expect(persistSettings).toHaveBeenCalledWith({
        cx2cc_reasoning_effort_mappings: createDefaultCx2ccReasoningEffortMappings(),
      })
    );
  });

  it("blocks invalid reasoning effort mappings before persistence", () => {
    const persistSettings = vi.fn().mockResolvedValue(null);
    render(
      <CliManagerCx2ccTab
        appSettings={createAppSettings()}
        commonSettingsSaving={false}
        onPersistCommonSettings={persistSettings}
      />
    );

    fireEvent.change(screen.getByLabelText("规则 6 来源强度"), {
      target: { value: " max " },
    });
    fireEvent.click(screen.getByRole("button", { name: "保存思考强度转换规则" }));

    expect(persistSettings).not.toHaveBeenCalled();
    expect(toast).toHaveBeenCalledWith("第 6 条规则的来源强度“max”重复");
  });

  it("keeps an incomplete new rule as a draft and refuses to save it", () => {
    const persistSettings = vi.fn().mockResolvedValue(null);
    render(
      <CliManagerCx2ccTab
        appSettings={createAppSettings()}
        commonSettingsSaving={false}
        onPersistCommonSettings={persistSettings}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: "添加思考强度转换规则" }));
    fireEvent.click(screen.getByRole("button", { name: "保存思考强度转换规则" }));

    expect(persistSettings).not.toHaveBeenCalled();
    expect(screen.getByLabelText("规则 7 来源强度")).toHaveValue("");
    expect(toast).toHaveBeenCalledWith("第 7 条规则的来源强度不能为空");
  });

  it("rolls a failed reasoning effort mapping save back to the server snapshot", async () => {
    const persistSettings = vi.fn().mockResolvedValue(null);
    render(
      <CliManagerCx2ccTab
        appSettings={createAppSettings()}
        commonSettingsSaving={false}
        onPersistCommonSettings={persistSettings}
      />
    );

    fireEvent.change(screen.getByLabelText("规则 1 目标强度"), {
      target: { value: "xhigh" },
    });
    fireEvent.click(screen.getByRole("button", { name: "保存思考强度转换规则" }));

    await waitFor(() => expect(screen.getByLabelText("规则 1 目标强度")).toHaveValue("low"));
  });

  it("does not overwrite a newer mapping draft when an older save settles", async () => {
    let resolvePersist: ((settings: AppSettings | null) => void) | undefined;
    const persistSettings = vi.fn(
      () =>
        new Promise<AppSettings | null>((resolve) => {
          resolvePersist = resolve;
        })
    );
    render(
      <CliManagerCx2ccTab
        appSettings={createAppSettings()}
        commonSettingsSaving={false}
        onPersistCommonSettings={persistSettings}
      />
    );

    fireEvent.change(screen.getByLabelText("规则 1 目标强度"), {
      target: { value: "xhigh" },
    });
    fireEvent.click(screen.getByRole("button", { name: "保存思考强度转换规则" }));
    fireEvent.change(screen.getByLabelText("规则 1 目标强度"), {
      target: { value: "max" },
    });
    resolvePersist?.(
      createAppSettings({
        cx2cc_reasoning_effort_mappings: [
          { source: "low", target: "xhigh" },
          ...createDefaultCx2ccReasoningEffortMappings().slice(1),
        ],
      })
    );

    await waitFor(() => expect(screen.getByLabelText("规则 1 目标强度")).toHaveValue("max"));
  });

  it("does not roll back a refreshed canonical mapping when an older save fails", async () => {
    let resolvePersist: ((settings: AppSettings | null) => void) | undefined;
    const persistSettings = vi.fn(
      () =>
        new Promise<AppSettings | null>((resolve) => {
          resolvePersist = resolve;
        })
    );
    const initialSettings = createAppSettings();
    const canonicalMappings = [
      { source: "low", target: "xhigh" },
      ...createDefaultCx2ccReasoningEffortMappings().slice(1),
    ];
    const { rerender } = render(
      <CliManagerCx2ccTab
        appSettings={initialSettings}
        commonSettingsSaving={false}
        onPersistCommonSettings={persistSettings}
      />
    );

    fireEvent.change(screen.getByLabelText("规则 1 目标强度"), {
      target: { value: "xhigh" },
    });
    fireEvent.click(screen.getByRole("button", { name: "保存思考强度转换规则" }));
    rerender(
      <CliManagerCx2ccTab
        appSettings={createAppSettings({
          cx2cc_reasoning_effort_mappings: canonicalMappings,
        })}
        commonSettingsSaving={false}
        onPersistCommonSettings={persistSettings}
      />
    );
    resolvePersist?.(null);

    await waitFor(() => expect(screen.getByLabelText("规则 1 目标强度")).toHaveValue("xhigh"));
  });

  it("allows only one reasoning effort mapping save at a time", () => {
    const persistSettings = vi.fn(() => new Promise<AppSettings | null>(() => undefined));
    render(
      <CliManagerCx2ccTab
        appSettings={createAppSettings()}
        commonSettingsSaving={false}
        onPersistCommonSettings={persistSettings}
      />
    );

    fireEvent.change(screen.getByLabelText("规则 1 目标强度"), {
      target: { value: "xhigh" },
    });
    const saveButton = screen.getByRole("button", { name: "保存思考强度转换规则" });
    fireEvent.click(saveButton);
    fireEvent.click(saveButton);

    expect(persistSettings).toHaveBeenCalledTimes(1);
  });

  it("persists opus fallback model on blur", async () => {
    const persistSettings = vi.fn().mockResolvedValue(null);
    render(
      <CliManagerCx2ccTab
        appSettings={createAppSettings()}
        commonSettingsSaving={false}
        onPersistCommonSettings={persistSettings}
      />
    );

    const opusInput = screen.getByDisplayValue("gpt-5.4-opus");
    fireEvent.change(opusInput, { target: { value: "new-opus-model" } });
    fireEvent.blur(opusInput);

    expect(persistSettings).toHaveBeenCalledWith({
      cx2cc_fallback_model_opus: "new-opus-model",
    });
  });

  it("persists sonnet fallback model on blur", async () => {
    const persistSettings = vi.fn().mockResolvedValue(null);
    render(
      <CliManagerCx2ccTab
        appSettings={createAppSettings()}
        commonSettingsSaving={false}
        onPersistCommonSettings={persistSettings}
      />
    );

    const sonnetInput = screen.getByDisplayValue("gpt-5.4-sonnet");
    fireEvent.change(sonnetInput, { target: { value: "new-sonnet-model" } });
    fireEvent.blur(sonnetInput);

    expect(persistSettings).toHaveBeenCalledWith({
      cx2cc_fallback_model_sonnet: "new-sonnet-model",
    });
  });

  it("persists haiku fallback model on blur", async () => {
    const persistSettings = vi.fn().mockResolvedValue(null);
    render(
      <CliManagerCx2ccTab
        appSettings={createAppSettings()}
        commonSettingsSaving={false}
        onPersistCommonSettings={persistSettings}
      />
    );

    const haikuInput = screen.getByDisplayValue("gpt-5.4-haiku");
    fireEvent.change(haikuInput, { target: { value: "new-haiku-model" } });
    fireEvent.blur(haikuInput);

    expect(persistSettings).toHaveBeenCalledWith({
      cx2cc_fallback_model_haiku: "new-haiku-model",
    });
  });

  it("persists main fallback model on blur", async () => {
    const persistSettings = vi.fn().mockResolvedValue(null);
    render(
      <CliManagerCx2ccTab
        appSettings={createAppSettings()}
        commonSettingsSaving={false}
        onPersistCommonSettings={persistSettings}
      />
    );

    const mainInput = screen.getByDisplayValue("gpt-5.4-main");
    fireEvent.change(mainInput, { target: { value: "new-main-model" } });
    fireEvent.blur(mainInput);

    expect(persistSettings).toHaveBeenCalledWith({
      cx2cc_fallback_model_main: "new-main-model",
    });
  });

  it("does not persist empty fallback model on blur", async () => {
    const persistSettings = vi.fn().mockResolvedValue(null);
    render(
      <CliManagerCx2ccTab
        appSettings={createAppSettings()}
        commonSettingsSaving={false}
        onPersistCommonSettings={persistSettings}
      />
    );

    const opusInput = screen.getByDisplayValue("gpt-5.4-opus");
    fireEvent.change(opusInput, { target: { value: "" } });
    fireEvent.blur(opusInput);

    expect(persistSettings).not.toHaveBeenCalled();
  });

  it("rejects oversized fallback model before persisting", async () => {
    const persistSettings = vi.fn().mockResolvedValue(null);
    render(
      <CliManagerCx2ccTab
        appSettings={createAppSettings()}
        commonSettingsSaving={false}
        onPersistCommonSettings={persistSettings}
      />
    );

    const opusInput = screen.getByDisplayValue("gpt-5.4-opus");
    fireEvent.change(opusInput, { target: { value: "x".repeat(129) } });
    fireEvent.blur(opusInput);

    expect(persistSettings).not.toHaveBeenCalled();
    expect(toast).toHaveBeenCalledWith("Opus 默认模型必须 <= 128 字符");
  });

  it("persists service tier on blur", async () => {
    const persistSettings = vi.fn().mockResolvedValue(null);
    render(
      <CliManagerCx2ccTab
        appSettings={createAppSettings()}
        commonSettingsSaving={false}
        onPersistCommonSettings={persistSettings}
      />
    );

    const serviceTierInput = screen.getByDisplayValue("fast");
    fireEvent.change(serviceTierInput, { target: { value: "standard" } });
    fireEvent.blur(serviceTierInput);

    expect(persistSettings).toHaveBeenCalledWith({
      cx2cc_service_tier: "standard",
    });
  });

  it("persists empty service tier on blur", async () => {
    const persistSettings = vi.fn().mockResolvedValue(null);
    render(
      <CliManagerCx2ccTab
        appSettings={createAppSettings()}
        commonSettingsSaving={false}
        onPersistCommonSettings={persistSettings}
      />
    );

    const serviceTierInput = screen.getByDisplayValue("fast");
    fireEvent.change(serviceTierInput, { target: { value: "" } });
    fireEvent.blur(serviceTierInput);

    expect(persistSettings).toHaveBeenCalledWith({
      cx2cc_service_tier: "",
    });
  });

  it("rejects service tier control characters before persisting", async () => {
    const persistSettings = vi.fn().mockResolvedValue(null);
    render(
      <CliManagerCx2ccTab
        appSettings={createAppSettings()}
        commonSettingsSaving={false}
        onPersistCommonSettings={persistSettings}
      />
    );

    const serviceTierInput = screen.getByDisplayValue("fast");
    fireEvent.change(serviceTierInput, { target: { value: "standard\u0001" } });
    fireEvent.blur(serviceTierInput);

    expect(persistSettings).not.toHaveBeenCalled();
    expect(toast).toHaveBeenCalledWith("服务层级不能包含控制字符");
  });

  it("omits the legacy fixed reasoning effort control without rewriting its value", () => {
    const persistSettings = vi.fn().mockResolvedValue(null);
    render(
      <CliManagerCx2ccTab
        appSettings={createAppSettings({ cx2cc_model_reasoning_effort: "ultra" })}
        commonSettingsSaving={false}
        onPersistCommonSettings={persistSettings}
      />
    );

    expect(screen.queryByText("推理强度")).not.toBeInTheDocument();
    expect(screen.queryByRole("radio")).not.toBeInTheDocument();
    expect(screen.queryByText("ultra")).not.toBeInTheDocument();

    const serviceTierInput = screen.getByDisplayValue("fast");
    fireEvent.change(serviceTierInput, { target: { value: "standard" } });
    fireEvent.blur(serviceTierInput);

    expect(persistSettings).toHaveBeenCalledWith({
      cx2cc_service_tier: "standard",
    });
  });

  it("persists disable response storage switch change", () => {
    const persistSettings = vi.fn().mockResolvedValue(null);
    render(
      <CliManagerCx2ccTab
        appSettings={createAppSettings({ cx2cc_disable_response_storage: true })}
        commonSettingsSaving={false}
        onPersistCommonSettings={persistSettings}
      />
    );

    const switches = screen.getAllByRole("switch");
    // First switch is "禁用响应存储"
    fireEvent.click(switches[0]);

    expect(persistSettings).toHaveBeenCalledWith({
      cx2cc_disable_response_storage: false,
    });
  });

  it("persists enable reasoning to thinking switch change", () => {
    const persistSettings = vi.fn().mockResolvedValue(null);
    render(
      <CliManagerCx2ccTab
        appSettings={createAppSettings({ cx2cc_enable_reasoning_to_thinking: true })}
        commonSettingsSaving={false}
        onPersistCommonSettings={persistSettings}
      />
    );

    const switches = screen.getAllByRole("switch");
    // Second switch is "启用推理转思考"
    fireEvent.click(switches[1]);

    expect(persistSettings).toHaveBeenCalledWith({
      cx2cc_enable_reasoning_to_thinking: false,
    });
  });

  it("persists drop stop sequences switch change", () => {
    const persistSettings = vi.fn().mockResolvedValue(null);
    render(
      <CliManagerCx2ccTab
        appSettings={createAppSettings({ cx2cc_drop_stop_sequences: true })}
        commonSettingsSaving={false}
        onPersistCommonSettings={persistSettings}
      />
    );

    const switches = screen.getAllByRole("switch");
    // Third switch is "丢弃停止序列"
    fireEvent.click(switches[2]);

    expect(persistSettings).toHaveBeenCalledWith({
      cx2cc_drop_stop_sequences: false,
    });
  });

  it("persists clean schema switch change", () => {
    const persistSettings = vi.fn().mockResolvedValue(null);
    render(
      <CliManagerCx2ccTab
        appSettings={createAppSettings({ cx2cc_clean_schema: true })}
        commonSettingsSaving={false}
        onPersistCommonSettings={persistSettings}
      />
    );

    const switches = screen.getAllByRole("switch");
    // Fourth switch is "清理 Schema"
    fireEvent.click(switches[3]);

    expect(persistSettings).toHaveBeenCalledWith({
      cx2cc_clean_schema: false,
    });
  });

  it("persists filter batch tool switch change", () => {
    const persistSettings = vi.fn().mockResolvedValue(null);
    render(
      <CliManagerCx2ccTab
        appSettings={createAppSettings({ cx2cc_filter_batch_tool: true })}
        commonSettingsSaving={false}
        onPersistCommonSettings={persistSettings}
      />
    );

    const switches = screen.getAllByRole("switch");
    // Fifth switch is "过滤 BatchTool"
    fireEvent.click(switches[4]);

    expect(persistSettings).toHaveBeenCalledWith({
      cx2cc_filter_batch_tool: false,
    });
  });

  it("disables controls when saving", () => {
    const persistSettings = vi.fn().mockResolvedValue(null);
    render(
      <CliManagerCx2ccTab
        appSettings={createAppSettings()}
        commonSettingsSaving={true}
        onPersistCommonSettings={persistSettings}
      />
    );

    const opusInput = screen.getByDisplayValue("gpt-5.4-opus");
    expect(opusInput).toBeDisabled();

    const switches = screen.getAllByRole("switch");
    switches.forEach((switchEl) => {
      expect(switchEl).toBeDisabled();
    });

    expect(screen.queryByRole("radio")).not.toBeInTheDocument();
  });

  it("disables controls when appSettings is null", () => {
    const persistSettings = vi.fn().mockResolvedValue(null);
    render(
      <CliManagerCx2ccTab
        appSettings={null}
        commonSettingsSaving={false}
        onPersistCommonSettings={persistSettings}
      />
    );

    const switches = screen.getAllByRole("switch");
    switches.forEach((switchEl) => {
      expect(switchEl).toBeDisabled();
    });
  });

  it("updates local state when appSettings changes", () => {
    const persistSettings = vi.fn().mockResolvedValue(null);
    const { rerender } = render(
      <CliManagerCx2ccTab
        appSettings={createAppSettings({
          cx2cc_fallback_model_opus: "old-model",
          cx2cc_model_reasoning_effort: "low",
        })}
        commonSettingsSaving={false}
        onPersistCommonSettings={persistSettings}
      />
    );

    expect(screen.getByDisplayValue("old-model")).toBeInTheDocument();
    expect(screen.queryByRole("radio")).not.toBeInTheDocument();

    rerender(
      <CliManagerCx2ccTab
        appSettings={createAppSettings({
          cx2cc_fallback_model_opus: "new-model",
          cx2cc_model_reasoning_effort: "xhigh",
        })}
        commonSettingsSaving={false}
        onPersistCommonSettings={persistSettings}
      />
    );

    expect(screen.getByDisplayValue("new-model")).toBeInTheDocument();
    expect(screen.queryByRole("radio")).not.toBeInTheDocument();
  });
});
