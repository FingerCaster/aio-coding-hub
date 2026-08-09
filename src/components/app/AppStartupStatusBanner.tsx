import { useState } from "react";
import { useNavigate } from "react-router-dom";
import { toast } from "sonner";
import { retryAppStartupStatusSnapshot, useAppStartupStatus } from "../../app/startupStatusStore";
import { appExit } from "../../services/app/dataManagement";
import { logToConsole } from "../../services/consoleLog";
import type { AppStartupStage } from "../../services/app/startupStatus";
import { Button } from "../../ui/Button";

function startupStageLabel(stage: AppStartupStage | null): string {
  switch (stage) {
    case "resetting_data":
      return "数据重置";
    case "initializing_db":
      return "数据库初始化";
    case "reading_settings":
      return "设置加载";
    case "starting_gateway":
      return "网关启动";
    case "syncing_cli_proxy":
      return "CLI 代理同步";
    case "finalizing_wsl":
      return "WSL 启动收尾";
    default:
      return "应用启动";
  }
}

export function AppStartupStatusBanner() {
  const navigate = useNavigate();
  const status = useAppStartupStatus();
  const [retrying, setRetrying] = useState(false);

  if (!status || (!status.maintenanceMode && status.currentStage !== "failed")) {
    return null;
  }

  const failedStageLabel = startupStageLabel(status.failedStage);
  const maintenanceRunning = status.maintenanceMode && status.running;
  const detail = maintenanceRunning
    ? "正在完成上次未结束的数据清理"
    : (status.errorMessage ?? `${failedStageLabel}失败`);

  async function handleRetry() {
    if (!status.canRetry || retrying) {
      return;
    }

    setRetrying(true);
    try {
      await retryAppStartupStatusSnapshot();
    } catch (error) {
      logToConsole("error", "重试启动任务失败", {
        error: String(error),
        failed_stage: status.failedStage,
      });
      toast("重试启动失败：请查看 Console 日志");
    } finally {
      setRetrying(false);
    }
  }

  async function handleExit() {
    try {
      await appExit();
    } catch (error) {
      logToConsole("error", "退出维护模式失败", { error: String(error) });
      toast("退出应用失败");
    }
  }

  return (
    <div
      role="alert"
      className="mb-4 rounded-2xl border border-amber-200 bg-amber-50 px-4 py-3 text-sm text-amber-900 dark:border-amber-700 dark:bg-amber-900/30 dark:text-amber-200"
    >
      <div className="flex flex-col gap-3 lg:flex-row lg:items-start lg:justify-between">
        <div className="min-w-0">
          <div className="font-semibold">
            {status.maintenanceMode
              ? maintenanceRunning
                ? "正在执行数据重置维护"
                : "数据重置维护未完成，普通功能已停用"
              : "启动没有完成，当前功能处于降级状态"}
          </div>
          <div className="mt-1 break-words text-amber-800 dark:text-amber-300">
            {maintenanceRunning ? detail : `${failedStageLabel}失败：${detail}`}
          </div>
        </div>

        <div className="flex shrink-0 items-center gap-2">
          <Button
            size="sm"
            variant="secondary"
            onClick={handleRetry}
            disabled={!status.canRetry || retrying}
          >
            {retrying ? "重试中..." : status.maintenanceMode ? "重试数据清理" : "重试启动"}
          </Button>
          {status.maintenanceMode ? (
            <Button size="sm" variant="ghost" onClick={handleExit}>
              退出应用
            </Button>
          ) : (
            <Button size="sm" variant="ghost" onClick={() => navigate("/settings")}>
              打开设置
            </Button>
          )}
        </div>
      </div>
    </div>
  );
}
