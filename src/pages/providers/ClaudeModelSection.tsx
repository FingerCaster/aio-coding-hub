import { ChevronDown } from "lucide-react";
import { CX2CC_CONTEXT_WINDOW_MAX, CX2CC_CONTEXT_WINDOW_MIN } from "../../constants/cx2cc";
import { FormField } from "../../ui/FormField";
import { Input } from "../../ui/Input";
import type { UseProviderEditorFormReturn } from "./useProviderEditorForm";

type Cx2ccModelField = "haiku_model" | "sonnet_model" | "opus_model";
type Cx2ccContextWindowField =
  | "main_context_window"
  | "haiku_context_window"
  | "sonnet_context_window"
  | "opus_context_window";

const CX2CC_MAPPED_MODEL_FIELDS: ReadonlyArray<{
  modelField: Cx2ccModelField;
  contextField: Cx2ccContextWindowField;
  label: string;
  hint: string;
  placeholder: string;
}> = [
  {
    modelField: "haiku_model",
    contextField: "haiku_context_window",
    label: "Haiku 默认模型",
    hint: "当请求模型名包含 haiku 时使用（子串匹配）",
    placeholder: "例如: glm-4-plus-haiku",
  },
  {
    modelField: "sonnet_model",
    contextField: "sonnet_context_window",
    label: "Sonnet 默认模型",
    hint: "当请求模型名包含 sonnet 时使用（子串匹配）",
    placeholder: "例如: glm-4-plus-sonnet",
  },
  {
    modelField: "opus_model",
    contextField: "opus_context_window",
    label: "Opus 默认模型",
    hint: "当请求模型名包含 opus 时使用（子串匹配）",
    placeholder: "例如: glm-4-plus-opus",
  },
];

function ModelAndContextInputs(props: {
  isCx2cc: boolean;
  id: string;
  hintId?: string;
  modelValue: string;
  onModelChange: (value: string) => void;
  modelPlaceholder: string;
  contextLabel: string;
  contextValue: number | null | undefined;
  onContextChange: (value: string) => void;
  disabled: boolean;
}) {
  const modelInput = (
    <Input
      id={props.id}
      aria-describedby={props.hintId}
      value={props.modelValue}
      onChange={(event) => props.onModelChange(event.currentTarget.value)}
      placeholder={props.modelPlaceholder}
      disabled={props.disabled}
    />
  );

  if (!props.isCx2cc) return modelInput;

  return (
    <div className="grid gap-2 sm:grid-cols-[minmax(0,1fr)_minmax(11rem,14rem)]">
      {modelInput}
      <Input
        aria-label={`${props.contextLabel}上下文窗口`}
        type="number"
        min={CX2CC_CONTEXT_WINDOW_MIN}
        max={CX2CC_CONTEXT_WINDOW_MAX}
        step={1}
        inputMode="numeric"
        value={props.contextValue == null ? "" : String(props.contextValue)}
        onChange={(event) => props.onContextChange(event.currentTarget.value)}
        placeholder="上下文窗口（可选）"
        disabled={props.disabled}
      />
    </div>
  );
}

export function ClaudeModelSection(props: { form: UseProviderEditorFormReturn }) {
  const { claudeModels, setClaudeModels, claudeModelCount, saving, cliKey, authMode } = props.form;

  // Show for all claude providers (api_key, oauth, cx2cc) — each provider can
  // independently override which upstream model to use per model tier.
  if (cliKey !== "claude") return null;

  const isCx2cc = authMode === "cx2cc";
  const sectionTitle = isCx2cc ? "CX2CC 模型映射" : "Claude 模型映射";
  const mainPlaceholder = isCx2cc
    ? "例如: gpt-5.4 / o3"
    : "例如: glm-4-plus / minimax-text-01 / kimi-k2";
  const mainHint = isCx2cc
    ? "默认兜底模型；未命中 haiku/sonnet/opus 时使用"
    : "默认兜底模型；未命中 haiku/sonnet/opus 且未启用 Thinking 时使用";

  function updateContextWindow(field: Cx2ccContextWindowField, raw: string) {
    const value = raw.trim() ? Number(raw) : null;
    setClaudeModels((prev) => ({ ...prev, [field]: value }));
  }

  function updateMappedModel(
    modelField: Cx2ccModelField,
    contextField: Cx2ccContextWindowField,
    value: string
  ) {
    setClaudeModels((prev) => ({
      ...prev,
      [modelField]: value,
      ...(isCx2cc ? { [contextField]: null } : {}),
    }));
  }

  return (
    <details className="group rounded-xl border border-border bg-white shadow-sm open:ring-2 open:ring-accent/10 transition-all dark:border-border dark:bg-secondary">
      <summary className="flex cursor-pointer items-center justify-between px-4 py-3 select-none">
        <div className="flex items-center gap-3">
          <span className="text-sm font-medium text-secondary-foreground group-open:text-accent dark:text-secondary-foreground">
            {sectionTitle}
          </span>
          <span className="text-xs font-mono text-muted-foreground">
            已配置 {claudeModelCount}/{isCx2cc ? 4 : 5}
          </span>
        </div>
        <ChevronDown className="h-4 w-4 text-muted-foreground transition-transform group-open:rotate-180" />
      </summary>

      <div className="space-y-4 border-t border-border px-4 py-3 dark:border-border">
        <FormField label="主模型" hint={mainHint}>
          {(id, hintId) => (
            <ModelAndContextInputs
              isCx2cc={isCx2cc}
              id={id}
              hintId={hintId}
              modelValue={claudeModels.main_model ?? ""}
              onModelChange={(value) => {
                setClaudeModels((prev) => {
                  const oldMain = (prev.main_model ?? "").trim();
                  const syncIfMatch = (field: string | null | undefined) => {
                    const trimmed = (field ?? "").trim();
                    return !trimmed || trimmed === oldMain ? value : field;
                  };
                  const nextHaiku = syncIfMatch(prev.haiku_model);
                  const nextSonnet = syncIfMatch(prev.sonnet_model);
                  const nextOpus = syncIfMatch(prev.opus_model);
                  return {
                    ...prev,
                    main_model: value,
                    haiku_model: nextHaiku,
                    sonnet_model: nextSonnet,
                    opus_model: nextOpus,
                    ...(isCx2cc
                      ? {
                          main_context_window: null,
                          ...(nextHaiku !== prev.haiku_model ? { haiku_context_window: null } : {}),
                          ...(nextSonnet !== prev.sonnet_model
                            ? { sonnet_context_window: null }
                            : {}),
                          ...(nextOpus !== prev.opus_model ? { opus_context_window: null } : {}),
                        }
                      : {}),
                  };
                });
              }}
              modelPlaceholder={mainPlaceholder}
              contextLabel="主模型"
              contextValue={claudeModels.main_context_window}
              onContextChange={(value) => updateContextWindow("main_context_window", value)}
              disabled={saving}
            />
          )}
        </FormField>

        {!isCx2cc ? (
          <FormField
            label="推理模型 (Thinking)"
            hint="当请求中 thinking.type=enabled 且未命中 Haiku/Sonnet/Opus 槽位时使用"
          >
            {(id, hintId) => (
              <Input
                id={id}
                aria-describedby={hintId}
                value={claudeModels.reasoning_model ?? ""}
                onChange={(event) => {
                  const value = event.currentTarget.value;
                  setClaudeModels((prev) => ({ ...prev, reasoning_model: value }));
                }}
                placeholder="例如: kimi-k2-thinking / glm-4-plus-thinking"
                disabled={saving}
              />
            )}
          </FormField>
        ) : null}

        {CX2CC_MAPPED_MODEL_FIELDS.map((field) => (
          <FormField key={field.modelField} label={field.label} hint={field.hint}>
            {(id, hintId) => (
              <ModelAndContextInputs
                isCx2cc={isCx2cc}
                id={id}
                hintId={hintId}
                modelValue={claudeModels[field.modelField] ?? ""}
                onModelChange={(value) =>
                  updateMappedModel(field.modelField, field.contextField, value)
                }
                modelPlaceholder={field.placeholder}
                contextLabel={field.label}
                contextValue={claudeModels[field.contextField]}
                onContextChange={(value) => updateContextWindow(field.contextField, value)}
                disabled={saving}
              />
            )}
          </FormField>
        ))}
      </div>
    </details>
  );
}
