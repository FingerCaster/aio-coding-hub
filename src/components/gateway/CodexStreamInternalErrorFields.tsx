import { AlertTriangle, ChevronDown } from "lucide-react";
import { useEffect, useId, useState } from "react";
import type { UpstreamStreamInternalErrorPolicy } from "../../services/settings/settings";
import {
  bodyContainsFromTextarea,
  bodyContainsToTextarea,
} from "../../services/gateway/upstreamRetryPolicy";
import { Input } from "../../ui/Input";
import { Switch } from "../../ui/Switch";
import { Textarea } from "../../ui/Textarea";
import { cn } from "../../utils/cn";

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

export function CodexStreamInternalErrorFields({
  policy,
  disabled,
  onChange,
  streamInternalErrorGuardMs,
  maxStreamInternalErrorGuardMs,
  onStreamInternalErrorGuardMsChange,
}: {
  policy: UpstreamStreamInternalErrorPolicy;
  disabled: boolean;
  onChange: (policy: UpstreamStreamInternalErrorPolicy) => void;
  streamInternalErrorGuardMs?: number;
  maxStreamInternalErrorGuardMs?: number;
  onStreamInternalErrorGuardMsChange?: (value: number) => void;
}) {
  const [detailsOpen, setDetailsOpen] = useState(false);
  const guardInputId = useId();

  return (
    <div className="border-y border-border" data-testid="codex-stream-internal-error-fields">
      <div className="flex min-h-14 items-center gap-2 py-2">
        <button
          type="button"
          className="flex min-w-0 flex-1 items-center gap-2 rounded px-1.5 py-2 text-left transition-colors hover:bg-secondary/60 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
          aria-expanded={detailsOpen}
          aria-label={`${detailsOpen ? "收起" : "查看"} Codex 流终态防火墙设置`}
          onClick={() => setDetailsOpen((current) => !current)}
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
              detailsOpen && "rotate-180"
            )}
            aria-hidden="true"
          />
        </button>
        <Switch
          checked={policy.enabled}
          aria-label="启用 Codex 流终态防火墙"
          disabled={disabled}
          onCheckedChange={(enabled) => onChange({ ...policy, enabled })}
        />
      </div>
      {detailsOpen ? (
        <div className="space-y-4 border-t border-border/70 bg-secondary/20 px-3 py-4">
          <div className="text-xs leading-relaxed text-muted-foreground">
            开启后，瞬时或容量错误会在提交前复用下方共享重试预算；提交后默认丢弃终态帧并结束。
            硬性错误和未知错误在提交前返回 502 / GW_FAKE_200。关闭后不拦截、不重试、不切换，
            所有终态帧保持原样。
          </div>
          <KeywordTextarea
            label="终态帧透传例外（每行一项）"
            values={policy.passthrough_keywords}
            disabled={disabled || !policy.enabled}
            onChange={(passthroughKeywords) =>
              onChange({ ...policy, passthrough_keywords: passthroughKeywords })
            }
          />
          <p className="text-xs leading-relaxed text-muted-foreground">
            例外仅作用于提交后的完整终态帧，可用于明确的安全策略拒绝；容量、认证、配额与无效请求错误始终阻断。
          </p>
          {policy.legacy_retry_keywords.length > 0 ? (
            <div className="flex gap-2 border-l-2 border-amber-500 px-3 py-2 text-xs leading-relaxed text-muted-foreground">
              <AlertTriangle
                className="mt-0.5 h-4 w-4 shrink-0 text-amber-600"
                aria-hidden="true"
              />
              <span>
                已迁移 {policy.legacy_retry_keywords.length}{" "}
                条旧版重试词；它们仅在提交前将未知终态提升为可重试，当前版本不可新增或编辑。
              </span>
            </div>
          ) : null}
          {streamInternalErrorGuardMs != null &&
          maxStreamInternalErrorGuardMs != null &&
          onStreamInternalErrorGuardMsChange ? (
            <div className="flex flex-col gap-2 border-t border-border pt-3 sm:flex-row sm:items-center sm:justify-between">
              <div>
                <label htmlFor={guardInputId} className="text-sm font-medium text-foreground">
                  首个有效输出观察窗口
                </label>
                <p className="mt-1 text-xs text-muted-foreground">
                  在提交给 Codex 前继续观察终态错误；0 表示不额外等待。
                </p>
              </div>
              <div className="flex items-center gap-2">
                <Input
                  id={guardInputId}
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
  );
}
