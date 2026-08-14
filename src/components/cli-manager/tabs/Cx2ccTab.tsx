import { useReducer, useRef, type ReactNode } from "react";
import { toast } from "sonner";
import { Plus, RotateCcw, Save, Settings, Trash2 } from "lucide-react";
import {
  createDefaultCx2ccReasoningEffortMappings,
  CX2CC_PROVIDER_DEFAULT_MODEL,
  CX2CC_REASONING_EFFORT_MAPPING_MAX_RULES,
  type Cx2ccReasoningEffortMapping,
} from "../../../constants/cx2cc";
import type { AppSettings } from "../../../services/settings/settings";
import {
  validateCx2ccFallbackModel,
  validateCx2ccOptionalField,
  validateCx2ccReasoningEffortMappings,
} from "../../../services/settings/settingsValidation";
import { cn } from "../../../utils/cn";
import { Button } from "../../../ui/Button";
import { Card } from "../../../ui/Card";
import { Input } from "../../../ui/Input";
import { Switch } from "../../../ui/Switch";
import { Tooltip } from "../../../ui/Tooltip";

export type CliManagerCx2ccTabProps = {
  appSettings: AppSettings | null;
  commonSettingsSaving: boolean;
  onPersistCommonSettings: (patch: Partial<AppSettings>) => Promise<AppSettings | null>;
};

type Cx2ccTextSettingKey =
  | "cx2cc_fallback_model_opus"
  | "cx2cc_fallback_model_sonnet"
  | "cx2cc_fallback_model_haiku"
  | "cx2cc_fallback_model_main"
  | "cx2cc_service_tier";

type Cx2ccDraftKey = Cx2ccTextSettingKey;

type Cx2ccDraftState = {
  sourceKey: string;
  sourceRevision: number;
  values: Record<Cx2ccDraftKey, string>;
  reasoningEffortMappings: Cx2ccReasoningEffortMapping[];
};

type Cx2ccDraftAction =
  | { type: "resetFromSettings"; state: Cx2ccDraftState }
  | { type: "setValue"; key: Cx2ccDraftKey; value: string }
  | { type: "setReasoningEffortMappings"; mappings: Cx2ccReasoningEffortMapping[] }
  | {
      type: "settleReasoningEffortMappings";
      expectedSourceRevision: number;
      expected: Cx2ccReasoningEffortMapping[];
      mappings: Cx2ccReasoningEffortMapping[];
    };

const EMPTY_CX2CC_DRAFT_VALUES: Record<Cx2ccDraftKey, string> = {
  cx2cc_fallback_model_opus: "",
  cx2cc_fallback_model_sonnet: "",
  cx2cc_fallback_model_haiku: "",
  cx2cc_fallback_model_main: "",
  cx2cc_service_tier: "",
};

function createCx2ccDraftState(appSettings: AppSettings | null): Cx2ccDraftState {
  if (!appSettings) {
    return {
      sourceKey: "empty",
      sourceRevision: 0,
      values: EMPTY_CX2CC_DRAFT_VALUES,
      reasoningEffortMappings: createDefaultCx2ccReasoningEffortMappings(),
    };
  }

  const values: Record<Cx2ccDraftKey, string> = {
    cx2cc_fallback_model_opus: appSettings.cx2cc_fallback_model_opus,
    cx2cc_fallback_model_sonnet: appSettings.cx2cc_fallback_model_sonnet,
    cx2cc_fallback_model_haiku: appSettings.cx2cc_fallback_model_haiku,
    cx2cc_fallback_model_main: appSettings.cx2cc_fallback_model_main,
    cx2cc_service_tier: appSettings.cx2cc_service_tier,
  };

  return {
    sourceKey: `${Object.values(values).join("\u0000")}\u0000${JSON.stringify(
      appSettings.cx2cc_reasoning_effort_mappings
    )}`,
    sourceRevision: 0,
    values,
    reasoningEffortMappings: appSettings.cx2cc_reasoning_effort_mappings.map((mapping) => ({
      ...mapping,
    })),
  };
}

function cx2ccDraftReducer(state: Cx2ccDraftState, action: Cx2ccDraftAction): Cx2ccDraftState {
  if (action.type === "resetFromSettings") {
    return {
      ...action.state,
      sourceRevision: state.sourceRevision + 1,
    };
  }
  if (action.type === "setReasoningEffortMappings") {
    return {
      ...state,
      reasoningEffortMappings: action.mappings,
    };
  }
  if (action.type === "settleReasoningEffortMappings") {
    if (
      state.sourceRevision !== action.expectedSourceRevision ||
      !reasoningEffortMappingsEqual(state.reasoningEffortMappings, action.expected)
    ) {
      return state;
    }
    return {
      ...state,
      reasoningEffortMappings: action.mappings,
    };
  }
  return {
    ...state,
    values: {
      ...state.values,
      [action.key]: action.value,
    },
  };
}

function cloneReasoningEffortMappings(
  mappings: readonly Cx2ccReasoningEffortMapping[]
): Cx2ccReasoningEffortMapping[] {
  return mappings.map((mapping) => ({ ...mapping }));
}

function normalizeReasoningEffortMappings(
  mappings: readonly Cx2ccReasoningEffortMapping[]
): Cx2ccReasoningEffortMapping[] {
  return mappings.map((mapping) => ({
    source: mapping.source.trim(),
    target: mapping.target.trim(),
  }));
}

function reasoningEffortMappingsEqual(
  left: readonly Cx2ccReasoningEffortMapping[],
  right: readonly Cx2ccReasoningEffortMapping[]
): boolean {
  return (
    left.length === right.length &&
    left.every(
      (mapping, index) =>
        mapping.source === right[index]?.source && mapping.target === right[index]?.target
    )
  );
}

function SettingItem({
  label,
  subtitle,
  children,
  className,
}: {
  label: string;
  subtitle: string;
  children: ReactNode;
  className?: string;
}) {
  return (
    <div
      className={cn(
        "flex flex-col gap-2 py-3 sm:flex-row sm:items-start sm:justify-between",
        className
      )}
    >
      <div className="min-w-0">
        <div className="text-sm text-secondary-foreground">{label}</div>
        <div className="mt-1 text-xs text-muted-foreground leading-relaxed">{subtitle}</div>
      </div>
      <div className="flex flex-wrap items-center justify-end gap-2">{children}</div>
    </div>
  );
}

export function CliManagerCx2ccTab({
  appSettings,
  commonSettingsSaving,
  onPersistCommonSettings,
}: CliManagerCx2ccTabProps) {
  const nextDraftState = createCx2ccDraftState(appSettings);
  const [draftState, dispatchDraft] = useReducer(cx2ccDraftReducer, nextDraftState);
  const reasoningEffortSaveInFlight = useRef(false);
  const effectiveDraftState =
    draftState.sourceKey === nextDraftState.sourceKey ? draftState : nextDraftState;
  if (draftState.sourceKey !== nextDraftState.sourceKey) {
    dispatchDraft({ type: "resetFromSettings", state: nextDraftState });
  }

  const fallbackModelOpusText = effectiveDraftState.values.cx2cc_fallback_model_opus;
  const fallbackModelSonnetText = effectiveDraftState.values.cx2cc_fallback_model_sonnet;
  const fallbackModelHaikuText = effectiveDraftState.values.cx2cc_fallback_model_haiku;
  const fallbackModelMainText = effectiveDraftState.values.cx2cc_fallback_model_main;
  const serviceTierText = effectiveDraftState.values.cx2cc_service_tier;
  const reasoningEffortMappings = effectiveDraftState.reasoningEffortMappings;

  const controlsDisabled = commonSettingsSaving || !appSettings;
  const reasoningEffortMappingsDirty = appSettings
    ? !reasoningEffortMappingsEqual(
        reasoningEffortMappings,
        appSettings.cx2cc_reasoning_effort_mappings
      )
    : false;

  function setDraftValue(key: Cx2ccDraftKey, value: string) {
    dispatchDraft({ type: "setValue", key, value });
  }

  function setReasoningEffortMappings(mappings: Cx2ccReasoningEffortMapping[]) {
    dispatchDraft({ type: "setReasoningEffortMappings", mappings });
  }

  function updateReasoningEffortMapping(
    index: number,
    field: keyof Cx2ccReasoningEffortMapping,
    value: string
  ) {
    const next = cloneReasoningEffortMappings(reasoningEffortMappings);
    const mapping = next[index];
    if (!mapping) return;
    mapping[field] = value;
    setReasoningEffortMappings(next);
  }

  async function persistReasoningEffortMappings(candidate: readonly Cx2ccReasoningEffortMapping[]) {
    if (!appSettings) return;
    if (reasoningEffortSaveInFlight.current) return;

    const previous = cloneReasoningEffortMappings(appSettings.cx2cc_reasoning_effort_mappings);
    const sourceRevision = effectiveDraftState.sourceRevision;
    const normalized = normalizeReasoningEffortMappings(candidate);
    const validationMessage = validateCx2ccReasoningEffortMappings(normalized);
    if (validationMessage) {
      toast(validationMessage);
      return;
    }

    reasoningEffortSaveInFlight.current = true;
    setReasoningEffortMappings(normalized);
    try {
      const updated = await onPersistCommonSettings({
        cx2cc_reasoning_effort_mappings: normalized,
      });
      dispatchDraft({
        type: "settleReasoningEffortMappings",
        expectedSourceRevision: sourceRevision,
        expected: normalized,
        mappings: cloneReasoningEffortMappings(
          updated?.cx2cc_reasoning_effort_mappings ?? previous
        ),
      });
    } catch {
      dispatchDraft({
        type: "settleReasoningEffortMappings",
        expectedSourceRevision: sourceRevision,
        expected: normalized,
        mappings: previous,
      });
    } finally {
      reasoningEffortSaveInFlight.current = false;
    }
  }

  async function persistFallbackModel(
    key: Exclude<Cx2ccTextSettingKey, "cx2cc_service_tier">,
    label: string,
    value: string
  ) {
    if (!appSettings) return;

    const previous = appSettings[key];
    const trimmed = value.trim();
    setDraftValue(key, trimmed);

    const validationMessage = validateCx2ccFallbackModel(label, trimmed);
    if (validationMessage) {
      toast(validationMessage);
      setDraftValue(key, previous);
      return;
    }

    const updated = await onPersistCommonSettings({ [key]: trimmed } as Partial<AppSettings>);
    setDraftValue(key, updated ? updated[key] : previous);
  }

  async function persistOptionalTextSetting(
    key: Extract<Cx2ccTextSettingKey, "cx2cc_service_tier">,
    label: string,
    value: string
  ) {
    if (!appSettings) return;

    const previous = appSettings[key];
    const trimmed = value.trim();
    setDraftValue(key, trimmed);

    const validationMessage = validateCx2ccOptionalField(label, trimmed);
    if (validationMessage) {
      toast(validationMessage);
      setDraftValue(key, previous);
      return;
    }

    const updated = await onPersistCommonSettings({ [key]: trimmed } as Partial<AppSettings>);
    setDraftValue(key, updated ? updated[key] : previous);
  }

  return (
    <div className="space-y-6">
      <Card className="overflow-hidden p-5">
        <h3 className="mb-3 flex items-center gap-2 text-sm font-semibold text-foreground">
          <Settings className="h-4 w-4 text-muted-foreground" />
          模型 Fallback 映射
        </h3>
        <div className="divide-y divide-border">
          <SettingItem label="Opus 默认模型" subtitle="当 Provider 未设置 Opus 覆盖时使用此模型">
            <Input
              value={fallbackModelOpusText}
              onChange={(e) => setDraftValue("cx2cc_fallback_model_opus", e.currentTarget.value)}
              onBlur={(e) => {
                void persistFallbackModel(
                  "cx2cc_fallback_model_opus",
                  "Opus 默认模型",
                  e.currentTarget.value
                );
              }}
              placeholder={CX2CC_PROVIDER_DEFAULT_MODEL}
              className="font-mono w-[240px] max-w-full"
              disabled={controlsDisabled}
            />
          </SettingItem>

          <SettingItem
            label="Sonnet 默认模型"
            subtitle="当 Provider 未设置 Sonnet 覆盖时使用此模型"
          >
            <Input
              value={fallbackModelSonnetText}
              onChange={(e) => setDraftValue("cx2cc_fallback_model_sonnet", e.currentTarget.value)}
              onBlur={(e) => {
                void persistFallbackModel(
                  "cx2cc_fallback_model_sonnet",
                  "Sonnet 默认模型",
                  e.currentTarget.value
                );
              }}
              placeholder={CX2CC_PROVIDER_DEFAULT_MODEL}
              className="font-mono w-[240px] max-w-full"
              disabled={controlsDisabled}
            />
          </SettingItem>

          <SettingItem label="Haiku 默认模型" subtitle="当 Provider 未设置 Haiku 覆盖时使用此模型">
            <Input
              value={fallbackModelHaikuText}
              onChange={(e) => setDraftValue("cx2cc_fallback_model_haiku", e.currentTarget.value)}
              onBlur={(e) => {
                void persistFallbackModel(
                  "cx2cc_fallback_model_haiku",
                  "Haiku 默认模型",
                  e.currentTarget.value
                );
              }}
              placeholder={CX2CC_PROVIDER_DEFAULT_MODEL}
              className="font-mono w-[240px] max-w-full"
              disabled={controlsDisabled}
            />
          </SettingItem>

          <SettingItem label="主模型默认" subtitle="当 Provider 未设置 Main 覆盖时使用此模型">
            <Input
              value={fallbackModelMainText}
              onChange={(e) => setDraftValue("cx2cc_fallback_model_main", e.currentTarget.value)}
              onBlur={(e) => {
                void persistFallbackModel(
                  "cx2cc_fallback_model_main",
                  "主模型默认",
                  e.currentTarget.value
                );
              }}
              placeholder={CX2CC_PROVIDER_DEFAULT_MODEL}
              className="font-mono w-[240px] max-w-full"
              disabled={controlsDisabled}
            />
          </SettingItem>
        </div>
      </Card>

      <Card className="overflow-hidden p-5">
        <div className="mb-3 flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
          <h3 className="flex items-center gap-2 text-sm font-semibold text-foreground">
            <Settings className="h-4 w-4 text-muted-foreground" />
            思考强度转换
          </h3>
          <div className="flex flex-wrap items-center gap-2">
            <Tooltip content="恢复默认思考强度转换规则">
              <Button
                variant="secondary"
                size="sm"
                aria-label="恢复默认思考强度转换规则"
                disabled={controlsDisabled}
                onClick={() => {
                  void persistReasoningEffortMappings(createDefaultCx2ccReasoningEffortMappings());
                }}
              >
                <RotateCcw className="h-3.5 w-3.5" aria-hidden="true" />
                恢复默认
              </Button>
            </Tooltip>
            <Tooltip content="添加思考强度转换规则">
              <Button
                variant="secondary"
                size="sm"
                aria-label="添加思考强度转换规则"
                disabled={
                  controlsDisabled ||
                  reasoningEffortMappings.length >= CX2CC_REASONING_EFFORT_MAPPING_MAX_RULES
                }
                onClick={() => {
                  setReasoningEffortMappings([
                    ...reasoningEffortMappings,
                    { source: "", target: "" },
                  ]);
                }}
              >
                <Plus className="h-3.5 w-3.5" aria-hidden="true" />
                添加规则
              </Button>
            </Tooltip>
          </div>
        </div>

        <div className="mb-2 text-xs leading-relaxed text-muted-foreground">
          按来源强度精确匹配；未命中时原样透传。
        </div>

        <div className="divide-y divide-border border-y border-border">
          {reasoningEffortMappings.map((mapping, index) => (
            <div
              key={index}
              role="group"
              aria-label={`思考强度转换规则 ${index + 1}`}
              className="grid gap-3 py-3 sm:grid-cols-[minmax(0,1fr)_minmax(0,1fr)_2rem] sm:items-end"
            >
              <label className="min-w-0 space-y-1.5 text-xs font-medium text-muted-foreground">
                来源强度
                <Input
                  value={mapping.source}
                  aria-label={`规则 ${index + 1} 来源强度`}
                  placeholder="例如: ultra"
                  className="w-full font-mono"
                  disabled={controlsDisabled}
                  onChange={(event) =>
                    updateReasoningEffortMapping(index, "source", event.currentTarget.value)
                  }
                />
              </label>
              <label className="min-w-0 space-y-1.5 text-xs font-medium text-muted-foreground">
                目标强度
                <Input
                  value={mapping.target}
                  aria-label={`规则 ${index + 1} 目标强度`}
                  placeholder="例如: max"
                  className="w-full font-mono"
                  disabled={controlsDisabled}
                  onChange={(event) =>
                    updateReasoningEffortMapping(index, "target", event.currentTarget.value)
                  }
                />
              </label>
              <div className="flex h-8 items-center justify-end">
                <Tooltip content={`删除思考强度转换规则 ${index + 1}`}>
                  <Button
                    variant="ghost"
                    size="icon"
                    aria-label={`删除思考强度转换规则 ${index + 1}`}
                    disabled={controlsDisabled}
                    onClick={() =>
                      setReasoningEffortMappings(
                        reasoningEffortMappings.filter((_, mappingIndex) => mappingIndex !== index)
                      )
                    }
                  >
                    <Trash2 className="h-4 w-4 text-destructive" aria-hidden="true" />
                  </Button>
                </Tooltip>
              </div>
            </div>
          ))}
          {reasoningEffortMappings.length === 0 ? (
            <div className="py-4 text-xs text-muted-foreground">
              暂无转换规则；所有显式强度将原样透传。
            </div>
          ) : null}
        </div>

        <div className="mt-3 flex flex-wrap items-center justify-between gap-3">
          <span className="text-xs text-muted-foreground">
            {reasoningEffortMappings.length}/{CX2CC_REASONING_EFFORT_MAPPING_MAX_RULES} 条
          </span>
          <Button
            variant="primary"
            size="sm"
            aria-label="保存思考强度转换规则"
            disabled={controlsDisabled || !reasoningEffortMappingsDirty}
            onClick={() => {
              void persistReasoningEffortMappings(reasoningEffortMappings);
            }}
          >
            <Save className="h-3.5 w-3.5" aria-hidden="true" />
            保存规则
          </Button>
        </div>
      </Card>

      <Card className="overflow-hidden p-5">
        <h3 className="mb-3 flex items-center gap-2 text-sm font-semibold text-foreground">
          <Settings className="h-4 w-4 text-muted-foreground" />
          上游请求注入
        </h3>
        <div className="divide-y divide-border">
          <SettingItem label="服务层级" subtitle="注入 service_tier 到上游请求；留空表示不注入。">
            <Input
              value={serviceTierText}
              onChange={(e) => setDraftValue("cx2cc_service_tier", e.currentTarget.value)}
              onBlur={(e) => {
                void persistOptionalTextSetting(
                  "cx2cc_service_tier",
                  "服务层级",
                  e.currentTarget.value
                );
              }}
              placeholder="例如: fast"
              className="font-mono w-[240px] max-w-full"
              disabled={controlsDisabled}
            />
          </SettingItem>

          <SettingItem label="禁用响应存储" subtitle="注入 store: false 到上游请求">
            <Switch
              checked={appSettings?.cx2cc_disable_response_storage ?? true}
              onCheckedChange={(checked) => {
                void onPersistCommonSettings({ cx2cc_disable_response_storage: checked });
              }}
              disabled={controlsDisabled}
            />
          </SettingItem>
        </div>
      </Card>

      <Card className="overflow-hidden p-5">
        <h3 className="mb-3 flex items-center gap-2 text-sm font-semibold text-foreground">
          <Settings className="h-4 w-4 text-muted-foreground" />
          转换行为开关
        </h3>
        <div className="divide-y divide-border">
          <SettingItem
            label="启用推理转思考"
            subtitle="将上游 reasoning 输出转换为 Claude thinking 格式"
          >
            <Switch
              checked={appSettings?.cx2cc_enable_reasoning_to_thinking ?? true}
              onCheckedChange={(checked) => {
                void onPersistCommonSettings({
                  cx2cc_enable_reasoning_to_thinking: checked,
                });
              }}
              disabled={controlsDisabled}
            />
          </SettingItem>

          <SettingItem label="丢弃停止序列" subtitle="丢弃 stop_sequences（Responses API 不支持）">
            <Switch
              checked={appSettings?.cx2cc_drop_stop_sequences ?? true}
              onCheckedChange={(checked) => {
                void onPersistCommonSettings({ cx2cc_drop_stop_sequences: checked });
              }}
              disabled={controlsDisabled}
            />
          </SettingItem>

          <SettingItem
            label="清理 Schema"
            subtitle='移除工具 schema 中的 format: "uri"（Responses API 不支持）'
          >
            <Switch
              checked={appSettings?.cx2cc_clean_schema ?? true}
              onCheckedChange={(checked) => {
                void onPersistCommonSettings({ cx2cc_clean_schema: checked });
              }}
              disabled={controlsDisabled}
            />
          </SettingItem>

          <SettingItem label="过滤 BatchTool" subtitle="过滤掉 BatchTool 类型的工具">
            <Switch
              checked={appSettings?.cx2cc_filter_batch_tool ?? true}
              onCheckedChange={(checked) => {
                void onPersistCommonSettings({ cx2cc_filter_batch_tool: checked });
              }}
              disabled={controlsDisabled}
            />
          </SettingItem>
        </div>
      </Card>
    </div>
  );
}
