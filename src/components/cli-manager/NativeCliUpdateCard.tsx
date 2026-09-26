import { useEffect, useRef, useState } from "react";
import { useIsMutating, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Download, RefreshCw } from "lucide-react";
import { toast } from "sonner";
import type { NativeCliKey } from "../../constants/clis";
import { nativeCliCheckLatestVersion, nativeCliUpdate } from "../../services/cli/nativeCliUpdate";
import { confirmDesktopDialog } from "../../services/desktop/confirm";
import { redactDiagnosticText } from "../../services/diagnosticRedaction";
import { Button } from "../../ui/Button";
import { Card } from "../../ui/Card";

export function NativeCliUpdateCard({ client }: { client: NativeCliKey }) {
  const queryClient = useQueryClient();
  const mounted = useRef(false);
  const confirming = useRef(false);
  const [confirmationOpen, setConfirmationOpen] = useState(false);
  const [operationError, setOperationError] = useState<string | null>(null);
  const name = client === "pi" ? "Pi" : "OMP";
  const activeInstalls = useIsMutating({ mutationKey: ["native-cli-update"] });
  const mutation = useMutation({
    mutationKey: ["native-cli-update", client],
    retry: false,
    mutationFn: async (planId: string) => {
      const result = await nativeCliUpdate(client, planId);
      if (!result.success) throw new Error(result.error ?? "安装或升级失败");
      return result;
    },
    onSuccess: () => toast.success(name + " 安装／升级完成，请重新打开 CLI 会话"),
    onError: (error) => {
      const message = redactDiagnosticText(error.message, 4096);
      toast.error(message);
      if (mounted.current) setOperationError(message);
    },
    onSettled: async () => {
      await queryClient.invalidateQueries({ queryKey: ["cli-manager", "native-info", client] });
      await queryClient.invalidateQueries({ queryKey: ["native-cli-version", client] });
    },
  });
  const busy = confirmationOpen || activeInstalls > 0;
  const check = useQuery({
    queryKey: ["native-cli-version", client],
    queryFn: () => nativeCliCheckLatestVersion(client),
    enabled: !busy,
    staleTime: 5 * 60 * 1000,
    refetchInterval: 60 * 60 * 1000,
    refetchIntervalInBackground: false,
    retry: false,
  });
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  async function install() {
    const preview = check.data;
    if (!preview?.planId || busy || confirming.current || check.isFetching || check.isError) return;
    confirming.current = true;
    setConfirmationOpen(true);
    setOperationError(null);
    try {
      const accepted = await confirmDesktopDialog(
        "确认" +
          (preview.installed ? "升级" : "下载安装") +
          " " +
          name +
          " 到 v" +
          preview.latestVersion +
          "？\n安装方式：" +
          preview.installMethod +
          "\n目录：" +
          (preview.installDirectory ?? "未知") +
          "\n确认后才会下载并安装。现有配置保持不变，请先关闭正在运行的 CLI。"
      );
      if (accepted && mounted.current) mutation.mutate(preview.planId);
    } catch (error) {
      if (mounted.current)
        setOperationError(error instanceof Error ? error.message : "无法打开确认窗口");
    } finally {
      confirming.current = false;
      if (mounted.current) setConfirmationOpen(false);
    }
  }
  const result = check.data;
  const stateText = !result
    ? null
    : !result.installed
      ? "尚未安装"
      : !result.installedVersion
        ? "当前版本未知，无法比较"
        : result.updateAvailable
          ? "发现新版本"
          : result.installedVersion === result.latestVersion
            ? "已是最新稳定版"
            : "当前版本不需要升级";

  return (
    <Card className="space-y-4" aria-label={name + " 版本管理"}>
      <div className="flex flex-wrap items-center justify-between gap-3">
        <h3 className="flex items-center gap-2 text-sm font-semibold">
          <Download className="h-4 w-4 text-muted-foreground" />
          版本管理
        </h3>
        <Button
          size="sm"
          variant="secondary"
          disabled={busy || check.isFetching}
          onClick={() => void check.refetch()}
        >
          <RefreshCw className={"h-3.5 w-3.5 " + (check.isFetching ? "animate-spin" : "")} />
          {check.isFetching ? "检查中…" : "检查更新"}
        </Button>
      </div>
      <p className="text-xs leading-relaxed text-muted-foreground">
        进入页面及每小时自动检查新版本，只提示更新；点击并确认后才下载安装或升级。
      </p>
      {check.isError ? (
        <p role="alert" className="text-sm text-destructive">
          检查失败：{redactDiagnosticText(check.error.message, 4096)}。可点击检查更新重试。
        </p>
      ) : null}
      {result ? (
        <>
          <div className="flex flex-wrap items-center gap-x-6 gap-y-2 text-sm">
            <span>
              当前：
              <strong>
                {result.installedVersion
                  ? "v" + result.installedVersion
                  : result.installed
                    ? "未知"
                    : "未安装"}
              </strong>
            </span>
            <span>
              最新稳定版：<strong>v{result.latestVersion}</strong>
            </span>
            <span
              className={
                result.updateAvailable
                  ? "text-amber-600 dark:text-amber-400"
                  : "text-muted-foreground"
              }
            >
              {stateText}
            </span>
          </div>
          <p className="break-all text-xs text-muted-foreground">
            {result.installMethod}
            {result.installDirectory ? " · " + result.installDirectory : ""}
          </p>
          {result.blockedReason ? (
            <p className="text-xs text-amber-600 dark:text-amber-400">{result.blockedReason}</p>
          ) : null}
          {result.planId ? (
            <Button
              size="sm"
              disabled={busy || check.isFetching || check.isError}
              onClick={() => void install()}
            >
              <Download className="h-3.5 w-3.5" />
              {mutation.isPending
                ? "正在下载并安装…"
                : confirmationOpen
                  ? "等待确认…"
                  : result.installed
                    ? "一键升级"
                    : "下载安装"}
            </Button>
          ) : null}
        </>
      ) : null}
      {activeInstalls > 0 ? (
        <p role="status" className="text-xs text-muted-foreground">
          正在安装或升级 CLI，请等待完成。下载较大文件可能需要数分钟。
        </p>
      ) : null}
      {operationError ? (
        <p role="alert" className="whitespace-pre-wrap break-words text-xs text-destructive">
          {operationError}
        </p>
      ) : null}
      {mutation.data?.output ? (
        <details className="text-xs text-muted-foreground">
          <summary className="cursor-pointer">安装结果</summary>
          <pre className="mt-2 max-h-40 overflow-auto whitespace-pre-wrap break-words">
            {mutation.data.output}
          </pre>
        </details>
      ) : null}
    </Card>
  );
}
