import { useMemo, useState } from "react";
import { toast } from "sonner";
import { Spinner } from "../ui/Spinner";
import { cn } from "../utils/cn";
import { Globe, AlertTriangle, Zap, ChevronDown, ArrowRight, Copy } from "lucide-react";
import { getGatewayErrorShortLabel } from "../constants/gatewayErrorCodes";
import { DisclosureSection } from "./home/DisclosureSection";
import {
  isCircuitProbeAttempt,
  normalizeAttemptProbeMetadata,
  parseAttemptsJson,
  type AttemptJsonEntry,
  type AttemptProbeMetadata,
  type StreamInternalErrorEvidence,
} from "../services/gateway/attemptsJson";
import { normalizeCircuitState } from "../services/gateway/circuitState";
import { formatCircuitRecovery } from "../utils/formatters";
import { copyText } from "../services/clipboard";
import { Tooltip } from "../ui/Tooltip";

export type ProviderChainAttemptLog = AttemptProbeMetadata & {
  attempt_index: number;
  provider_id: number;
  provider_name: string;
  base_url: string;
  outcome: string;
  status: number | null;
  attempt_started_ms?: number | null;
  attempt_duration_ms?: number | null;
  session_reuse?: boolean | null;
  selection_method?: string | null;
  circuit_state_before?: string | null;
  circuit_state_after?: string | null;
  circuit_failure_count?: number | null;
  circuit_failure_threshold?: number | null;
  stream_internal_error?: StreamInternalErrorEvidence | null;
};

type ProviderChainAttempt = {
  attempt_index: number;
  provider_id: number;
  provider_name: string;
  base_url: string;
  outcome: string;
  status: number | null;
  attempt_started_ms: number | null;
  attempt_duration_ms: number | null;
  provider_index: number | null;
  retry_index: number | null;
  session_reuse: boolean | null;
  error_category: string | null;
  error_code: string | null;
  decision: string | null;
  reason: string | null;
  selection_method: string | null;
  reason_code: string | null;
  circuit_state_before: string | null;
  circuit_state_after: string | null;
  circuit_failure_count: number | null;
  circuit_failure_threshold: number | null;
  circuit_recover_at_unix: number | null;
  circuit_trigger_error_code: string | null;
  probe: boolean | null;
  probe_trigger: string | null;
  probe_result: string | null;
  probe_generation: number | null;
  stream_internal_error: StreamInternalErrorEvidence | null;
};

function normalizeAttemptFromSources({
  attemptIndex,
  log,
  live,
  json,
}: {
  attemptIndex: number;
  log?: ProviderChainAttemptLog;
  live?: ProviderChainAttemptLog;
  json?: AttemptJsonEntry;
}): ProviderChainAttempt {
  const liveProbe = live ? normalizeAttemptProbeMetadata(live) : null;
  const logProbe = log ? normalizeAttemptProbeMetadata(log) : null;
  const jsonProbe = json ? normalizeAttemptProbeMetadata(json) : null;

  return {
    attempt_index: attemptIndex,
    provider_id: live?.provider_id ?? log?.provider_id ?? json?.provider_id ?? 0,
    provider_name: live?.provider_name || log?.provider_name || json?.provider_name || "未知",
    base_url: live?.base_url || log?.base_url || json?.base_url || "",
    outcome: live?.outcome || log?.outcome || json?.outcome || "",
    status: live?.status ?? log?.status ?? json?.status ?? null,
    attempt_started_ms:
      live?.attempt_started_ms ?? log?.attempt_started_ms ?? json?.attempt_started_ms ?? null,
    attempt_duration_ms:
      live?.attempt_duration_ms ?? log?.attempt_duration_ms ?? json?.attempt_duration_ms ?? null,
    provider_index: json?.provider_index ?? null,
    retry_index: json?.retry_index ?? null,
    session_reuse: live?.session_reuse ?? log?.session_reuse ?? json?.session_reuse ?? null,
    error_category: json?.error_category ?? null,
    error_code: json?.error_code ?? null,
    decision: json?.decision ?? null,
    reason: json?.reason ?? null,
    selection_method:
      live?.selection_method ??
      log?.selection_method ??
      json?.selection_method ??
      (liveProbe?.probe === true || logProbe?.probe === true ? "circuit_probe" : null),
    reason_code: json?.reason_code ?? null,
    circuit_state_before:
      live?.circuit_state_before ?? log?.circuit_state_before ?? json?.circuit_state_before ?? null,
    circuit_state_after:
      live?.circuit_state_after ?? log?.circuit_state_after ?? json?.circuit_state_after ?? null,
    circuit_failure_count:
      live?.circuit_failure_count ??
      log?.circuit_failure_count ??
      json?.circuit_failure_count ??
      null,
    circuit_failure_threshold:
      live?.circuit_failure_threshold ??
      log?.circuit_failure_threshold ??
      json?.circuit_failure_threshold ??
      null,
    circuit_recover_at_unix: json?.circuit_recover_at_unix ?? null,
    circuit_trigger_error_code: json?.circuit_trigger_error_code ?? null,
    probe: liveProbe?.probe ?? logProbe?.probe ?? jsonProbe?.probe ?? null,
    probe_trigger:
      liveProbe?.probe_trigger ?? logProbe?.probe_trigger ?? jsonProbe?.probe_trigger ?? null,
    probe_result:
      liveProbe?.probe_result ?? logProbe?.probe_result ?? jsonProbe?.probe_result ?? null,
    probe_generation:
      liveProbe?.probe_generation ??
      logProbe?.probe_generation ??
      jsonProbe?.probe_generation ??
      null,
    stream_internal_error:
      live?.stream_internal_error ??
      log?.stream_internal_error ??
      json?.stream_internal_error ??
      null,
  };
}

function formatProviderIdentity(providerName: unknown, providerId: unknown): string {
  const normalizedName = typeof providerName === "string" ? providerName.trim() : "";
  const providerLabel =
    normalizedName && normalizedName.toLowerCase() !== "unknown" && normalizedName !== "未知"
      ? normalizedName
      : "未知供应商";
  const hasStableId =
    typeof providerId === "number" && Number.isSafeInteger(providerId) && providerId > 0;

  return hasStableId ? `${providerLabel} (#${providerId})` : `${providerLabel} (ID 不可用)`;
}

export function ProviderChainView({
  attemptLogs,
  liveAttempts = [],
  attemptLogsLoading,
  attemptsJson,
}: {
  attemptLogs: ProviderChainAttemptLog[];
  liveAttempts?: ProviderChainAttemptLog[];
  attemptLogsLoading?: boolean;
  attemptsJson: string | null | undefined;
}) {
  const parsedAttemptsJson = useMemo(() => {
    const attempts = parseAttemptsJson(attemptsJson);
    return attempts
      ? { ok: true as const, attempts }
      : { ok: false as const, attempts: null as AttemptJsonEntry[] | null };
  }, [attemptsJson]);

  const attempts = useMemo((): ProviderChainAttempt[] | null => {
    const logs = attemptLogs ?? [];
    const live = liveAttempts ?? [];
    const jsonAttempts = parsedAttemptsJson.ok ? parsedAttemptsJson.attempts : null;

    if (logs.length === 0 && live.length === 0 && !jsonAttempts) return null;

    const logsByAttemptIndex = new Map(logs.map((log) => [log.attempt_index, log]));
    const liveByAttemptIndex = new Map(live.map((attempt) => [attempt.attempt_index, attempt]));
    const jsonByAttemptIndex = new Map(
      (jsonAttempts ?? []).map((attempt, index) => [index + 1, attempt])
    );
    const attemptIndexes = new Set([
      ...logsByAttemptIndex.keys(),
      ...liveByAttemptIndex.keys(),
      ...jsonByAttemptIndex.keys(),
    ]);

    return [...attemptIndexes]
      .sort((a, b) => a - b)
      .map((attemptIndex) =>
        normalizeAttemptFromSources({
          attemptIndex,
          log: logsByAttemptIndex.get(attemptIndex),
          live: liveByAttemptIndex.get(attemptIndex),
          json: jsonByAttemptIndex.get(attemptIndex),
        })
      );
  }, [attemptLogs, liveAttempts, parsedAttemptsJson]);

  const dataSourceLabel = useMemo(() => {
    if (attemptLogsLoading) return "加载中…";
    if (liveAttempts.length > 0) {
      return parsedAttemptsJson.ok
        ? "数据源：实时网关事件 + request_logs.attempts_json"
        : "数据源：实时网关事件";
    }
    if (attemptLogs.length > 0) {
      return parsedAttemptsJson.ok
        ? "数据源：request_logs.attempts_json（结构化）"
        : "数据源：attempts 兼容接口";
    }
    if (parsedAttemptsJson.ok) return "数据源：request_logs.attempts_json";
    return "数据源：尝试 JSON（原始）";
  }, [attemptLogs.length, attemptLogsLoading, liveAttempts.length, parsedAttemptsJson.ok]);

  if (attemptLogsLoading) {
    return (
      <div className="mt-2 flex items-center gap-2 text-sm text-muted-foreground">
        <Spinner size="sm" />
        加载中…
      </div>
    );
  }

  if (!attempts) {
    return <div className="mt-2 text-sm text-muted-foreground">无故障切换尝试。</div>;
  }

  if (attempts.length === 0) {
    return <div className="mt-2 text-sm text-muted-foreground">无故障切换尝试。</div>;
  }

  const startAttempt = attempts[0] ?? null;
  const finalAttempt = attempts.length > 0 ? attempts[attempts.length - 1] : null;
  const startProviderLabel = startAttempt
    ? formatProviderIdentity(startAttempt.provider_name, startAttempt.provider_id)
    : "—";
  const finalProviderLabel = finalAttempt
    ? formatProviderIdentity(finalAttempt.provider_name, finalAttempt.provider_id)
    : "—";
  const finalSuccess = finalAttempt ? finalAttempt.outcome === "success" : false;

  return (
    <div className="mt-4 space-y-4">
      <div className="flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
        <span className="rounded-full bg-secondary px-2.5 py-1">
          起始供应商：
          <span className="font-medium text-foreground">{startProviderLabel}</span>
        </span>
        <span className="text-muted-foreground">→</span>
        <span className="rounded-full bg-secondary px-2.5 py-1">
          最终供应商：
          <span className="font-medium text-foreground">{finalProviderLabel}</span>
        </span>
        <span className="rounded-full bg-secondary px-2.5 py-1">共尝试 {attempts.length} 次</span>
        {finalAttempt ? (
          <span
            className={cn(
              "rounded-full px-2.5 py-1 font-medium",
              finalSuccess
                ? "bg-emerald-50 text-emerald-700 dark:bg-emerald-900/30 dark:text-emerald-400"
                : "bg-rose-50 text-rose-700 dark:bg-rose-900/30 dark:text-rose-400"
            )}
          >
            {finalSuccess ? "最终成功" : "最终失败"}
          </span>
        ) : null}
        <span className="text-muted-foreground">{dataSourceLabel}</span>
        {attemptLogs.length === 0 && liveAttempts.length === 0 && parsedAttemptsJson.ok ? (
          <span className="rounded-full bg-secondary px-2.5 py-1 font-medium text-secondary-foreground">
            当前显示的是摘要链路，未拿到逐次尝试日志
          </span>
        ) : null}
        {attemptsJson && !parsedAttemptsJson.ok ? (
          <span className="rounded-full bg-amber-50 dark:bg-amber-900/30 px-2.5 py-1 font-medium text-amber-700 dark:text-amber-400">
            尝试 JSON 解析失败
          </span>
        ) : null}
      </div>

      <div className="relative pl-8">
        <div className="absolute left-[15px] top-2 bottom-2 w-px bg-muted dark:bg-secondary" />
        <div className="space-y-4">
          {attempts.map((attempt) => (
            <AttemptCard
              key={`${attempt.attempt_index}-${attempt.provider_id}-${attempt.base_url}`}
              attempt={attempt}
              isFinal={Boolean(
                finalAttempt && attempt.attempt_index === finalAttempt.attempt_index
              )}
              hasMultipleAttempts={attempts.length > 1}
            />
          ))}
        </div>
      </div>
    </div>
  );
}

function AttemptCard({
  attempt,
  isFinal,
  hasMultipleAttempts,
}: {
  attempt: ProviderChainAttempt;
  isFinal: boolean;
  hasMultipleAttempts: boolean;
}) {
  const [expanded, setExpanded] = useState(true);
  const success = attempt.outcome === "success";
  const skipped = attempt.outcome === "skipped";
  const providerIdentity = formatProviderIdentity(attempt.provider_name, attempt.provider_id);

  const hasCircuitBreaker =
    attempt.circuit_state_after != null || attempt.circuit_state_before != null;

  return (
    <div className="relative">
      <div
        className={cn(
          "absolute -left-8 top-4 flex h-8 w-8 items-center justify-center rounded-full border-2 bg-white text-sm font-semibold shadow-sm dark:bg-card",
          success
            ? "border-emerald-300 text-emerald-600 dark:border-emerald-700 dark:text-emerald-400"
            : skipped
              ? "border-border text-muted-foreground dark:border-border dark:text-secondary-foreground"
              : "border-rose-300 text-rose-600 dark:border-rose-700 dark:text-rose-400"
        )}
      >
        {attempt.attempt_index}
      </div>

      <div
        className={cn(
          "rounded-2xl border bg-white shadow-sm dark:bg-secondary/90 overflow-hidden",
          isFinal
            ? success
              ? "border-emerald-200 bg-emerald-50/50 dark:border-emerald-700 dark:bg-emerald-900/20"
              : skipped
                ? "border-border bg-secondary/50 dark:border-border dark:bg-secondary/20"
                : "border-rose-200 bg-rose-50/50 dark:border-rose-700 dark:bg-rose-900/20"
            : "border-border"
        )}
      >
        {/* Header */}
        <button
          type="button"
          className="w-full text-left px-4 py-3 flex items-center justify-between gap-2 hover:bg-secondary/50 dark:hover:bg-secondary/30 transition-colors"
          onClick={() => setExpanded((prev) => !prev)}
        >
          <div className="flex flex-wrap items-center gap-2 min-w-0">
            <span className="text-base font-semibold text-foreground">
              {success
                ? `请求成功`
                : skipped
                  ? `跳过`
                  : hasMultipleAttempts
                    ? `重试 #${attempt.attempt_index}`
                    : `请求失败`}
            </span>
            <span className="min-w-0 break-words text-sm font-medium text-foreground">
              {providerIdentity}
            </span>
            <ProbeTags attempt={attempt} />
            {attempt.attempt_duration_ms != null ? (
              <span className="text-xs text-muted-foreground">
                +{attempt.attempt_duration_ms}ms
              </span>
            ) : null}
            {attempt.status != null ? (
              <span
                className={cn(
                  "text-xs font-medium",
                  attempt.status >= 400
                    ? "text-rose-600 dark:text-rose-400"
                    : "text-muted-foreground"
                )}
              >
                HTTP {attempt.status}
              </span>
            ) : null}
          </div>
          <ChevronDown
            className={cn(
              "h-4 w-4 text-muted-foreground shrink-0 transition-transform",
              expanded && "rotate-180"
            )}
          />
        </button>

        {/* Detail body */}
        {expanded && (
          <div className="px-4 pb-4 space-y-3 border-t border-border/50 pt-3">
            <div className="text-sm text-muted-foreground">
              供应商：
              <span className="font-semibold text-foreground">{providerIdentity}</span>
            </div>

            {/* Decision tags */}
            <DecisionTags attempt={attempt} />

            {attempt.base_url ? (
              <div className="flex items-start gap-2 text-sm">
                <Globe className="h-4 w-4 text-muted-foreground shrink-0 mt-0.5" />
                <div>
                  <span className="text-muted-foreground">端点</span>
                  <div className="font-mono text-foreground break-all">{attempt.base_url}</div>
                </div>
              </div>
            ) : null}

            {hasCircuitBreaker ? (
              <div className="flex flex-wrap items-center gap-2 text-sm">
                <Zap className="h-4 w-4 text-muted-foreground shrink-0" />
                <span className="text-muted-foreground">熔断器:</span>
                <CircuitBadge attempt={attempt} />
                {attempt.circuit_trigger_error_code ? (
                  <span className="text-sm text-muted-foreground">
                    触发：{getGatewayErrorShortLabel(attempt.circuit_trigger_error_code)}
                  </span>
                ) : null}
                {attempt.circuit_recover_at_unix != null ? (
                  <span className="text-sm text-muted-foreground">
                    {formatCircuitRecovery(attempt.circuit_recover_at_unix)}
                  </span>
                ) : null}
              </div>
            ) : null}

            {skipped && attempt.reason ? (
              <div className="rounded-lg border border-border bg-secondary/50 px-3 py-2">
                <div className="mb-1 text-xs font-medium text-muted-foreground">跳过原因</div>
                <pre className="whitespace-pre-wrap break-all text-xs font-mono text-secondary-foreground leading-relaxed">
                  {attempt.reason}
                </pre>
              </div>
            ) : null}

            {!success && !skipped && attempt.reason ? (
              <div className="rounded-lg border border-rose-200/60 bg-rose-50/50 px-3 py-3 dark:border-rose-500/20 dark:bg-rose-950/20">
                <div className="flex items-center gap-1.5 mb-2">
                  <AlertTriangle className="h-4 w-4 text-rose-500 dark:text-rose-400 shrink-0" />
                  <span className="text-sm font-semibold text-rose-600 dark:text-rose-400">
                    错误
                  </span>
                  {attempt.reason_code && attempt.reason_code !== attempt.reason ? (
                    <span className="rounded-full bg-rose-100 px-2 py-0.5 text-xs font-medium text-rose-600 dark:bg-rose-900/30 dark:text-rose-300">
                      {attempt.reason_code}
                    </span>
                  ) : null}
                </div>
                <pre className="whitespace-pre-wrap break-all text-xs font-mono text-rose-800 dark:text-rose-200 leading-relaxed">
                  {attempt.reason}
                </pre>
              </div>
            ) : null}

            {attempt.stream_internal_error ? (
              <StreamInternalErrorDetails evidence={attempt.stream_internal_error} />
            ) : null}

            {/* Expandable structured error details */}
            {!success && !skipped && hasStructuredDetails(attempt) ? (
              <DisclosureSection label="结构化错误详情">
                <div className="space-y-1.5 text-xs">
                  {attempt.error_code ? (
                    <div className="flex items-baseline gap-2">
                      <span className="shrink-0 text-muted-foreground">错误码:</span>
                      <span className="font-mono text-secondary-foreground">
                        {getGatewayErrorShortLabel(attempt.error_code)} ({attempt.error_code})
                      </span>
                    </div>
                  ) : null}
                  {attempt.error_category ? (
                    <div className="flex items-baseline gap-2">
                      <span className="shrink-0 text-muted-foreground">错误分类:</span>
                      <span className="font-mono text-secondary-foreground">
                        {attempt.error_category}
                      </span>
                    </div>
                  ) : null}
                  {attempt.decision ? (
                    <div className="flex items-baseline gap-2">
                      <span className="shrink-0 text-muted-foreground">决策:</span>
                      <span className="font-mono text-secondary-foreground">
                        {attempt.decision}
                      </span>
                    </div>
                  ) : null}
                  {attempt.selection_method ? (
                    <div className="flex items-baseline gap-2">
                      <span className="shrink-0 text-muted-foreground">选择方式:</span>
                      <span className="font-mono text-secondary-foreground">
                        {formatSelectionMethod(attempt.selection_method, true)}
                      </span>
                    </div>
                  ) : null}
                  {hasCircuitBreaker ? (
                    <div className="flex items-baseline gap-2">
                      <span className="shrink-0 text-muted-foreground">熔断器变化:</span>
                      <span className="font-mono text-secondary-foreground">
                        {attempt.circuit_state_before ?? "—"}
                        {attempt.circuit_state_after &&
                        attempt.circuit_state_after !== attempt.circuit_state_before ? (
                          <>
                            {" "}
                            <ArrowRight className="inline h-3 w-3" /> {attempt.circuit_state_after}
                          </>
                        ) : null}
                        {attempt.circuit_failure_count != null &&
                        attempt.circuit_failure_threshold != null
                          ? ` (${attempt.circuit_failure_count}/${attempt.circuit_failure_threshold})`
                          : null}
                      </span>
                    </div>
                  ) : null}
                </div>
              </DisclosureSection>
            ) : null}
          </div>
        )}
      </div>
    </div>
  );
}

function StreamInternalErrorDetails({ evidence }: { evidence: StreamInternalErrorEvidence }) {
  const fields = [
    ["事件", evidence.event_type],
    ["错误类型", evidence.error_type],
    ["错误码", evidence.error_code],
    ["分类", evidence.classification],
    ["匹配关键词", evidence.matched_keyword],
    ["处理", evidence.disposition],
  ].filter((field): field is [string, string] => Boolean(field[1]));

  const copyMessage = async () => {
    if (!evidence.message) return;
    try {
      await copyText(evidence.message);
      toast("流内部错误消息已复制");
    } catch {
      toast("复制流内部错误消息失败");
    }
  };

  return (
    <div className="border-l-2 border-amber-400 py-1 pl-3 dark:border-amber-500">
      <div className="mb-2 flex items-center justify-between gap-2">
        <div className="flex items-center gap-1.5">
          <AlertTriangle className="h-4 w-4 shrink-0 text-amber-600 dark:text-amber-400" />
          <span className="text-sm font-semibold text-amber-800 dark:text-amber-300">
            流内部错误
          </span>
        </div>
        {evidence.message ? (
          <Tooltip content="复制已脱敏错误消息">
            <button
              type="button"
              className="inline-flex h-7 w-7 items-center justify-center rounded-md text-amber-700 transition-colors hover:bg-amber-100 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring dark:text-amber-300 dark:hover:bg-amber-900/40"
              aria-label="复制已脱敏错误消息"
              onClick={() => void copyMessage()}
            >
              <Copy aria-hidden="true" className="h-3.5 w-3.5" />
            </button>
          </Tooltip>
        ) : null}
      </div>
      {evidence.message ? (
        <pre className="mb-2 max-h-40 overflow-auto whitespace-pre-wrap break-all rounded-md bg-white/70 px-2.5 py-2 text-xs font-mono leading-relaxed text-amber-950 dark:bg-black/15 dark:text-amber-100">
          {evidence.message}
        </pre>
      ) : null}
      <div className="space-y-1 text-xs">
        {fields.map(([label, value]) => (
          <div key={label} className="flex items-baseline gap-2">
            <span className="shrink-0 text-muted-foreground">{label}:</span>
            <span className="break-all font-mono text-secondary-foreground">{value}</span>
          </div>
        ))}
        {evidence.truncated ? (
          <div className="text-muted-foreground">部分字段已按日志上限截断</div>
        ) : null}
      </div>
    </div>
  );
}

// --- Helper components for AttemptCard ---

const DECISION_BADGE_TONES: Record<string, string> = {
  switch: "bg-amber-50 text-amber-700 dark:bg-amber-900/30 dark:text-amber-300",
  retry: "bg-blue-50 text-blue-700 dark:bg-blue-900/30 dark:text-blue-300",
  abort: "bg-rose-50 text-rose-700 dark:bg-rose-900/30 dark:text-rose-300",
};

const PROBE_RESULT_PRESENTATION: Record<string, { label: string; tone: string }> = {
  started: {
    label: "试探中",
    tone: "bg-sky-50 text-sky-700 dark:bg-sky-900/30 dark:text-sky-300",
  },
  success: {
    label: "试探成功",
    tone: "bg-emerald-50 text-emerald-700 dark:bg-emerald-900/30 dark:text-emerald-300",
  },
  failed: {
    label: "试探失败",
    tone: "bg-rose-50 text-rose-700 dark:bg-rose-900/30 dark:text-rose-300",
  },
  cooldown: {
    label: "试探冷却中",
    tone: "bg-amber-50 text-amber-700 dark:bg-amber-900/30 dark:text-amber-300",
  },
  in_flight: {
    label: "已有试探进行中",
    tone: "bg-cyan-50 text-cyan-700 dark:bg-cyan-900/30 dark:text-cyan-300",
  },
  not_triggered: {
    label: "试探未触发",
    tone: "bg-secondary text-muted-foreground dark:bg-secondary dark:text-secondary-foreground",
  },
};

const PROBE_TRIGGER_LABELS: Record<string, string> = {
  new_unbound_session: "新会话",
  route_changed: "路由变化",
  natural_compaction: "上下文压缩",
  natural_max_wait: "自然等待到期",
  aggressive_turn: "积极回切",
  max_open_wait: "最长熔断等待到期",
};

function formatSelectionMethod(value: string, includeCode = false) {
  if (value !== "circuit_probe") return value;
  return includeCode ? "熔断试探 (circuit_probe)" : "熔断试探";
}

function ProbeTags({ attempt }: { attempt: ProviderChainAttempt }) {
  if (!isCircuitProbeAttempt(attempt)) return null;

  const result = attempt.probe_result;
  const presentation = result
    ? (PROBE_RESULT_PRESENTATION[result] ?? {
        label: `试探：${result}`,
        tone: "bg-secondary text-secondary-foreground",
      })
    : {
        label: "熔断试探",
        tone: "bg-sky-50 text-sky-700 dark:bg-sky-900/30 dark:text-sky-300",
      };
  const trigger = attempt.probe_trigger
    ? (PROBE_TRIGGER_LABELS[attempt.probe_trigger] ?? attempt.probe_trigger)
    : null;

  return (
    <>
      <span
        className={cn("rounded-full px-2 py-0.5 text-xs font-medium", presentation.tone)}
        title={result ? `probe_result=${result}` : "selection_method=circuit_probe"}
      >
        {presentation.label}
      </span>
      {trigger ? (
        <span
          className="rounded-full bg-secondary px-2 py-0.5 text-xs font-medium text-secondary-foreground"
          title={`probe_trigger=${attempt.probe_trigger}`}
        >
          触发：{trigger}
        </span>
      ) : null}
      {attempt.probe_generation != null ? (
        <span
          className="rounded-full bg-secondary px-2 py-0.5 text-xs font-medium text-muted-foreground"
          title={`probe_generation=${attempt.probe_generation}`}
        >
          代次 {attempt.probe_generation}
        </span>
      ) : null}
    </>
  );
}

function DecisionTags({ attempt }: { attempt: ProviderChainAttempt }) {
  const tags: Array<{ label: string; value: string; tone: string }> = [];

  if (attempt.selection_method) {
    tags.push({
      label: "选择",
      value: formatSelectionMethod(attempt.selection_method),
      tone: "bg-blue-50 text-blue-700 dark:bg-blue-900/30 dark:text-blue-300",
    });
  }
  if (attempt.decision) {
    tags.push({
      label: "决策",
      value: attempt.decision,
      tone:
        DECISION_BADGE_TONES[attempt.decision] ??
        "bg-secondary text-secondary-foreground dark:bg-secondary dark:text-secondary-foreground",
    });
  }
  if (attempt.error_code) {
    tags.push({
      label: "错误码",
      value: getGatewayErrorShortLabel(attempt.error_code),
      tone: "bg-rose-50 text-rose-700 dark:bg-rose-900/30 dark:text-rose-300",
    });
  }
  if (attempt.error_category) {
    tags.push({
      label: "分类",
      value: attempt.error_category,
      tone: "bg-secondary text-muted-foreground dark:bg-secondary dark:text-secondary-foreground",
    });
  }

  if (tags.length === 0) return null;

  return (
    <div className="flex flex-wrap items-center gap-1.5">
      {tags.map((tag) => (
        <span
          key={tag.label}
          className={cn("rounded-full px-2 py-0.5 text-xs font-medium", tag.tone)}
          title={tag.label}
        >
          {tag.value}
        </span>
      ))}
    </div>
  );
}

function CircuitBadge({ attempt }: { attempt: ProviderChainAttempt }) {
  const state = attempt.circuit_state_after ?? attempt.circuit_state_before;
  // 后端写入大写状态（"OPEN"/"HALF_OPEN"），配色比较必须经归一化。
  const normalized = normalizeCircuitState(state);
  return (
    <>
      <span
        className={cn(
          "rounded-md px-2 py-0.5 text-xs font-bold text-white",
          normalized === "OPEN"
            ? "bg-rose-500"
            : normalized === "HALF_OPEN"
              ? "bg-amber-500"
              : "bg-emerald-500"
        )}
      >
        {state}
      </span>
      {attempt.circuit_failure_count != null && attempt.circuit_failure_threshold != null ? (
        <span className="text-sm text-muted-foreground dark:text-secondary-foreground">
          {attempt.circuit_failure_count}/{attempt.circuit_failure_threshold} 次失败
        </span>
      ) : null}
    </>
  );
}

function hasStructuredDetails(attempt: ProviderChainAttempt): boolean {
  return (
    attempt.error_code != null ||
    attempt.error_category != null ||
    attempt.decision != null ||
    attempt.selection_method != null ||
    attempt.probe_result != null ||
    attempt.probe_trigger != null ||
    attempt.probe_generation != null ||
    attempt.circuit_state_before != null ||
    attempt.circuit_state_after != null
  );
}
