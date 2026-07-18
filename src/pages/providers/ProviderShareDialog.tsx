import { useEffect, useState } from "react";
import { Copy, Download, ShieldAlert } from "lucide-react";
import { toast } from "sonner";
import {
  providerShareCopyToClipboard,
  providerShareSaveToFile,
} from "../../services/providers/providerShare";
import type { ProviderSummary } from "../../services/providers/providers";
import { Button } from "../../ui/Button";
import { Dialog } from "../../ui/Dialog";
import { formatActionFailureToast } from "../../utils/errors";

export type ProviderShareDialogProps = {
  open: boolean;
  provider: ProviderSummary | null;
  onOpenChange: (open: boolean) => void;
};

export function ProviderShareDialog({ open, provider, onOpenChange }: ProviderShareDialogProps) {
  const [pendingAction, setPendingAction] = useState<"copy" | "save" | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (open) setError(null);
  }, [open, provider?.id]);

  function handleOpenChange(nextOpen: boolean) {
    if (!nextOpen && pendingAction) return;
    if (!nextOpen) setError(null);
    onOpenChange(nextOpen);
  }

  async function runAction(action: "copy" | "save") {
    if (!provider || pendingAction) return;
    setPendingAction(action);
    setError(null);
    try {
      if (action === "copy") {
        await providerShareCopyToClipboard(provider.id);
        toast.success("供应商分享内容已复制，60 秒后仅在内容未变化时自动清空");
      } else {
        const saved = await providerShareSaveToFile(provider.id);
        if (!saved) return;
        toast.success("供应商分享文件已保存");
      }
      onOpenChange(false);
    } catch (cause) {
      const feedback = formatActionFailureToast(action === "copy" ? "复制" : "保存", cause);
      setError(feedback.toast);
      toast.error(feedback.toast);
    } finally {
      setPendingAction(null);
    }
  }

  return (
    <Dialog
      open={open}
      onOpenChange={handleOpenChange}
      title="分享供应商"
      description={provider ? provider.name : undefined}
      className="max-w-xl"
    >
      <div className="space-y-4">
        <div className="flex items-start gap-3 rounded-md border border-amber-300 bg-amber-50 p-3 text-amber-950 dark:border-amber-700 dark:bg-amber-950/30 dark:text-amber-100">
          <ShieldAlert className="mt-0.5 h-4 w-4 shrink-0" aria-hidden="true" />
          <div className="min-w-0 text-sm leading-6">
            分享内容包含完整 API Key、OAuth Token 和 Client
            Secret。请只发送给可信接收方，并妥善保管导出的文件。
          </div>
        </div>

        {provider?.source_provider_id != null ? (
          <div className="rounded-md border border-border bg-muted px-3 py-2 text-sm text-muted-foreground">
            该转译供应商引用了另一个供应商，无法作为独立配置分享。
          </div>
        ) : null}

        {error ? (
          <div
            role="alert"
            className="rounded-md border border-destructive/30 bg-destructive/5 p-3 text-sm text-destructive"
          >
            {error}
          </div>
        ) : null}

        <div className="flex flex-wrap items-center justify-end gap-2">
          <Button
            variant="secondary"
            onClick={() => handleOpenChange(false)}
            disabled={pendingAction != null}
          >
            取消
          </Button>
          <Button
            variant="secondary"
            onClick={() => void runAction("copy")}
            disabled={!provider || provider.source_provider_id != null || pendingAction != null}
          >
            <Copy className="h-4 w-4" aria-hidden="true" />
            {pendingAction === "copy" ? "复制中…" : "复制内容"}
          </Button>
          <Button
            variant="primary"
            onClick={() => void runAction("save")}
            disabled={!provider || provider.source_provider_id != null || pendingAction != null}
          >
            <Download className="h-4 w-4" aria-hidden="true" />
            {pendingAction === "save" ? "保存中…" : "保存到本地"}
          </Button>
        </div>
      </div>
    </Dialog>
  );
}
