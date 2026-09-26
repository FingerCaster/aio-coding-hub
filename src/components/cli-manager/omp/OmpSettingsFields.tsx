import type { OmpSettingField } from "../../../services/ompSettings";
import { FormField } from "../../../ui/FormField";
import { Input } from "../../../ui/Input";
import { Select } from "../../../ui/Select";
import { Button } from "../../../ui/Button";
import type { SettingsValues } from "./ompSettingsForm";

const LABELS: Record<string, string> = {
  auto: "自动",
  global: "全局 / 当前 Profile",
  project: "项目级（由 OMP 在项目中保存）",
  "always-ask": "始终询问",
  write: "写入操作询问",
  yolo: "直接执行",
  "one-at-a-time": "逐条",
  all: "全部",
  default: "默认",
  preferred: "优先委派",
  always: "始终委派",
  patch: "补丁",
  branch: "分支",
  generic: "固定说明",
  ai: "AI 生成",
};
export function OmpSettingsFields({
  fields,
  values,
  disabled,
  onChange,
}: {
  fields: OmpSettingField[];
  values: SettingsValues;
  disabled: boolean;
  onChange: (key: string, value: unknown) => void;
}) {
  return (
    <div className="grid gap-x-6 gap-y-5 lg:grid-cols-2">
      {fields.map((field) => {
        const explicit = values[field.key] != null;
        const value = values[field.key];
        const fallback = field.defaultValue;
        const displayDefault =
          field.kind === "boolean"
            ? fallback
              ? "开启"
              : "关闭"
            : fallback == null
              ? "自动"
              : (LABELS[String(fallback)] ?? String(fallback));
        return (
          <div key={field.key} className="space-y-1">
            <FormField label={field.label} hint={explicit ? "已配置" : "原生默认"}>
              {(id) =>
                field.kind === "number" ? (
                  <Input
                    id={id}
                    type="number"
                    min={field.min ?? undefined}
                    max={field.max ?? undefined}
                    step={1}
                    disabled={disabled}
                    value={value == null ? "" : String(value)}
                    placeholder={displayDefault}
                    onChange={(e) =>
                      onChange(
                        field.key,
                        e.target.validity.badInput
                          ? Number.NaN
                          : e.target.value === ""
                            ? undefined
                            : Number(e.target.value)
                      )
                    }
                  />
                ) : (
                  <Select
                    id={id}
                    disabled={disabled}
                    value={value == null ? "" : String(value)}
                    onChange={(e) =>
                      onChange(
                        field.key,
                        e.target.value === ""
                          ? undefined
                          : field.kind === "boolean"
                            ? e.target.value === "true"
                            : e.target.value
                      )
                    }
                  >
                    <option value="">继承默认 · {displayDefault}</option>
                    {field.kind === "boolean" ? (
                      <>
                        <option value="true">开启</option>
                        <option value="false">关闭</option>
                      </>
                    ) : (
                      <>
                        {explicit && !field.options.includes(String(value)) && (
                          <option value={String(value)}>{String(value)}（已有值）</option>
                        )}
                        {field.options.map((option) => (
                          <option key={option} value={option}>
                            {LABELS[option] ?? option}
                          </option>
                        ))}
                      </>
                    )}
                  </Select>
                )
              }
            </FormField>
            <div className="flex items-center justify-between gap-2">
              <code className="truncate text-[11px] text-muted-foreground" title={field.key}>
                {field.key}
              </code>
              {explicit && (
                <Button
                  size="sm"
                  variant="ghost"
                  className="h-6 px-1 text-xs"
                  disabled={disabled}
                  onClick={() => onChange(field.key, undefined)}
                  aria-label={"恢复默认：" + field.label}
                >
                  恢复默认
                </Button>
              )}
            </div>
          </div>
        );
      })}
    </div>
  );
}
