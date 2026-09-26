import { useState } from "react";
import type { OmpSettingsSnapshot } from "../../../services/ompSettings";
import { Button } from "../../../ui/Button";
import { Input } from "../../../ui/Input";
import { Select } from "../../../ui/Select";
import { Switch } from "../../../ui/Switch";
import { OmpModelPicker } from "./OmpModelPicker";
import { ompAgentModelPreview, ompSelectorModelPreview } from "./ompModelPreview";
import { record, selectorText, type SettingsValues } from "./ompSettingsForm";

export function OmpAgentSettings({
  snapshot,
  values,
  disabled,
  roles,
  onRecordChange,
  onChange,
  onEdit,
}: {
  snapshot: OmpSettingsSnapshot;
  values: SettingsValues;
  disabled: boolean;
  roles: string[];
  onRecordChange: (key: string, member: string, value: unknown) => void;
  onChange: (key: string, value: unknown) => void;
  onEdit: (fileName: string | null) => void;
}) {
  const [extraName, setExtraName] = useState("");
  const [extraAgents, setExtraAgents] = useState<string[]>([]);
  const agents = [
    ...snapshot.agents,
    ...extraAgents
      .filter((name) => !snapshot.agents.some((a) => a.name === name))
      .map((name) => ({
        name,
        description: "为项目 / 插件 Agent 设置全局覆盖",
        source: "configured",
        model: [],
        thinking: null,
        fileName: null,
      })),
  ];
  const disabledAgents = Array.isArray(values["task.disabledAgents"])
    ? values["task.disabledAgents"].filter((v): v is string => typeof v === "string")
    : [];
  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <p className="text-xs text-muted-foreground">
          内置目录基于 OMP 18.3.2；项目与插件定义不在本页加载。模型覆盖优先于 Agent 文件中的模型。
        </p>
        <Button size="sm" variant="secondary" disabled={disabled} onClick={() => onEdit(null)}>
          创建自定义 Agent
        </Button>
      </div>
      {agents.map((agent) => (
        <details key={agent.name} className="rounded-xl border border-border" open={undefined}>
          <summary className="flex cursor-pointer list-none items-center gap-3 px-4 py-3 [&::-webkit-details-marker]:hidden">
            <div className="min-w-0 flex-1">
              <div className="flex flex-wrap items-center gap-2">
                <span className="text-sm font-semibold">{agent.name}</span>
                <span className="rounded bg-secondary px-1.5 py-0.5 text-[10px] text-muted-foreground">
                  {agent.source === "bundled"
                    ? "内置"
                    : agent.source === "custom"
                      ? "自定义"
                      : "配置覆盖"}
                </span>
                {disabledAgents.includes(agent.name) && (
                  <span className="text-xs text-muted-foreground">已禁用</span>
                )}
              </div>
              <p className="mt-1 truncate text-xs text-muted-foreground" title={agent.description}>
                {agent.description}
              </p>
            </div>
            <span className="text-xs text-muted-foreground">展开设置</span>
          </summary>
          <div className="space-y-4 border-t border-border p-4">
            <div className="flex flex-wrap items-center justify-between gap-3">
              <label className="flex items-center gap-3 text-sm">
                启用 {agent.name}
                <Switch
                  aria-label={"启用 " + agent.name}
                  checked={!disabledAgents.includes(agent.name)}
                  disabled={disabled}
                  onCheckedChange={(enabled) =>
                    onChange(
                      "task.disabledAgents",
                      enabled
                        ? disabledAgents.filter((n) => n !== agent.name)
                        : [...disabledAgents, agent.name]
                    )
                  }
                />
              </label>
              {agent.fileName && (
                <Button
                  size="sm"
                  variant="secondary"
                  disabled={disabled}
                  onClick={() => onEdit(agent.fileName)}
                >
                  编辑定义与提示词
                </Button>
              )}
            </div>
            <OmpModelPicker
              label={agent.name + " 模型覆盖"}
              value={selectorText(record(values["task.agentModelOverrides"])[agent.name])}
              models={snapshot.models}
              roles={roles}
              disabled={disabled}
              placeholder={"继承定义 · " + (agent.model.join(", ") || "父会话模型")}
              preview={ompAgentModelPreview(agent, values, snapshot)}
              inheritance={ompAgentModelPreview(agent, values, snapshot, true)}
              onChange={(value) =>
                onRecordChange("task.agentModelOverrides", agent.name, value || undefined)
              }
            />
            <p className="text-xs text-muted-foreground">
              定义模型：{agent.model.join(", ") || "父会话模型"}；定义思考等级：
              {agent.thinking || "原生继承"}。选择器可追加 :high
              等等级；多个回退模型可在手动输入中用逗号分隔。
            </p>
            <div className="grid gap-4 sm:grid-cols-2">
              {[
                ["task.agentPrewalk", "首次编辑交接"],
                ["task.agentAdvisor", "顾问 Agent"],
              ].map(([key, label]) => {
                const value = selectorText(record(values[key])[agent.name]);
                return (
                  <div key={key} className="space-y-2">
                    <label className="text-sm">
                      {label}
                      <Select
                        aria-label={agent.name + " " + label}
                        disabled={disabled}
                        value={["", "on", "off"].includes(value) ? value : "custom"}
                        onChange={(e) =>
                          onRecordChange(
                            key,
                            agent.name,
                            e.target.value === "custom"
                              ? key.endsWith("Prewalk")
                                ? "@smol"
                                : "@advisor"
                              : e.target.value || undefined
                          )
                        }
                      >
                        <option value="">继承定义</option>
                        <option value="on">开启</option>
                        <option value="off">关闭</option>
                        <option value="custom">指定模型 / 角色</option>
                      </Select>
                    </label>
                    {value && !["on", "off"].includes(value) && (
                      <OmpModelPicker
                        label={agent.name + " " + label + "目标"}
                        value={value}
                        preview={ompSelectorModelPreview(value, values, snapshot)}
                        roles={roles}
                        models={snapshot.models}
                        disabled={disabled}
                        onChange={(next) => onRecordChange(key, agent.name, next || undefined)}
                      />
                    )}
                  </div>
                );
              })}
            </div>
            <label className="block space-y-1 text-sm">
              服务等级
              <Select
                aria-label={agent.name + " 服务等级"}
                disabled={disabled}
                value={selectorText(record(values["task.agentServiceTierOverrides"])[agent.name])}
                onChange={(e) =>
                  onRecordChange(
                    "task.agentServiceTierOverrides",
                    agent.name,
                    e.target.value || undefined
                  )
                }
              >
                <option value="">继承原生配置</option>
                {["inherit", "none", "auto", "default", "flex", "scale", "priority"].map((tier) => (
                  <option key={tier} value={tier}>
                    {tier}
                  </option>
                ))}
              </Select>
              <span className="text-xs text-muted-foreground">实际可用等级由模型供应商决定。</span>
            </label>
          </div>
        </details>
      ))}
      <div className="flex gap-2">
        <Input
          aria-label="其他 Agent 名称"
          placeholder="项目或插件中的 Agent 名称"
          value={extraName}
          disabled={disabled}
          onChange={(e) => setExtraName(e.target.value)}
        />
        <Button
          variant="secondary"
          className="shrink-0 whitespace-nowrap"
          disabled={disabled || !extraName.trim()}
          onClick={() => {
            setExtraAgents((v) => [...new Set([...v, extraName.trim()])]);
            setExtraName("");
          }}
        >
          添加覆盖
        </Button>
      </div>
    </div>
  );
}
