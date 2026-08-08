import { Plus, Trash2 } from "lucide-react";
import type { ModelRoutingPolicy, ModelRoutingRule } from "../../services/settings/settings";
import {
  emptyModelRoutingRule,
  MAX_MODEL_ROUTING_RULES,
} from "../../services/gateway/modelRoutingPolicy";
import { Button } from "../../ui/Button";
import { Input } from "../../ui/Input";
import { Switch } from "../../ui/Switch";
import { Tooltip } from "../../ui/Tooltip";

function optionalInput(value: string): string | null {
  return value.length > 0 ? value : null;
}

function ModelRoutingRuleEditor({
  rule,
  index,
  disabled,
  onChange,
  onDelete,
}: {
  rule: ModelRoutingRule;
  index: number;
  disabled: boolean;
  onChange: (rule: ModelRoutingRule) => void;
  onDelete: () => void;
}) {
  return (
    <div
      role="group"
      aria-label={`模型路由规则 ${index + 1}`}
      className="grid gap-3 py-3 first:pt-2 md:grid-cols-[minmax(10rem,1fr)_minmax(10rem,1fr)_minmax(8rem,0.7fr)_2rem]"
    >
      <label className="space-y-1.5 text-xs font-medium text-muted-foreground">
        来源模型
        <Input
          value={rule.source_model}
          disabled={disabled}
          placeholder="客户端模型名"
          onChange={(event) => onChange({ ...rule, source_model: event.currentTarget.value })}
        />
      </label>
      <label className="space-y-1.5 text-xs font-medium text-muted-foreground">
        目标模型
        <Input
          value={rule.target_model ?? ""}
          disabled={disabled}
          placeholder="留空不改模型"
          onChange={(event) =>
            onChange({ ...rule, target_model: optionalInput(event.currentTarget.value) })
          }
        />
      </label>
      <label className="space-y-1.5 text-xs font-medium text-muted-foreground">
        思考强度
        <Input
          value={rule.reasoning_effort ?? ""}
          disabled={disabled}
          placeholder="留空不改"
          onChange={(event) =>
            onChange({ ...rule, reasoning_effort: optionalInput(event.currentTarget.value) })
          }
        />
      </label>
      <div className="flex h-8 items-center justify-end md:mt-5">
        <Tooltip content={`删除模型路由规则 ${index + 1}`}>
          <Button
            variant="ghost"
            size="icon"
            aria-label={`删除模型路由规则 ${index + 1}`}
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

export function ModelRoutingPolicyFields({
  policy,
  disabled,
  showEnabled = true,
  onChange,
}: {
  policy: ModelRoutingPolicy;
  disabled: boolean;
  showEnabled?: boolean;
  onChange: (policy: ModelRoutingPolicy) => void;
}) {
  function updateRule(index: number, rule: ModelRoutingRule) {
    const rules = [...policy.rules];
    rules[index] = rule;
    onChange({ ...policy, rules });
  }

  return (
    <div className="space-y-4">
      {showEnabled ? (
        <div className="flex items-center justify-between gap-3">
          <div className="text-sm font-medium text-foreground">启用模型路由</div>
          <Switch
            checked={policy.enabled}
            aria-label="启用模型路由"
            disabled={disabled}
            onCheckedChange={(enabled) => onChange({ ...policy, enabled })}
          />
        </div>
      ) : null}

      <div className="space-y-2">
        <div className="flex items-center justify-between gap-3">
          <div className="text-xs font-medium text-muted-foreground">精确匹配规则</div>
          <Button
            variant="secondary"
            size="sm"
            disabled={disabled || policy.rules.length >= MAX_MODEL_ROUTING_RULES}
            onClick={() =>
              onChange({ ...policy, rules: [...policy.rules, emptyModelRoutingRule()] })
            }
          >
            <Plus className="h-3.5 w-3.5" aria-hidden="true" />
            新增规则
          </Button>
        </div>
        <div className="divide-y divide-border border-y border-border">
          {policy.rules.map((rule, index) => (
            <ModelRoutingRuleEditor
              key={index}
              rule={rule}
              index={index}
              disabled={disabled}
              onChange={(next) => updateRule(index, next)}
              onDelete={() =>
                onChange({
                  ...policy,
                  rules: policy.rules.filter((_, ruleIndex) => ruleIndex !== index),
                })
              }
            />
          ))}
          {policy.rules.length === 0 ? (
            <div className="py-4 text-xs text-muted-foreground">暂无模型路由规则</div>
          ) : null}
        </div>
      </div>
    </div>
  );
}
