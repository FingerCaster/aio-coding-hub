import { useEffect, useId, useMemo, useRef, useState } from "react";
import { AlertTriangle, Plus, RefreshCw, Settings2, Sparkles, Trash2 } from "lucide-react";
import type { CodexModelContextCandidatesState } from "../../../services/cli/cliManager";
import type { AppSettings } from "../../../services/settings/settings";
import {
  MAX_CODEX_MODEL_CONTEXT_RULES,
  addGpt56Preset,
  codexModelContextRulesEqual,
  codexModelContextRulesKey,
  describeCodexModelContextBase,
  formatCodexContextTokens,
  normalizeCodexModelContextRules,
  type CodexModelContextRule,
  type CodexModelContextRuleValidationError,
} from "../../../services/settings/codexModelContextRules";
import { Button } from "../../../ui/Button";
import { Input } from "../../../ui/Input";
import { Switch } from "../../../ui/Switch";
import {
  Dialog as DialogRoot,
  DialogContent,
  DialogDescription,
  DialogTitle,
} from "../../../ui/shadcn/dialog";
import { cn } from "../../../utils/cn";

export type CodexModelContextRulesSaveResult =
  | { status: "confirmed"; settings: AppSettings }
  | { status: "reverted"; settings: AppSettings }
  | { status: "blocked"; settings: AppSettings | null };

type RuleDraftRow = {
  rowId: string;
  model_id: string;
  context_window: string;
  enabled: boolean;
};

let ruleDraftRowSequence = 0;

function nextRuleDraftRowId() {
  ruleDraftRowSequence += 1;
  return `codex-model-context-rule-${ruleDraftRowSequence}`;
}

function rulesToDraft(rules: readonly CodexModelContextRule[]): RuleDraftRow[] {
  return rules.map((rule) => ({
    rowId: nextRuleDraftRowId(),
    model_id: rule.model_id,
    context_window: String(rule.context_window),
    enabled: rule.enabled,
  }));
}

function draftValues(rows: readonly RuleDraftRow[]) {
  return rows.map(({ model_id, context_window, enabled }) => ({
    model_id,
    context_window,
    enabled,
  }));
}

function draftTarget(value: string): number | null {
  const trimmed = value.trim();
  if (!/^[0-9]+$/u.test(trimmed)) return null;
  const target = Number(trimmed);
  return Number.isSafeInteger(target) ? target : null;
}

function candidateIssueText(
  candidates: CodexModelContextCandidatesState | null,
  candidatesError: boolean
) {
  if (candidatesError) return "基础目录读取失败，当前仅可手工输入精确模型 ID。";
  if (!candidates || candidates.status === "ready") return null;
  return "基础目录当前不可用，仍可手工编辑规则并由保存事务执行最终校验。";
}

export function CodexModelContextRulesSection({
  rules,
  candidates,
  candidatesLoading,
  candidatesError,
  settingsReadOnly,
  settingsReadErrorMessage,
  controlsDisabled,
  saving,
  autoOpenRequestKey = null,
  retrySettings,
  retryCandidates,
  persistRules,
}: {
  rules: readonly CodexModelContextRule[] | null;
  candidates: CodexModelContextCandidatesState | null;
  candidatesLoading: boolean;
  candidatesError: boolean;
  settingsReadOnly: boolean;
  settingsReadErrorMessage: string | null;
  controlsDisabled: boolean;
  saving: boolean;
  autoOpenRequestKey?: string | null;
  retrySettings: () => Promise<unknown> | unknown;
  retryCandidates: () => Promise<unknown> | unknown;
  persistRules?: (rules: CodexModelContextRule[]) => Promise<CodexModelContextRulesSaveResult>;
}) {
  const fieldPrefix = useId();
  const feedbackRef = useRef<HTMLDivElement>(null);
  const autoOpenedRef = useRef<string | null>(null);
  const canonicalRules = useMemo(() => {
    if (!rules) return null;
    try {
      return normalizeCodexModelContextRules(rules);
    } catch {
      return null;
    }
  }, [rules]);
  const canonicalKey = useMemo(
    () => (canonicalRules ? codexModelContextRulesKey(canonicalRules) : "unavailable"),
    [canonicalRules]
  );

  const [open, setOpen] = useState(autoOpenRequestKey != null);
  const [confirmedRules, setConfirmedRules] = useState<CodexModelContextRule[]>(
    canonicalRules ?? []
  );
  const [draft, setDraft] = useState<RuleDraftRow[]>(() => rulesToDraft(canonicalRules ?? []));
  const [feedback, setFeedback] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const observedCanonicalKeyRef = useRef(canonicalKey);

  useEffect(() => {
    if (autoOpenRequestKey == null || autoOpenedRef.current === autoOpenRequestKey) return;
    autoOpenedRef.current = autoOpenRequestKey;
    setOpen(true);
  }, [autoOpenRequestKey]);

  useEffect(() => {
    if (observedCanonicalKeyRef.current === canonicalKey) return;
    observedCanonicalKeyRef.current = canonicalKey;
    if (!canonicalRules) return;
    setConfirmedRules(canonicalRules);
    setDraft(rulesToDraft(canonicalRules));
    if (open) setFeedback("规则已更新，当前草稿已恢复为最新确认状态。");
  }, [canonicalKey, canonicalRules, open]);

  const candidateReady = candidates?.status === "ready";
  const candidateMap = useMemo(() => {
    const result = new Map<string, CodexModelContextCandidatesState["models"][number]>();
    if (!candidateReady || !candidates) return result;
    for (const candidate of candidates.models) {
      if (!result.has(candidate.model_id)) result.set(candidate.model_id, candidate);
    }
    return result;
  }, [candidateReady, candidates]);
  const visibleCandidates = useMemo(() => {
    if (!candidateReady || !candidates) return [];
    const seen = new Set<string>();
    return candidates.models.filter((candidate) => {
      const modelId = candidate.model_id.trim();
      if (candidate.hidden || !modelId || seen.has(modelId)) return false;
      seen.add(modelId);
      return true;
    });
  }, [candidateReady, candidates]);

  const enabledCount = canonicalRules?.filter((rule) => rule.enabled).length ?? 0;
  const totalCount = canonicalRules?.length ?? 0;
  const busy = saving || submitting;
  const readOnly = settingsReadOnly || canonicalRules == null;
  const editDisabled = readOnly || controlsDisabled || busy || !persistRules;
  const draftDirty = canonicalRules
    ? !codexModelContextRulesEqual(draftValues(draft), confirmedRules)
    : false;
  const candidateMessage = candidateIssueText(candidates, candidatesError);

  function resetDraft(nextRules = confirmedRules) {
    if (!nextRules) return;
    setDraft(rulesToDraft(nextRules));
  }

  function acceptConfirmedRules(nextRules: CodexModelContextRule[]) {
    setConfirmedRules(nextRules);
    setDraft(rulesToDraft(nextRules));
  }

  function openEditor() {
    resetDraft();
    setFeedback(null);
    setOpen(true);
  }

  function closeEditor() {
    if (busy) return;
    resetDraft();
    setFeedback(null);
    setOpen(false);
  }

  function updateRule(rowId: string, patch: Partial<Omit<RuleDraftRow, "rowId">>) {
    if (editDisabled) return;
    setFeedback(null);
    setDraft((current) => current.map((row) => (row.rowId === rowId ? { ...row, ...patch } : row)));
  }

  function removeRule(rowId: string) {
    if (editDisabled) return;
    setFeedback(null);
    setDraft((current) => current.filter((row) => row.rowId !== rowId));
  }

  function addRule() {
    if (editDisabled) return;
    if (draft.length >= MAX_CODEX_MODEL_CONTEXT_RULES) {
      setFeedback(`规则最多允许 ${MAX_CODEX_MODEL_CONTEXT_RULES} 条。`);
      return;
    }
    setFeedback(null);
    setDraft((current) => [
      ...current,
      {
        rowId: nextRuleDraftRowId(),
        model_id: "",
        context_window: "",
        enabled: true,
      },
    ]);
  }

  function addPreset() {
    if (editDisabled) return;
    try {
      const result = addGpt56Preset(draftValues(draft));
      if (result.added === 0) {
        setFeedback("GPT-5.6 372K 预设中的三条规则均已存在，未修改草稿。");
        return;
      }
      setDraft((current) => [
        ...current,
        ...result.addedRules.map((rule) => ({
          rowId: nextRuleDraftRowId(),
          model_id: rule.model_id,
          context_window: String(rule.context_window),
          enabled: rule.enabled,
        })),
      ]);
      setFeedback(
        result.skipped.length === 0
          ? "已向草稿添加 GPT-5.6 372K 预设；应用更改后生效。"
          : `已添加 ${result.added} 条，跳过已存在的 ${result.skipped.join("、")}。`
      );
    } catch (error) {
      setFeedback(error instanceof Error ? error.message : "GPT-5.6 372K 预设添加失败。");
    }
  }

  async function applyChanges() {
    if (editDisabled || !persistRules || !canonicalRules || !draftDirty) return;

    let normalized: CodexModelContextRule[];
    try {
      normalized = normalizeCodexModelContextRules(draftValues(draft));
    } catch (error) {
      const validation = error as CodexModelContextRuleValidationError;
      resetDraft();
      setFeedback(validation instanceof Error ? validation.message : "规则校验失败。");
      window.requestAnimationFrame(() => feedbackRef.current?.focus());
      return;
    }

    setSubmitting(true);
    setFeedback(null);
    try {
      const result = await persistRules(normalized);
      const confirmedRules = result.settings
        ? normalizeCodexModelContextRules(result.settings.codex_model_context_rules)
        : canonicalRules;
      acceptConfirmedRules(confirmedRules);

      if (result.status === "confirmed") {
        setOpen(false);
        return;
      }
      if (result.status === "reverted") {
        setFeedback("保存未获后端确认，已恢复为最新 canonical 规则。");
        return;
      }
      setFeedback("保存失败且设置回读未完成，编辑器已进入只读保护。");
    } catch (error) {
      resetDraft();
      setFeedback(error instanceof Error ? error.message : "保存失败，已恢复确认状态。");
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <section
      id="codex-model-context-rules"
      className="rounded-xl border border-border/80 bg-white/80 p-4 dark:border-border dark:bg-card/20"
    >
      <div className="flex flex-col gap-3 md:flex-row md:items-start md:justify-between">
        <div className="min-w-0">
          <div className="flex flex-wrap items-center gap-2">
            <h3 className="text-sm font-semibold text-foreground">模型上下文规则</h3>
            <span className="rounded-md border border-border bg-secondary px-2 py-0.5 text-[11px] font-medium text-secondary-foreground">
              启用 {enabledCount} / 共 {totalCount} 条
            </span>
          </div>
          <p className="mt-1 text-xs leading-relaxed text-muted-foreground">
            精确覆盖 Codex 模型目录中的名义上下文窗口，仅对新启动的 Codex 会话生效。
          </p>
          <p className="mt-2 text-[11px] leading-relaxed text-amber-700 dark:text-amber-400">
            目录声明不会增加模型或 Provider 的真实能力。
          </p>
        </div>
        <div className="flex shrink-0 items-center gap-2">
          {busy ? (
            <RefreshCw
              aria-label="正在保存 Codex 模型上下文规则"
              className="h-3.5 w-3.5 animate-spin text-muted-foreground"
            />
          ) : null}
          <Button size="sm" variant="secondary" onClick={openEditor} disabled={!canonicalRules}>
            <Settings2 className="h-4 w-4" aria-hidden="true" />
            管理规则
          </Button>
        </div>
      </div>

      {settingsReadErrorMessage ? (
        <div className="mt-3 flex flex-col gap-2 border-t border-rose-200 pt-3 text-xs text-rose-700 dark:border-rose-800 dark:text-rose-300 sm:flex-row sm:items-center sm:justify-between">
          <span>{settingsReadErrorMessage}</span>
          <Button
            size="sm"
            variant="secondary"
            onClick={() => void retrySettings()}
            disabled={busy}
          >
            <RefreshCw className="h-3.5 w-3.5" aria-hidden="true" />
            重试
          </Button>
        </div>
      ) : null}

      <DialogRoot
        open={open}
        onOpenChange={(next) => {
          if (busy) return;
          if (next) openEditor();
          else closeEditor();
        }}
      >
        <DialogContent
          className="max-w-5xl"
          onEscapeKeyDown={(event) => {
            if (busy) event.preventDefault();
          }}
          onPointerDownOutside={(event) => {
            if (busy) event.preventDefault();
          }}
        >
          <div className="flex items-start justify-between gap-3 border-b border-border px-4 py-3 sm:px-5 sm:py-4">
            <div className="min-w-0">
              <DialogTitle>Codex 模型上下文规则</DialogTitle>
              <DialogDescription className="mt-1">
                每条规则精确匹配一个模型 ID，并同时设置 context_window 与 max_context_window。
              </DialogDescription>
            </div>
            <Button size="sm" variant="secondary" onClick={closeEditor} disabled={busy}>
              关闭
            </Button>
          </div>

          <div className="min-h-0 flex-1 overflow-y-auto px-4 py-4 sm:px-5">
            <div className="space-y-4">
              {settingsReadErrorMessage ? (
                <div className="flex flex-col gap-2 border border-rose-200 bg-rose-50 px-3 py-2 text-xs text-rose-800 dark:border-rose-800 dark:bg-rose-950/30 dark:text-rose-200 sm:flex-row sm:items-center sm:justify-between">
                  <span>{settingsReadErrorMessage}</span>
                  <Button
                    size="sm"
                    variant="secondary"
                    onClick={() => void retrySettings()}
                    disabled={busy}
                  >
                    重试设置读取
                  </Button>
                </div>
              ) : null}

              {candidateMessage || candidatesLoading ? (
                <div className="flex flex-col gap-2 border-b border-border pb-3 text-xs text-muted-foreground sm:flex-row sm:items-center sm:justify-between">
                  <span>{candidatesLoading ? "正在读取基础模型目录…" : candidateMessage}</span>
                  {!candidatesLoading && candidateMessage ? (
                    <Button
                      size="sm"
                      variant="secondary"
                      onClick={() => void retryCandidates()}
                      disabled={busy}
                    >
                      <RefreshCw className="h-3.5 w-3.5" aria-hidden="true" />
                      重试候选读取
                    </Button>
                  ) : null}
                </div>
              ) : null}

              <div className="flex flex-wrap items-center justify-between gap-2">
                <div className="text-xs text-muted-foreground">
                  草稿 {draft.length} / {MAX_CODEX_MODEL_CONTEXT_RULES} 条
                </div>
                <div className="flex flex-wrap items-center gap-2">
                  <Button size="sm" variant="secondary" onClick={addRule} disabled={editDisabled}>
                    <Plus className="h-4 w-4" aria-hidden="true" />
                    添加规则
                  </Button>
                  <Button size="sm" variant="secondary" onClick={addPreset} disabled={editDisabled}>
                    <Sparkles className="h-4 w-4" aria-hidden="true" />
                    添加 GPT-5.6 372K 预设
                  </Button>
                </div>
              </div>

              <datalist id={`${fieldPrefix}-models`}>
                {visibleCandidates.map((candidate) => (
                  <option key={candidate.model_id} value={candidate.model_id}>
                    {candidate.display_name}
                  </option>
                ))}
              </datalist>

              {draft.length === 0 ? (
                <div className="border-y border-dashed border-border py-10 text-center text-sm text-muted-foreground">
                  暂无规则
                </div>
              ) : (
                <div className="divide-y divide-border border-y border-border">
                  {draft.map((row, index) => {
                    const modelInputId = `${fieldPrefix}-${row.rowId}-model`;
                    const contextInputId = `${fieldPrefix}-${row.rowId}-context`;
                    const target = draftTarget(row.context_window);
                    const candidate = candidateMap.get(row.model_id.trim()) ?? null;
                    const comparison =
                      target == null
                        ? null
                        : describeCodexModelContextBase(candidate, target, row.enabled);
                    const rowLabel = row.model_id.trim() || `第 ${index + 1} 条规则`;

                    return (
                      <div
                        key={row.rowId}
                        className="grid min-h-28 grid-cols-1 gap-3 py-4 md:grid-cols-[auto_minmax(0,2fr)_minmax(180px,1fr)_auto] md:items-start"
                      >
                        <div className="flex h-9 items-center gap-2 md:pt-5">
                          <Switch
                            checked={row.enabled}
                            onCheckedChange={(enabled) => updateRule(row.rowId, { enabled })}
                            disabled={editDisabled}
                            aria-label={`${rowLabel}启用状态`}
                          />
                          <span className="text-xs text-muted-foreground md:hidden">
                            {row.enabled ? "已启用" : "已停用"}
                          </span>
                        </div>

                        <div className="min-w-0">
                          <label
                            htmlFor={modelInputId}
                            className="mb-1 block text-xs font-medium text-secondary-foreground"
                          >
                            精确模型 ID
                          </label>
                          <Input
                            id={modelInputId}
                            list={`${fieldPrefix}-models`}
                            value={row.model_id}
                            onChange={(event) =>
                              updateRule(row.rowId, { model_id: event.target.value })
                            }
                            disabled={editDisabled}
                            autoComplete="off"
                            spellCheck={false}
                            placeholder="例如 gpt-5.6-sol"
                            className="font-mono"
                          />
                        </div>

                        <div className="min-w-0">
                          <label
                            htmlFor={contextInputId}
                            className="mb-1 block text-xs font-medium text-secondary-foreground"
                          >
                            上下文 tokens
                          </label>
                          <Input
                            id={contextInputId}
                            type="text"
                            inputMode="numeric"
                            value={row.context_window}
                            onChange={(event) =>
                              updateRule(row.rowId, { context_window: event.target.value })
                            }
                            disabled={editDisabled}
                            autoComplete="off"
                            placeholder="372000"
                          />
                        </div>

                        <div className="flex h-9 items-center justify-end md:pt-5">
                          <Button
                            size="icon"
                            variant="ghost"
                            onClick={() => removeRule(row.rowId)}
                            disabled={editDisabled}
                            aria-label={`删除${rowLabel}`}
                            title={`删除${rowLabel}`}
                          >
                            <Trash2 className="h-4 w-4" aria-hidden="true" />
                          </Button>
                        </div>

                        <div className="min-w-0 text-[11px] leading-relaxed text-muted-foreground md:col-span-3 md:col-start-2">
                          {target == null ? (
                            <span>目标值待校验</span>
                          ) : candidateReady && !candidate ? (
                            <span>
                              当前基础目录中没有该模型 · 目标 {formatCodexContextTokens(target)}
                            </span>
                          ) : (
                            <span>{comparison?.text}</span>
                          )}
                          {comparison?.increasesBase ? (
                            <span
                              className={cn(
                                "mt-1 flex items-start gap-1 text-amber-700 dark:text-amber-400"
                              )}
                            >
                              <AlertTriangle
                                className="mt-0.5 h-3.5 w-3.5 shrink-0"
                                aria-hidden="true"
                              />
                              目标高于基础目录声明；这不会增加模型或 Provider 的真实能力。
                            </span>
                          ) : null}
                        </div>
                      </div>
                    );
                  })}
                </div>
              )}

              {feedback ? (
                <div
                  ref={feedbackRef}
                  tabIndex={-1}
                  role="status"
                  className="border-l-2 border-amber-400 pl-3 text-xs leading-relaxed text-amber-800 outline-none dark:text-amber-300"
                >
                  {feedback}
                </div>
              ) : null}
            </div>
          </div>

          <div className="flex items-center justify-end gap-3 border-t border-border px-4 py-3 sm:px-5">
            <Button variant="secondary" onClick={closeEditor} disabled={busy}>
              取消
            </Button>
            <Button
              variant="primary"
              onClick={() => void applyChanges()}
              disabled={editDisabled || !draftDirty}
            >
              {busy ? <RefreshCw className="h-4 w-4 animate-spin" aria-hidden="true" /> : null}
              {busy ? "应用中…" : "应用更改"}
            </Button>
          </div>
        </DialogContent>
      </DialogRoot>
    </section>
  );
}
