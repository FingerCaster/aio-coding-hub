import { AlertTriangle, Copy, Lightbulb } from "lucide-react";
import { toast } from "sonner";
import { Card } from "../../ui/Card";
import {
  GatewayErrorDescriptions,
  getGatewayErrorShortLabel,
} from "../../constants/gatewayErrorCodes";
import type { RequestLogErrorObservation } from "./requestLogErrorDetails";
import { DisclosureSection } from "./DisclosureSection";
import { formatCircuitRecovery } from "../../utils/formatters";
import { copyText } from "../../services/clipboard";
import type { StreamInternalErrorEvidence } from "../../services/gateway/attemptsJson";
import { Tooltip } from "../../ui/Tooltip";

export type RequestLogErrorObservationCardProps = {
  observation: RequestLogErrorObservation | null;
};

export function RequestLogErrorObservationCard({
  observation,
}: RequestLogErrorObservationCardProps) {
  if (!observation) return null;

  const shortLabel = observation.displayErrorCode
    ? getGatewayErrorShortLabel(observation.displayErrorCode)
    : null;
  const desc = observation.gwDescription?.desc ?? null;
  const suggestion = observation.gwDescription?.suggestion ?? null;
  const fallbackTitle = resolveFallbackTitle(observation);

  const failureSummary =
    observation.attemptFailureSummary && observation.attemptFailureSummary.length > 0
      ? observation.attemptFailureSummary
      : null;
  const dominantGroup = failureSummary?.[0] ?? null;
  const dominantSuggestion =
    dominantGroup && dominantGroup.errorCode !== observation.displayErrorCode
      ? (GatewayErrorDescriptions[dominantGroup.errorCode as keyof typeof GatewayErrorDescriptions]
          ?.suggestion ?? null)
      : null;

  const detailFields = buildDetailFields(observation);
  const hasDetails =
    detailFields.length > 0 ||
    observation.upstreamBodyPreview != null ||
    observation.rawDetailsText != null;

  return (
    <Card padding="sm">
      <div className="space-y-2.5">
        {/* Header: error code badge + description */}
        <div className="flex flex-wrap items-start gap-2">
          <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0 text-rose-500 dark:text-rose-400" />
          <div className="min-w-0 flex-1">
            <div className="flex flex-wrap items-center gap-2">
              {observation.displayErrorCode ? (
                <span className="rounded-full bg-rose-50 px-2 py-0.5 text-xs font-semibold text-rose-700 dark:bg-rose-900/30 dark:text-rose-300">
                  {shortLabel !== observation.displayErrorCode
                    ? `${shortLabel} (${observation.displayErrorCode})`
                    : observation.displayErrorCode}
                </span>
              ) : null}
              {desc ? <span className="text-sm font-medium text-foreground">{desc}</span> : null}
            </div>

            {/* Reason text (if no desc available, show reason as primary text) */}
            {!desc && observation.reason ? (
              <p className="mt-1 text-sm text-secondary-foreground">{observation.reason}</p>
            ) : null}
            {!desc && !observation.reason && fallbackTitle ? (
              <p className="text-sm font-medium text-foreground">{fallbackTitle}</p>
            ) : null}
          </div>
        </div>

        {/* Suggestion */}
        {suggestion ? (
          <div className="flex items-start gap-2 rounded-lg bg-amber-50/60 px-3 py-2 dark:bg-amber-900/15">
            <Lightbulb className="mt-0.5 h-3.5 w-3.5 shrink-0 text-amber-600 dark:text-amber-400" />
            <p className="text-xs text-amber-800 dark:text-amber-300">{suggestion}</p>
          </div>
        ) : null}

        {/* Failure attempt summary (grouped by structured error_code) */}
        {failureSummary ? (
          <div className="space-y-1">
            <div className="text-xs font-medium text-muted-foreground">失败尝试</div>
            {failureSummary.map((group) => (
              <div key={group.errorCode} className="text-xs text-secondary-foreground">
                {getGatewayErrorShortLabel(group.errorCode)} ×{group.count}（
                {group.providerNames.join("、")}
                {group.timeoutSecs != null ? `，${group.timeoutSecs} 秒` : ""}）
                {group.circuitTriggerErrorCode
                  ? ` 触发：${getGatewayErrorShortLabel(group.circuitTriggerErrorCode)}`
                  : ""}
                {group.circuitRecoverAtUnix != null
                  ? `，${formatCircuitRecovery(group.circuitRecoverAtUnix)}`
                  : ""}
              </div>
            ))}
          </div>
        ) : null}

        {/* Dominant failure-code suggestion when it differs from the displayed terminal code */}
        {dominantSuggestion ? (
          <div className="flex items-start gap-2 rounded-lg bg-amber-50/60 px-3 py-2 dark:bg-amber-900/15">
            <Lightbulb className="mt-0.5 h-3.5 w-3.5 shrink-0 text-amber-600 dark:text-amber-400" />
            <p className="text-xs text-amber-800 dark:text-amber-300">{dominantSuggestion}</p>
          </div>
        ) : null}

        {observation.streamInternalError ? (
          <StreamInternalErrorBlock evidence={observation.streamInternalError} />
        ) : null}

        {/* Expandable detail fields */}
        {hasDetails ? (
          <DisclosureSection label="详细信息">
            <div className="space-y-2">
              {detailFields.map(({ label, value }) => (
                <DetailRow key={label} label={label} value={value} />
              ))}

              {observation.upstreamBodyPreview ? (
                <div>
                  <div className="mb-1 text-xs font-medium text-muted-foreground">上游响应预览</div>
                  <pre className="max-h-40 overflow-auto whitespace-pre-wrap break-all rounded-md bg-secondary px-2.5 py-2 text-xs font-mono text-secondary-foreground dark:bg-secondary dark:text-secondary-foreground">
                    {observation.upstreamBodyPreview.length > 500
                      ? `${observation.upstreamBodyPreview.slice(0, 500)}…`
                      : observation.upstreamBodyPreview}
                  </pre>
                </div>
              ) : null}

              {observation.rawDetailsText && !observation.upstreamBodyPreview ? (
                <div>
                  <div className="mb-1 text-xs font-medium text-muted-foreground">原始错误信息</div>
                  <pre className="max-h-40 overflow-auto whitespace-pre-wrap break-all rounded-md bg-secondary px-2.5 py-2 text-xs font-mono text-secondary-foreground dark:bg-secondary dark:text-secondary-foreground">
                    {observation.rawDetailsText.length > 500
                      ? `${observation.rawDetailsText.slice(0, 500)}…`
                      : observation.rawDetailsText}
                  </pre>
                </div>
              ) : null}
            </div>
          </DisclosureSection>
        ) : null}
      </div>
    </Card>
  );
}

function DetailRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex items-baseline gap-2 text-xs">
      <span className="shrink-0 text-muted-foreground">{label}:</span>
      <span className="font-mono text-secondary-foreground">{value}</span>
    </div>
  );
}

function StreamInternalErrorBlock({ evidence }: { evidence: StreamInternalErrorEvidence }) {
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
        <div className="text-xs font-semibold text-amber-800 dark:text-amber-300">流内部错误</div>
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
      <div className="grid gap-x-4 gap-y-1 text-xs sm:grid-cols-2">
        <DetailRow label="事件" value={evidence.event_type} />
        <DetailRow label="分类" value={evidence.classification} />
        {evidence.error_type ? <DetailRow label="错误类型" value={evidence.error_type} /> : null}
        {evidence.error_code ? <DetailRow label="错误码" value={evidence.error_code} /> : null}
        {evidence.matched_keyword ? (
          <DetailRow label="匹配关键词" value={evidence.matched_keyword} />
        ) : null}
        <DetailRow label="处理" value={evidence.disposition} />
        {evidence.truncated ? <DetailRow label="截断" value="是" /> : null}
      </div>
    </div>
  );
}

type DetailField = { label: string; value: string };

function resolveFallbackTitle(obs: RequestLogErrorObservation): string | null {
  if (obs.upstreamStatus != null) {
    return `HTTP ${obs.upstreamStatus} 响应异常`;
  }
  if (
    obs.errorCategory ||
    obs.streamInternalError ||
    obs.decision ||
    obs.selectionMethod ||
    obs.reasonCode ||
    obs.matchedRule ||
    obs.outcome ||
    obs.rawDetailsText
  ) {
    return "请求异常详情";
  }
  return null;
}

function buildDetailFields(obs: RequestLogErrorObservation): DetailField[] {
  const fields: DetailField[] = [];
  if (obs.errorCategory) fields.push({ label: "错误分类", value: obs.errorCategory });
  if (obs.upstreamStatus != null)
    fields.push({ label: "上游状态码", value: String(obs.upstreamStatus) });
  if (obs.decision) fields.push({ label: "决策", value: obs.decision });
  if (obs.selectionMethod) fields.push({ label: "选择方式", value: obs.selectionMethod });
  if (obs.reasonCode) fields.push({ label: "原因码", value: obs.reasonCode });
  if (obs.matchedRule) fields.push({ label: "匹配规则", value: obs.matchedRule });
  if (obs.reason && obs.gwDescription?.desc) {
    // Only show reason in detail if desc already shown as primary text
    fields.push({ label: "原因", value: obs.reason });
  }
  if (obs.circuitStateBefore || obs.circuitStateAfter) {
    const parts: string[] = [];
    if (obs.circuitStateBefore) parts.push(`${obs.circuitStateBefore}`);
    if (obs.circuitStateAfter && obs.circuitStateAfter !== obs.circuitStateBefore) {
      parts.push(`→ ${obs.circuitStateAfter}`);
    }
    if (obs.circuitFailureCount != null && obs.circuitFailureThreshold != null) {
      parts.push(`(${obs.circuitFailureCount}/${obs.circuitFailureThreshold})`);
    }
    fields.push({ label: "熔断器", value: parts.join(" ") });
  }
  if (obs.providerId != null) {
    const providerText = obs.providerName
      ? `${obs.providerName} (id=${obs.providerId})`
      : `id=${obs.providerId}`;
    fields.push({ label: "供应商", value: providerText });
  }
  return fields;
}
