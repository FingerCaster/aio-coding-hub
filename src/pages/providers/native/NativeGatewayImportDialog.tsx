import { useEffect, useRef, useState } from "react";
import { AlertCircle, AlertTriangle, Layers, RefreshCw } from "lucide-react";
import {
  nativeGatewayImportPreview,
  type GatewayImportPreview,
} from "../../../services/nativeGateway";
import { useNativeGatewayImportMutation } from "../../../query/nativeGateway";
import { Button } from "../../../ui/Button";
import { Dialog } from "../../../ui/Dialog";
import { FormField } from "../../../ui/FormField";
import { Input } from "../../../ui/Input";
import { nativeFailureMessage } from "./nativeProviderDraft";
import { cn } from "../../../utils/cn";

export function NativeGatewayImportDialog({
  targetId,
  nativeKey,
  onClose,
  onImported,
}: {
  targetId: string;
  nativeKey: string;
  onClose: () => void;
  onImported: () => void;
}) {
  const [preview, setPreview] = useState<GatewayImportPreview | null>(null);
  const [keys, setKeys] = useState<Record<string, string>>({});
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [refresh, setRefresh] = useState(0);
  const epoch = useRef(0);
  const mutation = useNativeGatewayImportMutation();

  useEffect(() => {
    const current = ++epoch.current;
    void nativeGatewayImportPreview(targetId, nativeKey).then(
      (result) => {
        if (current === epoch.current) {
          setPreview(result);
          setLoading(false);
        }
      },
      (cause: unknown) => {
        if (current === epoch.current) {
          setError(nativeFailureMessage(cause));
          setLoading(false);
        }
      }
    );
    return () => {
      epoch.current += 1;
    };
  }, [targetId, nativeKey, refresh]);

  const ready =
    preview?.canImport &&
    preview.groups.length > 0 &&
    preview.groups.every((group) => Boolean(keys[group.groupId]?.trim()));

  async function confirm() {
    if (!preview || !ready || mutation.isPending) return;
    const current = epoch.current;
    setError(null);
    try {
      await mutation.mutateAsync({
        targetId,
        nativeKey,
        expectedRevision: preview.revision,
        credentials: preview.groups.map((group) => ({
          groupId: group.groupId,
          apiKey: keys[group.groupId].trim(),
        })),
      });
      if (current === epoch.current) onImported();
    } catch (cause) {
      if (current === epoch.current) {
        setError(nativeFailureMessage(cause));
        setPreview(null);
        setKeys({});
      }
    }
  }

  return (
    <Dialog
      open
      title="添加到 AIO 网关：导入预览"
      description="创建独立、默认停用的上游快照。原生刷新不会更新此快照，原生节点和默认模型保持不变。"
      onOpenChange={(open) => {
        if (!open && !mutation.isPending) onClose();
      }}
      className="max-w-3xl"
    >
      <div className="space-y-4">
        <div className="flex items-center gap-2 rounded-lg bg-muted/40 p-2.5 text-xs text-muted-foreground">
          <span className="font-medium text-foreground">原生标识：</span>
          <span className="font-mono">{nativeKey}</span>
        </div>

        {loading ? (
          <p role="status" className="text-xs text-muted-foreground">
            读取并验证协议、模型覆盖和地址…
          </p>
        ) : null}

        {error ? (
          <div
            role="alert"
            className="flex items-center gap-2 rounded-lg border border-destructive/30 bg-destructive/5 p-3 text-xs text-destructive"
          >
            <AlertCircle className="h-4 w-4 shrink-0" aria-hidden="true" />
            <span>{error}</span>
          </div>
        ) : null}

        {preview?.issues.length ? (
          <div
            role="alert"
            className="rounded-xl border border-amber-400/60 bg-amber-50/50 p-3.5 text-xs text-amber-900 dark:border-amber-700/60 dark:bg-amber-950/20 dark:text-amber-300 space-y-2"
          >
            <div className="flex items-center gap-1.5 font-medium">
              <AlertTriangle className="h-4 w-4 shrink-0 text-amber-600 dark:text-amber-400" />
              <span>以下能力无法等价导入；阻止项需先在原生配置修正或在 AIO 手动配置：</span>
            </div>
            <ul className="space-y-1 pl-5 list-disc text-muted-foreground">
              {preview.issues.map((issue, index) => (
                <li key={index}>
                  <span className={issue.blocking ? "font-semibold text-destructive" : ""}>
                    {issue.blocking ? "阻止导入" : "提示"}
                  </span>{" "}
                  · {issue.modelId ?? "供应商"} · {issue.code}
                </li>
              ))}
            </ul>
          </div>
        ) : null}

        <div className="space-y-3">
          {preview?.groups.map((group) => (
            <div
              key={group.groupId}
              className="space-y-3 rounded-xl border border-border/70 bg-surface-panel/30 p-3.5"
            >
              <div className="flex flex-wrap items-center justify-between gap-2 border-b border-border/50 pb-2">
                <h3 className="font-mono text-sm font-semibold text-foreground flex items-center gap-1.5">
                  <Layers className="h-3.5 w-3.5 text-muted-foreground" aria-hidden="true" />
                  {group.protocol}
                </h3>
                <span className="break-all font-mono text-[11px] text-muted-foreground">
                  Base URL：{group.baseUrl}
                </span>
              </div>

              <div className="space-y-1.5">
                <div className="text-[11px] font-medium text-muted-foreground">包含模型：</div>
                <div className="flex flex-wrap gap-1.5">
                  {group.models.map((model) => (
                    <span
                      key={model.requestModelId}
                      className="inline-flex items-center gap-1 rounded-md border border-border/60 bg-muted/30 px-2 py-0.5 font-mono text-[11px] text-foreground"
                      title={`输入: ${model.input.join("/")} · 上下文: ${model.contextWindow} · 输出: ${model.maxTokens}`}
                    >
                      <span>{model.requestModelId}</span>
                      <span className="text-muted-foreground">·</span>
                      <span className="text-[10px] text-muted-foreground">
                        {model.reasoning ? "思考" : "无思考"}
                      </span>
                    </span>
                  ))}
                </div>
              </div>

              <FormField
                label={`AIO API Key · ${group.protocol}`}
                hint="须明确输入字面 API Key；不读取原生登录、环境变量或命令结果"
              >
                {(id) => (
                  <Input
                    id={id}
                    type="password"
                    autoComplete="off"
                    disabled={mutation.isPending}
                    value={keys[group.groupId] ?? ""}
                    onChange={(e) => setKeys({ ...keys, [group.groupId]: e.target.value })}
                    placeholder="输入该上游的 API Key"
                    className="h-9 text-xs"
                  />
                )}
              </FormField>
            </div>
          ))}
        </div>

        {preview && !preview.canImport ? (
          <div
            role="alert"
            className="rounded-lg border border-destructive/30 bg-destructive/5 p-3 text-xs text-destructive"
          >
            当前原生节点无法安全导入，未创建任何网关上游。
          </div>
        ) : null}

        <div className="flex flex-wrap justify-end gap-2 border-t border-border pt-3">
          <Button variant="secondary" disabled={mutation.isPending} onClick={onClose}>
            取消
          </Button>
          <Button
            variant="secondary"
            disabled={mutation.isPending || loading}
            onClick={() => {
              setLoading(true);
              setPreview(null);
              setKeys({});
              setError(null);
              setRefresh((v) => v + 1);
            }}
          >
            <RefreshCw className={cn("h-3.5 w-3.5 mr-1.5", loading && "animate-spin")} />
            重新预览
          </Button>
          <Button
            variant="primary"
            disabled={!ready || loading || mutation.isPending}
            onClick={() => void confirm()}
          >
            {mutation.isPending ? "导入中…" : "确认导入为停用上游"}
          </Button>
        </div>
      </div>
    </Dialog>
  );
}
