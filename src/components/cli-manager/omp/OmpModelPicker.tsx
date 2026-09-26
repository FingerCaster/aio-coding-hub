import { useId, useState } from "react";
import type { OmpModelOption } from "../../../services/ompSettings";
import { Input } from "../../../ui/Input";
import { Select } from "../../../ui/Select";
import { EFFORTS } from "./ompSettingsForm";
import { splitOmpModelSelector, type OmpModelPreview } from "./ompModelPreview";

export function OmpModelPicker({
  label,
  value,
  models,
  roles = [],
  disabled,
  onChange,
  placeholder = "继承 OMP 原生选择",
  preview,
  inheritance,
}: {
  label: string;
  value: string;
  models: OmpModelOption[];
  roles?: string[];
  disabled?: boolean;
  onChange: (value: string) => void;
  placeholder?: string;
  preview?: OmpModelPreview;
  inheritance?: OmpModelPreview;
}) {
  const id = useId();
  const [manual, setManual] = useState(false);
  const [search, setSearch] = useState("");
  const { base, effort } = splitOmpModelSelector(value, { models });
  const selected = models.find((m) => m.selector === base);
  const options = models.filter(
    (m) => !search || (m.selector + " " + m.label).toLowerCase().includes(search.toLowerCase())
  );
  const levels = selected?.thinkingLevels.length ? selected.thinkingLevels : EFFORTS;
  const availableLevels = [...new Set(["auto", ...levels, ...(effort ? [effort] : [])])];
  const known = options.some((m) => m.selector === base) || roles.some((r) => "@" + r === base);
  return (
    <div className="min-w-0 space-y-2">
      <label htmlFor={id} className="text-sm font-medium">
        {label}
      </label>
      <div className="grid gap-2 sm:grid-cols-[minmax(0,1fr)_8rem]">
        <Select
          id={id}
          className="min-w-0"
          aria-describedby={preview ? id + "-preview" : undefined}
          disabled={disabled}
          value={base}
          onChange={(event) => {
            if (event.target.value === "__manual__") {
              setManual(true);
              return;
            }
            setManual(false);
            onChange(event.target.value);
          }}
        >
          <option value="">{inheritance ? "继承 · " + inheritance.label : placeholder}</option>
          {base && !known && <option value={base}>{base}（已有 / 手动值）</option>}
          {roles.length > 0 && (
            <optgroup label="模型角色">
              {roles.map((role) => (
                <option key={role} value={"@" + role}>
                  @{role}
                </option>
              ))}
            </optgroup>
          )}
          {options.some((m) => m.source.startsWith("原生")) && (
            <optgroup label="原生供应商与 AIO 入口">
              {options
                .filter((m) => m.source.startsWith("原生"))
                .map((m) => (
                  <option key={m.selector} value={m.selector}>
                    {m.label}
                  </option>
                ))}
            </optgroup>
          )}
          {options.some((m) => !m.source.startsWith("原生")) && (
            <optgroup label="内置目录（未检查登录）">
              {options
                .filter((m) => !m.source.startsWith("原生"))
                .map((m) => (
                  <option key={m.selector} value={m.selector}>
                    {m.label}
                  </option>
                ))}
            </optgroup>
          )}
          <option value="__manual__">手动填写选择器…</option>
        </Select>
        <Select
          className="min-w-0"
          aria-label={label + "思考等级"}
          disabled={disabled || !base || base.includes(",")}
          value={effort}
          onChange={(event) =>
            onChange(base + (event.target.value ? ":" + event.target.value : ""))
          }
        >
          <option value="">{preview ? "默认 · " + preview.thinking : "原生默认"}</option>
          {availableLevels.map((level) => (
            <option key={level} value={level}>
              {level === "auto" ? "auto · 自动" : level}
            </option>
          ))}
        </Select>
      </div>
      {preview && (
        <div
          id={id + "-preview"}
          role="group"
          aria-label={label + "配置解析"}
          className="min-w-0 space-y-1 rounded-lg bg-secondary/40 px-3 py-2 text-xs leading-relaxed"
        >
          <p className="break-words [overflow-wrap:anywhere]">
            <span className="text-muted-foreground">
              {preview.kind === "model" ? "配置解析：" : "选择方式："}
            </span>
            <span className="font-medium">{preview.label}</span>
          </p>
          {preview.selectors.length > 0 && (
            <p className="break-all font-mono text-foreground/80">
              {preview.selectors.join(" → ")}
            </p>
          )}
          <p className="break-words text-muted-foreground [overflow-wrap:anywhere]">
            来源：{preview.source || "当前选择"}；思考：{preview.thinking}
          </p>
          {preview.kind !== "model" && (
            <p className="break-words text-muted-foreground [overflow-wrap:anywhere]">
              {preview.description}
            </p>
          )}
        </div>
      )}
      {manual ? (
        <Input
          aria-label={label + "手动选择器"}
          disabled={disabled}
          value={value}
          placeholder={roles.length ? "provider/model:high 或 @role" : "provider/model:high"}
          onChange={(e) => onChange(e.target.value)}
        />
      ) : (
        <Input
          aria-label={label + "搜索"}
          disabled={disabled}
          value={search}
          placeholder="搜索模型名称或供应商…"
          onChange={(e) => setSearch(e.target.value)}
          className="h-8 text-xs"
        />
      )}
    </div>
  );
}
