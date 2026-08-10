import { AlertTriangle, ChevronDown, Plus, Trash2 } from "lucide-react";
import { useEffect, useId, useState } from "react";
import type {
  UpstreamHttpRetryRule,
  UpstreamRetryPolicy,
  UpstreamTransportRetryKind,
} from "../../services/settings/settings";
import {
  bodyContainsFromTextarea,
  bodyContainsToTextarea,
  createUpstreamHttpRetryRule,
  MAX_UPSTREAM_RETRY_POLICY_BACKOFF_MS,
  MAX_UPSTREAM_RETRY_POLICY_HTTP_RULES,
  MAX_UPSTREAM_RETRY_POLICY_MAX_RETRIES,
  toggleRetryTransportError,
  UPSTREAM_RETRY_TRANSPORT_ERROR_LABELS,
  UPSTREAM_RETRY_TRANSPORT_ERRORS,
} from "../../services/gateway/upstreamRetryPolicy";
import { Button } from "../../ui/Button";
import { FormField } from "../../ui/FormField";
import { Input } from "../../ui/Input";
import { Switch } from "../../ui/Switch";
import { Textarea } from "../../ui/Textarea";
import { Tooltip } from "../../ui/Tooltip";
import { cn } from "../../utils/cn";

function retryRuleSummary(rule: UpstreamHttpRetryRule): string {
  const description = rule.description.trim();
  const matchSummary =
    rule.body_contains.length === 0
      ? "未设置匹配内容"
      : rule.body_contains.length === 1
        ? rule.body_contains[0]
        : `${rule.body_contains.length} 个匹配内容`;
  return description ? `${description} · ${matchSummary}` : matchSummary;
}

function RetryRuleEditor({
  rule,
  index,
  disabled,
  expanded,
  onToggle,
  onChange,
  onDelete,
}: {
  rule: UpstreamHttpRetryRule;
  index: number;
  disabled: boolean;
  expanded: boolean;
  onToggle: () => void;
  onChange: (rule: UpstreamHttpRetryRule) => void;
  onDelete: () => void;
}) {
  const fieldId = useId();
  const statusId = `${fieldId}-status`;
  const descriptionId = `${fieldId}-description`;
  const bodyId = `${fieldId}-body`;
  const [bodyDraft, setBodyDraft] = useState(() => bodyContainsToTextarea(rule.body_contains));

  useEffect(() => {
    const parsedDraft = bodyContainsFromTextarea(bodyDraft);
    const matchesRule =
      parsedDraft.length === rule.body_contains.length &&
      parsedDraft.every((value, contentIndex) => value === rule.body_contains[contentIndex]);
    if (!matchesRule) setBodyDraft(bodyContainsToTextarea(rule.body_contains));
  }, [bodyDraft, rule.body_contains]);

  return (
    <div role="group" aria-label={`HTTP 规则 ${index + 1}`} className="text-sm">
      <div className="flex min-h-12 items-center gap-2 py-1.5">
        <Switch
          className="shrink-0"
          checked={rule.enabled}
          disabled={disabled}
          aria-label={`启用 HTTP 规则 ${index + 1}`}
          onCheckedChange={(enabled) => onChange({ ...rule, enabled })}
        />
        <button
          type="button"
          className="flex min-w-0 flex-1 items-center gap-2 rounded px-1.5 py-2 text-left transition-colors hover:bg-secondary/60 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
          aria-expanded={expanded}
          aria-label={`${expanded ? "收起" : "编辑"} HTTP 规则 ${index + 1}`}
          onClick={onToggle}
        >
          <span className="shrink-0 font-medium text-foreground">规则 {index + 1}</span>
          <span className="shrink-0 rounded bg-secondary px-1.5 py-0.5 text-[11px] text-muted-foreground">
            HTTP {Number.isFinite(rule.status_code) ? rule.status_code : "未设置"}
          </span>
          {!rule.enabled ? (
            <span className="hidden shrink-0 rounded bg-secondary px-1.5 py-0.5 text-[11px] text-muted-foreground sm:inline">
              已停用
            </span>
          ) : null}
          <span className="min-w-0 flex-1 truncate text-xs text-muted-foreground">
            {retryRuleSummary(rule)}
          </span>
          <ChevronDown
            className={cn(
              "h-4 w-4 shrink-0 text-muted-foreground transition-transform",
              expanded && "rotate-180"
            )}
            aria-hidden="true"
          />
        </button>
        <Tooltip content={`删除 HTTP 规则 ${index + 1}`}>
          <Button
            variant="ghost"
            size="icon"
            aria-label={`删除 HTTP 规则 ${index + 1}`}
            disabled={disabled}
            onClick={onDelete}
          >
            <Trash2 className="h-4 w-4" aria-hidden="true" />
          </Button>
        </Tooltip>
      </div>

      {expanded ? (
        <div className="space-y-3 border-t border-border/70 bg-secondary/20 px-3 py-3 sm:pl-14">
          <div className="grid gap-3 sm:grid-cols-[8rem_minmax(0,1fr)]">
            <div className="space-y-1.5">
              <label htmlFor={statusId} className="text-xs font-medium text-muted-foreground">
                错误码
              </label>
              <Input
                id={statusId}
                aria-label={`规则 ${index + 1} · 错误码`}
                type="number"
                min={400}
                max={599}
                step={1}
                value={Number.isFinite(rule.status_code) ? rule.status_code : ""}
                disabled={disabled}
                onChange={(event) =>
                  onChange({
                    ...rule,
                    status_code:
                      event.currentTarget.value === ""
                        ? Number.NaN
                        : event.currentTarget.valueAsNumber,
                  })
                }
              />
            </div>
            <div className="space-y-1.5">
              <label htmlFor={descriptionId} className="text-xs font-medium text-muted-foreground">
                描述
              </label>
              <Input
                id={descriptionId}
                aria-label={`HTTP 规则 ${index + 1} 描述`}
                value={rule.description}
                disabled={disabled}
                onChange={(event) => onChange({ ...rule, description: event.currentTarget.value })}
              />
            </div>
          </div>

          <div className="space-y-1.5">
            <label htmlFor={bodyId} className="text-xs font-medium text-muted-foreground">
              匹配内容（每行一项）
            </label>
            <Textarea
              id={bodyId}
              aria-label={`HTTP 规则 ${index + 1} 匹配内容`}
              value={bodyDraft}
              disabled={disabled}
              rows={3}
              className="min-h-[5.5rem] resize-y"
              onChange={(event) => {
                const nextDraft = event.currentTarget.value;
                setBodyDraft(nextDraft);
                onChange({
                  ...rule,
                  body_contains: bodyContainsFromTextarea(nextDraft),
                });
              }}
            />
          </div>
        </div>
      ) : null}
    </div>
  );
}

function KeywordTextarea({
  label,
  values,
  disabled,
  onChange,
}: {
  label: string;
  values: string[];
  disabled: boolean;
  onChange: (values: string[]) => void;
}) {
  const id = useId();
  const [draft, setDraft] = useState(() => bodyContainsToTextarea(values));

  useEffect(() => {
    const parsedDraft = bodyContainsFromTextarea(draft);
    const matches =
      parsedDraft.length === values.length &&
      parsedDraft.every((value, index) => value === values[index]);
    if (!matches) setDraft(bodyContainsToTextarea(values));
  }, [draft, values]);

  return (
    <div className="space-y-1.5">
      <label htmlFor={id} className="text-xs font-medium text-muted-foreground">
        {label}
      </label>
      <Textarea
        id={id}
        value={draft}
        disabled={disabled}
        rows={5}
        className="min-h-[8.25rem] resize-y"
        onChange={(event) => {
          const next = event.currentTarget.value;
          setDraft(next);
          onChange(bodyContainsFromTextarea(next));
        }}
      />
    </div>
  );
}

export function RetryPolicyFields({
  policy,
  disabled,
  onChange,
  streamInternalErrorGuardMs,
  maxStreamInternalErrorGuardMs,
  onStreamInternalErrorGuardMsChange,
}: {
  policy: UpstreamRetryPolicy;
  disabled: boolean;
  onChange: (policy: UpstreamRetryPolicy) => void;
  streamInternalErrorGuardMs?: number;
  maxStreamInternalErrorGuardMs?: number;
  onStreamInternalErrorGuardMsChange?: (value: number) => void;
}) {
  const [expandedRuleIndex, setExpandedRuleIndex] = useState<number | null>(null);
  const [streamDetailsOpen, setStreamDetailsOpen] = useState(false);

  function updateRule(index: number, rule: UpstreamHttpRetryRule) {
    const httpRules = [...policy.http_rules];
    httpRules[index] = rule;
    onChange({ ...policy, http_rules: httpRules });
  }

  function deleteRule(index: number) {
    setExpandedRuleIndex((current) => {
      if (current == null) return null;
      if (current === index) return null;
      return current > index ? current - 1 : current;
    });
    onChange({
      ...policy,
      http_rules: policy.http_rules.filter((_, ruleIndex) => ruleIndex !== index),
    });
  }

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between gap-3">
        <div>
          <div className="text-sm font-medium text-foreground">启用瞬时错误重试</div>
          <div className="text-xs text-muted-foreground">
            关闭后匹配错误也会直接进入切换/失败流程。
          </div>
        </div>
        <Switch
          checked={policy.enabled}
          aria-label="启用瞬时错误重试"
          onCheckedChange={(checked) => onChange({ ...policy, enabled: checked })}
          disabled={disabled}
        />
      </div>

      <div className="space-y-2">
        <div className="flex items-center justify-between gap-3">
          <div className="text-xs font-medium text-muted-foreground">HTTP 规则</div>
          <Button
            variant="secondary"
            size="sm"
            disabled={disabled || policy.http_rules.length >= MAX_UPSTREAM_RETRY_POLICY_HTTP_RULES}
            onClick={() => {
              const nextIndex = policy.http_rules.length;
              setExpandedRuleIndex(nextIndex);
              onChange({
                ...policy,
                http_rules: [...policy.http_rules, createUpstreamHttpRetryRule()],
              });
            }}
          >
            <Plus className="h-3.5 w-3.5" aria-hidden="true" />
            新增规则
          </Button>
        </div>
        <div className="divide-y divide-border border-y border-border">
          {policy.http_rules.map((rule, index) => (
            <RetryRuleEditor
              key={index}
              rule={rule}
              index={index}
              disabled={disabled}
              expanded={expandedRuleIndex === index}
              onToggle={() => setExpandedRuleIndex((current) => (current === index ? null : index))}
              onChange={(next) => updateRule(index, next)}
              onDelete={() => deleteRule(index)}
            />
          ))}
          {policy.http_rules.length === 0 ? (
            <div className="py-4 text-xs text-muted-foreground">暂无 HTTP 规则</div>
          ) : null}
        </div>
      </div>

      <div className="space-y-2">
        <div>
          <div className="text-xs font-medium text-muted-foreground">网络与传输失败</div>
          <p className="mt-1 text-xs text-muted-foreground">
            勾选后，这些尚未形成有效上游响应的失败会使用下方共享重试次数。
          </p>
        </div>
        <div className="grid gap-2 md:grid-cols-3">
          {UPSTREAM_RETRY_TRANSPORT_ERRORS.map((kind) => (
            <label
              key={kind}
              className="flex min-w-0 items-start gap-2 border border-border px-3 py-2.5 text-xs text-secondary-foreground"
            >
              <input
                type="checkbox"
                className="mt-0.5"
                checked={policy.transport_errors.includes(kind)}
                disabled={disabled}
                onChange={() =>
                  onChange(toggleRetryTransportError(policy, kind as UpstreamTransportRetryKind))
                }
              />
              <span className="min-w-0">
                <span className="block font-medium text-foreground">
                  {UPSTREAM_RETRY_TRANSPORT_ERROR_LABELS[kind]}
                </span>
                <span className="mt-0.5 block leading-relaxed text-muted-foreground">
                  {kind === "connect"
                    ? "无法与供应商建立连接。"
                    : kind === "timeout"
                      ? "连接或等待上游响应超过超时限制。"
                      : "已连接，但读取响应流时失败。"}
                </span>
              </span>
            </label>
          ))}
        </div>
      </div>

      <div className="border-y border-border">
        <div className="flex min-h-14 items-center gap-2 py-2">
          <button
            type="button"
            className="flex min-w-0 flex-1 items-center gap-2 rounded px-1.5 py-2 text-left transition-colors hover:bg-secondary/60 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
            aria-expanded={streamDetailsOpen}
            aria-label={`${streamDetailsOpen ? "收起" : "查看"} Codex 流终态防火墙设置`}
            onClick={() => setStreamDetailsOpen((current) => !current)}
          >
            <span className="min-w-0 flex-1">
              <span className="block text-sm font-medium text-foreground">Codex 流终态防火墙</span>
              <span className="mt-0.5 block text-xs leading-relaxed text-muted-foreground">
                统一处理最终出线的 Codex Responses SSE 终态错误。
              </span>
            </span>
            <ChevronDown
              className={cn(
                "h-4 w-4 shrink-0 text-muted-foreground transition-transform",
                streamDetailsOpen && "rotate-180"
              )}
              aria-hidden="true"
            />
          </button>
          <Switch
            checked={policy.stream_internal_errors.enabled}
            aria-label="启用 Codex 流终态防火墙"
            disabled={disabled}
            onCheckedChange={(enabled) =>
              onChange({
                ...policy,
                stream_internal_errors: { ...policy.stream_internal_errors, enabled },
              })
            }
          />
        </div>
        {streamDetailsOpen ? (
          <div className="space-y-4 border-t border-border/70 bg-secondary/20 px-3 py-4">
            <div className="text-xs leading-relaxed text-muted-foreground">
              开启后，瞬时或容量错误会在提交前复用下方共享重试预算；提交后默认丢弃终态帧并结束。
              硬性错误和未知错误在提交前返回 502 / GW_FAKE_200。关闭后不拦截、不重试、不切换，
              所有终态帧保持原样。
            </div>
            <KeywordTextarea
              label="终态帧透传例外（每行一项）"
              values={policy.stream_internal_errors.passthrough_keywords}
              disabled={disabled || !policy.stream_internal_errors.enabled}
              onChange={(passthroughKeywords) =>
                onChange({
                  ...policy,
                  stream_internal_errors: {
                    ...policy.stream_internal_errors,
                    passthrough_keywords: passthroughKeywords,
                  },
                })
              }
            />
            <p className="text-xs leading-relaxed text-muted-foreground">
              例外仅作用于提交后的完整终态帧，可用于明确的安全策略拒绝；容量、认证、配额与无效请求错误始终阻断。
            </p>
            {policy.stream_internal_errors.legacy_retry_keywords.length > 0 ? (
              <div className="flex gap-2 border-l-2 border-amber-500 px-3 py-2 text-xs leading-relaxed text-muted-foreground">
                <AlertTriangle
                  className="mt-0.5 h-4 w-4 shrink-0 text-amber-600"
                  aria-hidden="true"
                />
                <span>
                  已迁移 {policy.stream_internal_errors.legacy_retry_keywords.length} 条旧版重试词；
                  它们仅在提交前将未知终态提升为可重试，当前版本不可新增或编辑。
                </span>
              </div>
            ) : null}
            {streamInternalErrorGuardMs != null &&
            maxStreamInternalErrorGuardMs != null &&
            onStreamInternalErrorGuardMsChange ? (
              <div className="flex flex-col gap-2 border-t border-border pt-3 sm:flex-row sm:items-center sm:justify-between">
                <div>
                  <label
                    htmlFor="stream-internal-error-guard-ms"
                    className="text-sm font-medium text-foreground"
                  >
                    首个有效输出观察窗口
                  </label>
                  <p className="mt-1 text-xs text-muted-foreground">
                    在提交给 Codex 前继续观察终态错误；0 表示不额外等待。
                  </p>
                </div>
                <div className="flex items-center gap-2">
                  <Input
                    id="stream-internal-error-guard-ms"
                    aria-label="流内部错误观察窗口"
                    type="number"
                    min={0}
                    max={maxStreamInternalErrorGuardMs}
                    value={streamInternalErrorGuardMs}
                    disabled={disabled}
                    className="w-28"
                    onChange={(event) => {
                      const next = event.currentTarget.valueAsNumber;
                      if (Number.isFinite(next)) onStreamInternalErrorGuardMsChange(next);
                    }}
                  />
                  <span className="text-sm text-muted-foreground">毫秒</span>
                </div>
              </div>
            ) : null}
          </div>
        ) : null}
      </div>

      <div className="grid gap-4 md:grid-cols-3">
        <FormField label="每个供应商最多重试次数" hint="HTTP / 网络 / 流内部共享">
          {(id, hintId) => (
            <Input
              id={id}
              aria-describedby={hintId}
              type="number"
              min={0}
              max={MAX_UPSTREAM_RETRY_POLICY_MAX_RETRIES}
              value={policy.max_retries}
              disabled={disabled}
              onChange={(event) => {
                const next = event.currentTarget.valueAsNumber;
                if (Number.isFinite(next)) onChange({ ...policy, max_retries: next });
              }}
            />
          )}
        </FormField>
        <FormField label="固定重试间隔（毫秒）" hint="每次同供应商重试前等待">
          {(id, hintId) => (
            <Input
              id={id}
              aria-describedby={hintId}
              type="number"
              min={0}
              max={MAX_UPSTREAM_RETRY_POLICY_BACKOFF_MS}
              value={policy.backoff_ms}
              disabled={disabled}
              onChange={(event) => {
                const next = event.currentTarget.valueAsNumber;
                if (Number.isFinite(next)) onChange({ ...policy, backoff_ms: next });
              }}
            />
          )}
        </FormField>
        <div className="flex items-center justify-between gap-3 border border-border px-3 py-2">
          <div>
            <div className="text-xs font-medium text-foreground">每次重试失败计入熔断</div>
            <div className="text-[11px] leading-relaxed text-muted-foreground">
              关闭时，仅同一供应商的最终失败计数。
            </div>
          </div>
          <Switch
            checked={policy.counts_toward_circuit_breaker}
            aria-label="配置型重试计入熔断"
            disabled={disabled}
            onCheckedChange={(checked) =>
              onChange({ ...policy, counts_toward_circuit_breaker: checked })
            }
          />
        </div>
      </div>
    </div>
  );
}
