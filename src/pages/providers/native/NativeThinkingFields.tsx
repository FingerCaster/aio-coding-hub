import type { NativeCliKey } from "../../../constants/clis";
import type { ThinkingSpec } from "../../../services/nativeChannels";
import { Select } from "../../../ui/Select";
import {
  emptyThinking,
  OMP_THINKING_MODES,
  THINKING_LEVELS,
} from "./nativeChannelModelSuggestions";

export function NativeThinkingFields({
  client,
  value,
  supported,
  label,
  onChange,
}: {
  client: NativeCliKey;
  value: ThinkingSpec | null;
  supported?: string[] | null;
  label: string;
  onChange: (value: ThinkingSpec) => void;
}) {
  const thinking = value?.client === client ? value : emptyThinking(client);
  const sourceLevels = supported?.length ? supported : [...THINKING_LEVELS];
  function setMapping(level: string, target: string) {
    if (thinking.client === "pi") {
      onChange({ ...thinking, levelMap: { ...thinking.levelMap, [level]: target || null } });
    } else {
      const efforts = THINKING_LEVELS.filter(
        (item) =>
          item !== "off" && (item === level ? Boolean(target) : thinking.efforts.includes(item))
      );
      const effortMap = { ...thinking.effortMap };
      if (target) effortMap[level] = target;
      else delete effortMap[level];
      onChange({
        ...thinking,
        efforts,
        effortMap,
        defaultLevel: efforts.includes(thinking.defaultLevel as (typeof efforts)[number])
          ? thinking.defaultLevel
          : null,
      });
    }
  }
  return (
    <div className="space-y-3 rounded-lg border border-line bg-surface-inset/40 p-3">
      <p className="text-xs text-muted-foreground">
        {supported?.length
          ? "已获取上游思考等级，可调整 CLI 档位与上游等级的对应关系。"
          : "上游未返回完整思考规格，请按来源能力选择；无需编辑 JSON。"}
      </p>
      {thinking.client === "omp" && (
        <label className="block space-y-1 text-xs">
          <span>思考模式</span>
          <Select
            aria-label={`${label} 思考模式`}
            value={thinking.mode}
            onChange={(e) => onChange({ ...thinking, mode: e.target.value })}
          >
            <option value="">请选择思考模式</option>
            {Object.entries(OMP_THINKING_MODES).map(([mode, name]) => (
              <option key={mode} value={mode}>
                {name}
              </option>
            ))}
          </Select>
        </label>
      )}
      <div className="grid grid-cols-2 gap-3 sm:grid-cols-3">
        {THINKING_LEVELS.filter((level) => client === "pi" || level !== "off").map((level) => {
          const selected =
            thinking.client === "pi"
              ? (thinking.levelMap[level] ?? "")
              : thinking.efforts.includes(level)
                ? (thinking.effortMap[level] ?? level)
                : "";
          const choices = [...new Set([...sourceLevels, ...(selected ? [selected] : [])])].filter(
            (target) => level === "off" || !["off", "none"].includes(target)
          );
          return (
            <label key={level} className="space-y-1 text-xs">
              <span>{level} 对应上游等级</span>
              <Select
                aria-label={`${label} ${level} 对应上游等级`}
                value={selected}
                onChange={(e) => setMapping(level, e.target.value)}
              >
                <option value="">不支持 / 不发布</option>
                {choices.map((target) => (
                  <option key={target} value={target}>
                    {target}
                  </option>
                ))}
              </Select>
            </label>
          );
        })}
      </div>
      {thinking.client === "omp" && (
        <div className="grid grid-cols-1 gap-3 sm:grid-cols-3">
          <label className="space-y-1 text-xs">
            <span>默认思考等级</span>
            <Select
              aria-label={`${label} 默认思考等级`}
              value={thinking.defaultLevel ?? ""}
              onChange={(e) => onChange({ ...thinking, defaultLevel: e.target.value || null })}
            >
              <option value="">沿用 CLI 默认</option>
              {thinking.efforts.map((level) => (
                <option key={level} value={level}>
                  {level}
                </option>
              ))}
            </Select>
          </label>
          {(["supportsDisplay", "requiresEffort"] as const).map((key) => {
            const name = key === "supportsDisplay" ? "显示思考内容" : "必须指定等级";
            return (
              <label key={key} className="space-y-1 text-xs">
                <span>{name}</span>
                <Select
                  aria-label={`${label} ${name}`}
                  value={thinking[key] == null ? "" : String(thinking[key])}
                  onChange={(e) =>
                    onChange({
                      ...thinking,
                      [key]: e.target.value === "" ? null : e.target.value === "true",
                    })
                  }
                >
                  <option value="">自动（原生默认）</option>
                  <option value="true">是</option>
                  <option value="false">否</option>
                </Select>
              </label>
            );
          })}
        </div>
      )}
    </div>
  );
}
