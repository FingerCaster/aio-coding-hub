import { Plus, Trash2 } from "lucide-react";
import type { NativeCliKey } from "../../../constants/clis";
import type { JsonValue } from "../../../generated/bindings";
import { Button } from "../../../ui/Button";
import { FormField } from "../../../ui/FormField";
import { Input } from "../../../ui/Input";
import { Select } from "../../../ui/Select";
import { isNativeNode, nativeString, setNativeField, type NativeNode } from "./nativeProviderDraft";

export function NativeModelFields({
  client,
  models,
  disabled,
  onChange,
}: {
  client: NativeCliKey;
  models: JsonValue | undefined;
  disabled: boolean;
  onChange: (models: JsonValue[]) => void;
}) {
  if (models !== undefined && (!Array.isArray(models) || models.some((m) => !isNativeNode(m)))) {
    return (
      <div
        role="alert"
        className="rounded-lg border border-amber-400/60 bg-amber-50/50 p-3 text-xs text-amber-900 dark:border-amber-700/60 dark:bg-amber-950/20 dark:text-amber-300"
      >
        模型结构不可识别，请使用原文编辑；现有内容已保留。
      </div>
    );
  }
  const rows = (models ?? []) as NativeNode[];
  function update(index: number, key: string, value: JsonValue | undefined) {
    onChange(rows.map((row, i) => (i === index ? setNativeField(row, key, value) : row)));
  }
  return (
    <div className="space-y-3">
      <div className="flex flex-col gap-1">
        <div className="text-xs font-semibold text-foreground">显式模型能力定义</div>
        <p className="text-xs text-muted-foreground">
          能力由你明确填写，不根据模型名称推断。
          {client === "pi" ? "Pi 使用 thinkingLevelMap。" : "OMP 使用 thinking 对象。"}
          复杂思考、成本、兼容性和覆盖项可在原文中编辑。
        </p>
      </div>

      {rows.map((row, index) => (
        <fieldset
          key={index}
          disabled={disabled}
          className="space-y-3 rounded-xl border border-border/70 bg-surface-panel/30 p-3.5"
        >
          <div className="flex items-center justify-between border-b border-border/50 pb-2">
            <legend className="px-1 text-xs font-semibold text-foreground font-mono">
              模型 {index + 1}
            </legend>
            <Button
              variant="danger"
              size="sm"
              className="h-7 px-2 text-[11px] gap-1"
              onClick={() => onChange(rows.filter((_, i) => i !== index))}
            >
              <Trash2 className="h-3 w-3" aria-hidden="true" />
              删除模型 {index + 1}
            </Button>
          </div>
          <div className="grid gap-3 sm:grid-cols-2">
            {[
              ["id", "模型 ID"],
              ["name", "显示名称"],
              ["api", "模型协议覆盖"],
              ["baseUrl", "模型地址覆盖"],
            ].map(([key, label]) => (
              <FormField key={key} label={label}>
                {(id) => (
                  <Input
                    id={id}
                    value={nativeString(row, key)}
                    onChange={(e) => update(index, key, e.target.value || undefined)}
                    className="h-9 text-xs"
                  />
                )}
              </FormField>
            ))}
            {[
              ["contextWindow", "上下文窗口"],
              ["maxTokens", "最大输出 Token"],
            ].map(([key, label]) => (
              <FormField key={key} label={label} hint="留空保留原生默认语义">
                {(id) => (
                  <Input
                    id={id}
                    type="number"
                    min={1}
                    step={1}
                    value={typeof row[key] === "number" ? row[key] : ""}
                    onChange={(e) =>
                      update(index, key, e.target.value === "" ? undefined : Number(e.target.value))
                    }
                    className="h-9 text-xs"
                  />
                )}
              </FormField>
            ))}
            <FormField label="思考能力">
              {(id) => (
                <Select
                  id={id}
                  value={typeof row.reasoning === "boolean" ? String(row.reasoning) : "unset"}
                  onChange={(e) =>
                    update(
                      index,
                      "reasoning",
                      e.target.value === "unset" ? undefined : e.target.value === "true"
                    )
                  }
                  className="h-9 text-xs"
                >
                  <option value="unset">未声明</option>
                  <option value="true">支持</option>
                  <option value="false">不支持</option>
                </Select>
              )}
            </FormField>
            <FormField label="输入类型" hint="逗号分隔；不自动补充图像能力">
              {(id) => (
                <Input
                  id={id}
                  value={
                    Array.isArray(row.input)
                      ? row.input.filter((v) => typeof v === "string").join(", ")
                      : ""
                  }
                  onChange={(e) =>
                    update(
                      index,
                      "input",
                      e.target.value.trim()
                        ? e.target.value
                            .split(",")
                            .map((s) => s.trim())
                            .filter(Boolean)
                        : undefined
                    )
                  }
                  placeholder="例如：text, image"
                  className="h-9 text-xs"
                />
              )}
            </FormField>
          </div>
        </fieldset>
      ))}

      <Button
        variant="secondary"
        size="sm"
        disabled={disabled}
        className="h-8 text-xs gap-1.5"
        onClick={() => onChange([...rows, { id: "" }])}
      >
        <Plus className="h-3.5 w-3.5" aria-hidden="true" />
        添加模型
      </Button>
    </div>
  );
}
