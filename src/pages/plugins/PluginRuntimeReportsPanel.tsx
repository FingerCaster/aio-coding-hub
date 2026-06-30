// Usage: Render plugin runtime reports, failures, audit traces, and replay export actions.

import { Copy, Download } from "lucide-react";
import { toast } from "sonner";
import {
  usePluginExportReplayFixtureMutation,
  usePluginExtensionRuntimeReportsQuery,
} from "../../query/plugins";
import { copyText } from "../../services/clipboard";
import type {
  JsonValue,
  PluginDetail,
  PluginExtensionExecutionReport,
} from "../../services/plugins";
import { Button } from "../../ui/Button";
import { formatActionFailureToast } from "../../utils/errors";

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="space-y-2">
      <h2 className="text-sm font-semibold text-foreground">{title}</h2>
      {children}
    </section>
  );
}

function jsonRecord(value: JsonValue): Record<string, JsonValue> | null {
  if (value && typeof value === "object" && !Array.isArray(value)) {
    return value as Record<string, JsonValue>;
  }
  return null;
}

function detailValue(details: JsonValue, key: string) {
  const value = jsonRecord(details)?.[key];
  if (typeof value !== "string") return null;
  const trimmed = value.trim();
  return trimmed ? trimmed : null;
}

function TraceIdButton({ traceId }: { traceId: string | null | undefined }) {
  if (!traceId) return <span className="font-mono text-xs text-muted-foreground">-</span>;
  const value = traceId;

  async function handleCopy() {
    try {
      await copyText(value);
      toast.success("Trace ID 已复制");
    } catch (error) {
      toast.error(formatActionFailureToast("复制 Trace ID", error).toast);
    }
  }

  return (
    <Button
      size="sm"
      variant="ghost"
      className="h-auto min-h-0 px-1.5 py-0.5 font-mono text-xs"
      onClick={handleCopy}
      title="复制 Trace ID"
    >
      <Copy className="h-3 w-3" />
      {value}
    </Button>
  );
}

function reportFailureLabel(report: PluginExtensionExecutionReport) {
  return report.failureKind ?? report.errorCode ?? "-";
}

export function PluginRuntimeReportsPanel({ detail }: { detail: PluginDetail }) {
  const reportsQuery = usePluginExtensionRuntimeReportsQuery({
    pluginId: detail.summary.plugin_id,
    limit: 8,
  });
  const replayExportMutation = usePluginExportReplayFixtureMutation();
  const reports = reportsQuery.data?.slice(0, 8) ?? [];
  const failures = detail.runtime_failures.slice(0, 5);
  const auditLogs = detail.audit_logs.slice(0, 8);
  const empty =
    reports.length === 0 &&
    failures.length === 0 &&
    auditLogs.length === 0 &&
    !reportsQuery.isLoading;

  async function handleExportReplayFixture(report: PluginExtensionExecutionReport) {
    if (!report.traceId || report.contributionType !== "hook") return;
    try {
      const fixture = await replayExportMutation.mutateAsync({
        traceId: report.traceId,
        hookName: report.contributionId,
        pluginId: report.pluginId,
      });
      await copyText(JSON.stringify(fixture, null, 2));
      toast.success("Replay fixture 已复制");
    } catch (error) {
      toast.error(formatActionFailureToast("导出 replay fixture", error).toast);
    }
  }

  return (
    <Section title="运行观测">
      {empty ? (
        <div className="rounded-md border border-dashed border-border px-3 py-4 text-sm text-muted-foreground">
          还没有记录到插件运行事件
        </div>
      ) : (
        <div className="grid gap-2">
          {reportsQuery.isLoading ? (
            <div className="rounded-md border border-border px-3 py-3 text-sm text-muted-foreground">
              正在读取插件运行报告
            </div>
          ) : null}

          {reports.map((report) => (
            <div key={`report-${report.id}`} className="rounded-md border border-border px-3 py-2">
              <div className="flex flex-wrap items-start justify-between gap-2 text-sm">
                <span className="font-medium text-foreground">{report.status}</span>
                <span className="rounded-md border border-border px-2 py-0.5 text-xs text-muted-foreground">
                  {report.contributionType}
                </span>
              </div>
              <div className="mt-2 grid gap-2 text-xs text-muted-foreground sm:grid-cols-4">
                <div>
                  <div>{report.contributionType === "command" ? "命令" : "Hook"}</div>
                  <div className="break-words font-mono text-foreground">
                    {report.commandOrHook ?? report.contributionId}
                  </div>
                </div>
                <div>
                  <div>耗时</div>
                  <div className="break-words font-mono text-foreground">{report.durationMs}ms</div>
                </div>
                <div>
                  <div>Failure</div>
                  <div className="break-words font-mono text-foreground">
                    {reportFailureLabel(report)}
                  </div>
                </div>
                <div>
                  <div>Trace ID</div>
                  <TraceIdButton traceId={report.traceId} />
                </div>
              </div>
              {report.contributionType === "hook" ? (
                <div className="mt-2 flex flex-wrap items-center gap-2">
                  <Button
                    size="sm"
                    variant="secondary"
                    disabled={!report.traceId || replayExportMutation.isPending}
                    onClick={() => void handleExportReplayFixture(report)}
                    title="导出 replay fixture"
                  >
                    <Download className="h-3.5 w-3.5" />
                    导出 Replay
                  </Button>
                </div>
              ) : null}
            </div>
          ))}

          {failures.map((failure) => (
            <div
              key={`failure-${failure.id}`}
              className="rounded-md border border-border px-3 py-2"
            >
              <div className="flex flex-wrap items-start justify-between gap-2 text-sm">
                <span className="font-medium text-foreground">{failure.message}</span>
                <span className="rounded-md bg-destructive/10 px-2 py-0.5 text-xs font-semibold text-destructive">
                  {failure.failure_kind}
                </span>
              </div>
              <div className="mt-2 grid gap-2 text-xs text-muted-foreground sm:grid-cols-2">
                <div>
                  <div>Hook</div>
                  <div className="break-words font-mono text-foreground">
                    {failure.hook_name ?? "-"}
                  </div>
                </div>
                <div>
                  <div>Trace ID</div>
                  <TraceIdButton traceId={failure.trace_id} />
                </div>
              </div>
            </div>
          ))}

          {auditLogs.map((log) => {
            const hookName = detailValue(log.details, "hookName");
            const failureKind = detailValue(log.details, "failureKind");

            return (
              <div key={`audit-${log.id}`} className="rounded-md border border-border px-3 py-2">
                <div className="flex flex-wrap items-start justify-between gap-2 text-sm">
                  <span className="font-medium text-foreground">{log.message}</span>
                  <span className="rounded-md border border-border px-2 py-0.5 text-xs text-muted-foreground">
                    {log.risk_level}
                  </span>
                </div>
                <div className="mt-1 break-words font-mono text-xs text-muted-foreground">
                  {log.event_type}
                </div>
                <div className="mt-2 grid gap-2 text-xs text-muted-foreground sm:grid-cols-3">
                  <div>
                    <div>Hook</div>
                    <div className="break-words font-mono text-foreground">{hookName ?? "-"}</div>
                  </div>
                  <div>
                    <div>Failure</div>
                    <div className="break-words font-mono text-foreground">
                      {failureKind ?? "-"}
                    </div>
                  </div>
                  <div>
                    <div>Trace ID</div>
                    <TraceIdButton traceId={log.trace_id} />
                  </div>
                </div>
              </div>
            );
          })}
        </div>
      )}
    </Section>
  );
}
