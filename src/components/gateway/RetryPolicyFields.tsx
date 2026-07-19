import { Plus, Trash2 } from "lucide-react";
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

function RetryRuleEditor({
  rule,
  index,
  disabled,
  onChange,
  onDelete,
}: {
  rule: UpstreamHttpRetryRule;
  index: number;
  disabled: boolean;
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
    <div
      role="group"
      aria-label={`HTTP 规则 ${index + 1}`}
      className="grid gap-3 py-4 first:pt-3 md:grid-cols-[minmax(7rem,0.65fr)_minmax(12rem,1fr)_2rem]"
    >
      <div className="space-y-3">
        <div className="flex h-8 items-center justify-between gap-3">
          <label htmlFor={statusId} className="text-xs font-medium text-muted-foreground">
            规则 {index + 1} · 错误码
          </label>
          <Switch
            checked={rule.enabled}
            disabled={disabled}
            aria-label={`启用 HTTP 规则 ${index + 1}`}
            onCheckedChange={(enabled) => onChange({ ...rule, enabled })}
          />
        </div>
        <Input
          id={statusId}
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
                event.currentTarget.value === "" ? Number.NaN : event.currentTarget.valueAsNumber,
            })
          }
        />
        <div className="space-y-1.5">
          <label htmlFor={descriptionId} className="text-xs font-medium text-muted-foreground">
            描述
          </label>
          <Input
            id={descriptionId}
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
          value={bodyDraft}
          disabled={disabled}
          rows={5}
          className="min-h-[8.25rem] resize-y"
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

      <div className="flex h-8 items-center justify-end">
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
    </div>
  );
}

export function RetryPolicyFields({
  policy,
  disabled,
  onChange,
}: {
  policy: UpstreamRetryPolicy;
  disabled: boolean;
  onChange: (policy: UpstreamRetryPolicy) => void;
}) {
  function updateRule(index: number, rule: UpstreamHttpRetryRule) {
    const httpRules = [...policy.http_rules];
    httpRules[index] = rule;
    onChange({ ...policy, http_rules: httpRules });
  }

  function deleteRule(index: number) {
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
            onClick={() =>
              onChange({
                ...policy,
                http_rules: [...policy.http_rules, createUpstreamHttpRetryRule()],
              })
            }
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
        <div className="text-xs font-medium text-muted-foreground">传输错误</div>
        <div className="flex flex-wrap gap-2">
          {UPSTREAM_RETRY_TRANSPORT_ERRORS.map((kind) => (
            <label
              key={kind}
              className="inline-flex items-center gap-2 rounded-md border border-border px-2.5 py-1.5 text-xs text-secondary-foreground"
            >
              <input
                type="checkbox"
                checked={policy.transport_errors.includes(kind)}
                disabled={disabled}
                onChange={() =>
                  onChange(toggleRetryTransportError(policy, kind as UpstreamTransportRetryKind))
                }
              />
              {UPSTREAM_RETRY_TRANSPORT_ERROR_LABELS[kind]}
            </label>
          ))}
        </div>
      </div>

      <div className="grid gap-4 md:grid-cols-3">
        <FormField label="同供应商重试次数">
          {(id) => (
            <Input
              id={id}
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
        <FormField label="重试间隔（毫秒）">
          {(id) => (
            <Input
              id={id}
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
            <div className="text-xs font-medium text-foreground">计入熔断</div>
            <div className="text-[11px] text-muted-foreground">关闭时仅最终失败计数。</div>
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
