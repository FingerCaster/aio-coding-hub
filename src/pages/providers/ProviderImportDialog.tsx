import { useEffect, useRef, useState } from "react";
import { ClipboardPaste, FileJson, ShieldAlert } from "lucide-react";
import { toast } from "sonner";
import { useProviderShareImportMutation } from "../../query/providerShare";
import {
  providerShareImportPreviewDiscard,
  providerShareImportPreviewFromContent,
  providerShareImportPreviewFromFile,
  type ProviderShareImportPreview,
} from "../../services/providers/providerShare";
import type { ProviderSummary } from "../../services/providers/providers";
import { Button } from "../../ui/Button";
import { Dialog } from "../../ui/Dialog";
import { Textarea } from "../../ui/Textarea";
import { formatActionFailureToast } from "../../utils/errors";

type ImportMode = "file" | "content";

export type ProviderImportDialogProps = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onImported: (provider: ProviderSummary) => void;
};

const CREDENTIAL_LABELS: Record<ProviderShareImportPreview["credentialStatus"], string> = {
  configured: "API Key 已配置",
  needs_api_key: "需要填写 API Key",
  not_required: "无需独立凭据",
  available: "OAuth 凭据可用",
  refreshable: "OAuth 凭据可刷新",
  needs_login: "需要重新登录 OAuth",
};

const COMPATIBILITY_LABELS: Record<
  ProviderShareImportPreview["extensions"][number]["compatibility"],
  string
> = {
  compatible: "兼容",
  missing_plugin: "缺少插件",
  plugin_unavailable: "插件不可用",
  version_mismatch: "版本不匹配",
  namespace_mismatch: "扩展声明不匹配",
};

function discardPreviewToken(token: string | null) {
  if (!token) return;
  void providerShareImportPreviewDiscard(token).catch(() => undefined);
}

export function ProviderImportDialog({
  open,
  onOpenChange,
  onImported,
}: ProviderImportDialogProps) {
  const [mode, setMode] = useState<ImportMode>("file");
  const [content, setContent] = useState("");
  const [preview, setPreview] = useState<ProviderShareImportPreview | null>(null);
  const [previewLoading, setPreviewLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const previewTokenRef = useRef<string | null>(null);
  const requestVersionRef = useRef(0);
  const previousOpenRef = useRef(open);
  const importMutation = useProviderShareImportMutation();

  useEffect(() => {
    return () => {
      requestVersionRef.current += 1;
      discardPreviewToken(previewTokenRef.current);
      previewTokenRef.current = null;
    };
  }, []);

  useEffect(() => {
    if (previousOpenRef.current && !open) {
      requestVersionRef.current += 1;
      discardPreviewToken(previewTokenRef.current);
      previewTokenRef.current = null;
      setPreview(null);
      setContent("");
      setError(null);
      setPreviewLoading(false);
      setMode("file");
    }
    previousOpenRef.current = open;
  }, [open]);

  function clearPreview() {
    const token = previewTokenRef.current;
    previewTokenRef.current = null;
    setPreview(null);
    discardPreviewToken(token);
  }

  function resetSensitiveState() {
    requestVersionRef.current += 1;
    clearPreview();
    setContent("");
    setError(null);
    setPreviewLoading(false);
    setMode("file");
  }

  function handleOpenChange(nextOpen: boolean) {
    if (!nextOpen && importMutation.isPending) return;
    if (!nextOpen) resetSensitiveState();
    onOpenChange(nextOpen);
  }

  function switchMode(nextMode: ImportMode) {
    if (mode === nextMode || importMutation.isPending) return;
    requestVersionRef.current += 1;
    clearPreview();
    setContent("");
    setError(null);
    setPreviewLoading(false);
    setMode(nextMode);
  }

  function rememberPreview(nextPreview: ProviderShareImportPreview) {
    clearPreview();
    previewTokenRef.current = nextPreview.previewToken;
    setPreview(nextPreview);
  }

  async function previewFile() {
    const requestVersion = ++requestVersionRef.current;
    clearPreview();
    setError(null);
    setPreviewLoading(true);
    try {
      const nextPreview = await providerShareImportPreviewFromFile();
      if (!nextPreview) return;
      if (requestVersionRef.current !== requestVersion) {
        discardPreviewToken(nextPreview.previewToken);
        return;
      }
      rememberPreview(nextPreview);
    } catch (cause) {
      if (requestVersionRef.current === requestVersion) {
        setError(formatActionFailureToast("读取分享文件", cause).toast);
      }
    } finally {
      if (requestVersionRef.current === requestVersion) setPreviewLoading(false);
    }
  }

  async function previewContent() {
    if (!content) {
      setError("请粘贴供应商分享 JSON 内容");
      return;
    }
    const requestVersion = ++requestVersionRef.current;
    clearPreview();
    setError(null);
    setPreviewLoading(true);
    try {
      const nextPreview = await providerShareImportPreviewFromContent(content);
      if (requestVersionRef.current !== requestVersion) {
        discardPreviewToken(nextPreview.previewToken);
        return;
      }
      rememberPreview(nextPreview);
    } catch (cause) {
      if (requestVersionRef.current === requestVersion) {
        setError(formatActionFailureToast("校验分享内容", cause).toast);
      }
    } finally {
      if (requestVersionRef.current === requestVersion) setPreviewLoading(false);
    }
  }

  async function confirmImport() {
    if (!preview || !preview.canImport || importMutation.isPending) return;
    const requestVersion = ++requestVersionRef.current;
    const token = preview.previewToken;
    previewTokenRef.current = null;
    setError(null);
    try {
      const imported = await importMutation.mutateAsync({ previewToken: token });
      if (requestVersionRef.current !== requestVersion) return;
      setPreview(null);
      setContent("");
      toast.success(`已导入供应商：${imported.name}（默认禁用）`);
      onOpenChange(false);
      onImported(imported);
    } catch (cause) {
      discardPreviewToken(token);
      if (requestVersionRef.current !== requestVersion) return;
      setPreview(null);
      setContent("");
      const feedback = formatActionFailureToast("导入供应商", cause);
      setError(`${feedback.toast}。预览已失效，请重新校验。`);
      toast.error(feedback.toast);
    }
  }

  return (
    <Dialog
      open={open}
      onOpenChange={handleOpenChange}
      title="导入供应商"
      description="导入会新增一个默认禁用的供应商，不会覆盖现有配置。"
      className="max-w-2xl"
    >
      <div className="space-y-4">
        <div className="flex items-start gap-3 rounded-md border border-amber-300 bg-amber-50 p-3 text-amber-950 dark:border-amber-700 dark:bg-amber-950/30 dark:text-amber-100">
          <ShieldAlert className="mt-0.5 h-4 w-4 shrink-0" aria-hidden="true" />
          <div className="min-w-0 text-sm leading-6">
            分享内容包含凭据。只导入来自可信来源的文件或内容，预览不会显示 API Key、Token、URL
            或本机路径。
          </div>
        </div>

        <div className="inline-grid h-9 grid-cols-2 rounded-md border border-border bg-muted p-0.5">
          <button
            type="button"
            aria-pressed={mode === "file"}
            onClick={() => switchMode("file")}
            className={`inline-flex min-w-28 items-center justify-center gap-1.5 rounded px-3 text-sm transition-colors ${
              mode === "file" ? "bg-background text-foreground shadow-sm" : "text-muted-foreground"
            }`}
          >
            <FileJson className="h-4 w-4" aria-hidden="true" />
            文件
          </button>
          <button
            type="button"
            aria-pressed={mode === "content"}
            onClick={() => switchMode("content")}
            className={`inline-flex min-w-28 items-center justify-center gap-1.5 rounded px-3 text-sm transition-colors ${
              mode === "content"
                ? "bg-background text-foreground shadow-sm"
                : "text-muted-foreground"
            }`}
          >
            <ClipboardPaste className="h-4 w-4" aria-hidden="true" />
            内容
          </button>
        </div>

        {mode === "file" ? (
          <div className="flex items-center gap-3 rounded-md border border-dashed border-border px-3 py-4">
            <div className="min-w-0 flex-1 text-sm text-muted-foreground">
              选择 `.json` 分享文件后，后端会读取并生成脱敏预览。
            </div>
            <Button
              variant="secondary"
              onClick={() => void previewFile()}
              disabled={previewLoading || importMutation.isPending}
            >
              <FileJson className="h-4 w-4" aria-hidden="true" />
              {previewLoading ? "读取中…" : preview ? "重新选择" : "选择文件"}
            </Button>
          </div>
        ) : (
          <div className="space-y-2">
            <Textarea
              value={content}
              onChange={(event) => {
                requestVersionRef.current += 1;
                clearPreview();
                setError(null);
                setPreviewLoading(false);
                setContent(event.currentTarget.value);
              }}
              rows={8}
              spellCheck={false}
              placeholder="粘贴供应商分享 JSON"
              aria-label="供应商分享 JSON 内容"
              className="min-h-44 resize-y font-mono text-xs"
              disabled={importMutation.isPending}
            />
            <div className="flex justify-end">
              <Button
                variant="secondary"
                onClick={() => void previewContent()}
                disabled={!content || previewLoading || importMutation.isPending}
              >
                {previewLoading ? "校验中…" : preview ? "重新校验" : "校验并预览"}
              </Button>
            </div>
          </div>
        )}

        {error ? (
          <div
            role="alert"
            className="rounded-md border border-destructive/30 bg-destructive/5 p-3 text-sm text-destructive"
          >
            {error}
          </div>
        ) : null}

        {preview ? <ProviderImportPreviewDetails preview={preview} /> : null}

        <div className="flex flex-wrap items-center justify-end gap-2 border-t border-border pt-4">
          <Button
            variant="secondary"
            onClick={() => handleOpenChange(false)}
            disabled={importMutation.isPending}
          >
            取消
          </Button>
          <Button
            variant="primary"
            onClick={() => void confirmImport()}
            disabled={!preview?.canImport || previewLoading || importMutation.isPending}
          >
            {importMutation.isPending ? "导入中…" : "确认导入"}
          </Button>
        </div>
      </div>
    </Dialog>
  );
}

function ProviderImportPreviewDetails({ preview }: { preview: ProviderShareImportPreview }) {
  return (
    <section aria-label="供应商导入预览" className="space-y-3 border-t border-border pt-4">
      <div className="grid gap-x-6 gap-y-2 text-sm sm:grid-cols-2">
        <PreviewRow label="CLI" value={preview.cliKey} />
        <PreviewRow label="认证" value={preview.authMode === "oauth" ? "OAuth" : "API Key"} />
        <PreviewRow label="原名称" value={preview.sourceName} />
        <PreviewRow label="导入名称" value={preview.finalName} />
        <PreviewRow label="来源状态" value={preview.sourceEnabled ? "已启用" : "已禁用"} />
        <PreviewRow label="导入状态" value="禁用" />
        <PreviewRow label="凭据状态" value={CREDENTIAL_LABELS[preview.credentialStatus]} />
        <PreviewRow label="插件扩展" value={`${preview.extensionCount} 项`} />
      </div>

      {preview.extensions.length > 0 ? (
        <div className="space-y-2">
          {preview.extensions.map((extension) => (
            <div
              key={`${extension.pluginId}:${extension.namespace}`}
              className="flex flex-col gap-1 rounded-md border border-border px-3 py-2 text-xs sm:flex-row sm:items-center sm:justify-between"
            >
              <div className="min-w-0">
                <div className="truncate font-medium text-foreground">{extension.pluginId}</div>
                <div className="truncate text-muted-foreground">
                  {extension.namespace} · 需要 {extension.requiredVersion}
                  {extension.installedVersion ? ` · 当前 ${extension.installedVersion}` : ""}
                </div>
              </div>
              <span
                className={
                  extension.compatibility === "compatible"
                    ? "shrink-0 text-emerald-700 dark:text-emerald-400"
                    : "shrink-0 text-destructive"
                }
              >
                {COMPATIBILITY_LABELS[extension.compatibility]}
              </span>
            </div>
          ))}
        </div>
      ) : null}

      {!preview.canImport ? (
        <div className="rounded-md border border-destructive/30 bg-destructive/5 p-3 text-sm text-destructive">
          插件扩展与当前环境不兼容，解决上方问题后重新预览。
        </div>
      ) : null}
    </section>
  );
}

function PreviewRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex min-w-0 items-baseline gap-2">
      <span className="shrink-0 text-xs text-muted-foreground">{label}</span>
      <span className="min-w-0 break-words text-foreground">{value}</span>
    </div>
  );
}
