import { useId, useState } from "react";
import type { AppAboutInfo } from "../../services/app/appAbout";
import { updaterCandidateChannel, type UpdaterCheckUpdate } from "../../services/app/updater";
import type { UpdateChannel } from "../../services/app/updateChannel";
import { Button } from "../../ui/Button";
import { Card } from "../../ui/Card";
import { Dialog } from "../../ui/Dialog";
import { Switch } from "../../ui/Switch";

type SettingsAboutCardProps = {
  about: AppAboutInfo | null;
  checkingUpdate: boolean;
  checkUpdate: () => Promise<void>;
  updateCandidate?: UpdaterCheckUpdate | null;
  updateChannel?: UpdateChannel;
  updateChannelReady?: boolean;
  updateChannelSaving?: boolean;
  updateChannelError?: string | null;
  updateCheckError?: string | null;
  betaParticipationConfirmed?: boolean;
  setUpdateChannel?: (channel: UpdateChannel) => Promise<boolean>;
};

export function SettingsAboutCard({
  about,
  checkingUpdate,
  checkUpdate,
  updateCandidate = null,
  updateChannel = "stable",
  updateChannelReady = true,
  updateChannelSaving = false,
  updateChannelError = null,
  updateCheckError = null,
  betaParticipationConfirmed = false,
  setUpdateChannel = async () => false,
}: SettingsAboutCardProps) {
  const errorId = useId();
  const [riskDialogOpen, setRiskDialogOpen] = useState(false);
  const [submittingChannel, setSubmittingChannel] = useState(false);
  const [localChannelError, setLocalChannelError] = useState<string | null>(null);
  const channelBusy = updateChannelSaving || submittingChannel;
  const channelError = updateChannelError ?? localChannelError;
  const candidateIsCurrent =
    updateCandidate != null && updaterCandidateChannel(updateCandidate) === updateChannel;
  const betaCandidate = candidateIsCurrent && updateChannel === "beta";

  async function requestChannel(channel: UpdateChannel) {
    if (channelBusy) return false;
    setSubmittingChannel(true);
    setLocalChannelError(null);
    try {
      const saved = await setUpdateChannel(channel);
      if (!saved) {
        setLocalChannelError("未能保存更新频道，当前频道未改变。");
      }
      return saved;
    } catch (error) {
      setLocalChannelError(error instanceof Error ? error.message : String(error));
      return false;
    } finally {
      setSubmittingChannel(false);
    }
  }

  function handleParticipationChange(enabled: boolean) {
    if (!enabled) {
      void requestChannel("stable");
      return;
    }
    if (betaParticipationConfirmed) {
      void requestChannel("beta");
      return;
    }
    setRiskDialogOpen(true);
  }

  async function confirmBetaParticipation() {
    const saved = await requestChannel("beta");
    if (saved) setRiskDialogOpen(false);
  }

  return (
    <>
      <Card>
        <div className="mb-4 font-semibold text-foreground">关于应用</div>
        {about ? (
          <div className="grid gap-2 text-sm text-secondary-foreground">
            <div className="flex items-center justify-between gap-4">
              <span className="text-muted-foreground">版本</span>
              <span className="font-mono">{about.app_version}</span>
            </div>
            <div className="flex items-center justify-between gap-4">
              <span className="text-muted-foreground">构建</span>
              <span className="font-mono">{about.profile}</span>
            </div>
            <div className="flex items-center justify-between gap-4">
              <span className="text-muted-foreground">平台</span>
              <span className="font-mono">
                {about.os}/{about.arch}
              </span>
            </div>
            {about.bundle_type ? (
              <div className="flex items-center justify-between gap-4">
                <span className="text-muted-foreground">Bundle</span>
                <span className="font-mono">{about.bundle_type}</span>
              </div>
            ) : null}
            {about.run_mode !== "unknown" ? (
              <div className="flex items-center justify-between gap-4">
                <span className="text-muted-foreground">运行模式</span>
                <span className="font-mono">{about.run_mode}</span>
              </div>
            ) : null}

            <div className="mt-2 border-t border-line-subtle pt-3">
              <div className="flex items-start justify-between gap-4">
                <div className="min-w-0">
                  <label className="text-sm text-foreground" htmlFor="beta-participation-switch">
                    参与 Beta 测试
                  </label>
                  <div className="mt-1 text-xs leading-relaxed text-muted-foreground">
                    获取预发布版本；可能不稳定，且不会自动降级。
                  </div>
                </div>
                <div className="flex shrink-0 items-center gap-2">
                  <span className="text-xs text-muted-foreground">
                    {updateChannelReady
                      ? updateChannel === "beta"
                        ? "Beta 更新"
                        : "稳定更新"
                      : "读取中…"}
                  </span>
                  <Switch
                    id="beta-participation-switch"
                    checked={updateChannel === "beta"}
                    onCheckedChange={handleParticipationChange}
                    disabled={!updateChannelReady || channelBusy}
                    aria-label="参与 Beta 测试"
                    aria-invalid={channelError ? true : undefined}
                    aria-describedby={channelError ? errorId : undefined}
                  />
                </div>
              </div>
              {channelError ? (
                <div id={errorId} role="alert" className="mt-2 text-xs text-rose-600">
                  {channelError}
                </div>
              ) : null}
            </div>

            {candidateIsCurrent ? (
              <div
                className="flex items-center justify-between gap-4"
                aria-label={`${betaCandidate ? "Beta 更新" : "可用更新"} ${updateCandidate?.version ?? "未知版本"}`}
              >
                <span className="text-muted-foreground">
                  {betaCandidate ? "Beta 更新" : "可用更新"}
                </span>
                <span className="font-mono">{updateCandidate?.version ?? "—"}</span>
              </div>
            ) : null}

            <div className="flex items-center justify-between gap-4">
              <span className="text-muted-foreground">
                {about.run_mode === "portable" ? "获取新版本" : "检查更新"}
              </span>
              <Button
                onClick={() => void checkUpdate()}
                variant="secondary"
                size="sm"
                disabled={checkingUpdate || !updateChannelReady}
                aria-label={updateChannel === "beta" ? "检查 Beta 更新" : undefined}
              >
                {checkingUpdate ? "检查中…" : about.run_mode === "portable" ? "打开" : "检查"}
              </Button>
            </div>
            {updateCheckError ? (
              <div role="alert" className="text-xs text-rose-600">
                检查更新失败：{updateCheckError}
              </div>
            ) : null}
          </div>
        ) : (
          <div className="text-sm text-muted-foreground">加载中…</div>
        )}
      </Card>

      <Dialog
        open={riskDialogOpen}
        onOpenChange={(open) => {
          if (!channelBusy) setRiskDialogOpen(open);
        }}
        title="参与 Beta 测试"
        description="Beta 更新属于预发布版本，可能不稳定。"
        className="max-w-lg"
      >
        <div className="space-y-4">
          <div className="text-sm leading-relaxed text-secondary-foreground">
            问题版本不会自动降级。退出 Beta 后只会恢复稳定频道检查，不会自动降级已经安装的版本。
          </div>
          {channelError ? (
            <div role="alert" className="text-sm text-rose-600">
              {channelError}
            </div>
          ) : null}
          <div className="flex justify-end gap-2 border-t border-line-subtle pt-3">
            <Button
              type="button"
              variant="secondary"
              onClick={() => setRiskDialogOpen(false)}
              disabled={channelBusy}
            >
              取消
            </Button>
            <Button
              type="button"
              variant="warning"
              onClick={() => void confirmBetaParticipation()}
              disabled={channelBusy}
            >
              {channelBusy ? "保存中…" : "确认参与 Beta 测试"}
            </Button>
          </div>
        </div>
      </Dialog>
    </>
  );
}
