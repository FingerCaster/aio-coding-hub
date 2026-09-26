import { useState } from "react";
import { AlertCircle, AlertTriangle, Info, Plus, RefreshCw } from "lucide-react";
import { isNativeCliKey } from "../../../constants/clis";
import type { ProviderSummary } from "../../../services/providers/providers";
import {
  useNativeGatewayModelsQuery,
  useNativeGatewayModelsMutation,
} from "../../../query/nativeGateway";
import { Button } from "../../../ui/Button";
import { Dialog } from "../../../ui/Dialog";
import { FormField } from "../../../ui/FormField";
import { Textarea } from "../../../ui/Textarea";
import { parseGatewayModels } from "./nativeGatewayModelDraft";
import { nativeFailureMessage } from "./nativeProviderDraft";
import { cn } from "../../../utils/cn";

export function NativeGatewayModelsDialog({
  provider,
  onClose,
}: {
  provider: ProviderSummary;
  onClose: () => void;
}) {
  const query = useNativeGatewayModelsQuery(provider.id, provider.provider_uuid);
  const mutation = useNativeGatewayModelsMutation(provider.id, provider.provider_uuid);
  const [draft, setDraft] = useState<{ raw: string; revision: string } | null>(null);
  const [error, setError] = useState<string | null>(null);

  const raw = draft?.raw ?? JSON.stringify(query.data?.models ?? [], null, 2);
  const stale = Boolean(draft && draft.revision !== query.data?.revision);

  function change(raw: string) {
    if (query.data) setDraft({ raw, revision: draft?.revision ?? query.data.revision });
  }

  const blocked =
    query.isPending || query.isError || query.data == null || mutation.isPending || stale;

  async function save() {
    if (blocked || !isNativeCliKey(provider.cli_key)) return;
    let models;
    try {
      models = parseGatewayModels(raw, provider.cli_key);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "模型声明无效。");
      return;
    }
    try {
      await mutation.mutateAsync({
        models,
        expectedRevision: draft?.revision ?? query.data!.revision,
      });
      onClose();
    } catch (cause) {
      setError(nativeFailureMessage(cause));
    }
  }

  return (
    <Dialog
      open
      title="AIO 网关模型声明"
      description={`${provider.name} · ${provider.gateway_protocol ?? "协议未配置"}。只发布明确声明的能力；保存后须在 AIO 入口预览并应用。`}
      onOpenChange={(open) => {
        if (!open && !mutation.isPending) onClose();
      }}
      className="max-w-3xl"
    >
      <div className="space-y-4">
        {/* 指南卡片 */}
        <div className="rounded-xl border border-border/70 bg-surface-panel/40 p-3 space-y-1.5 text-xs text-muted-foreground">
          <div className="flex items-center gap-1.5 font-medium text-foreground">
            <Info className="h-3.5 w-3.5 text-muted-foreground" aria-hidden="true" />
            <span>能力声明规范</span>
          </div>
          <p>
            requestModelId 是请求中的公开模型
            ID，实际模型映射沿用供应商模型路由。displayName、input、contextWindow、maxTokens、reasoning
            和 supportsTools 均需明确声明；不根据模型名称补齐能力。
          </p>
          <p>
            {provider.cli_key === "pi"
              ? "Pi 思考格式：thinking 的 client 为 pi，levelMap 明确包含七个档位，未支持值用 null。"
              : "OMP 思考格式：thinking 的 client 为 omp，包含 mode、efforts、defaultLevel、effortMap、supportsDisplay、requiresEffort。"}{" "}
            无思考时 reasoning=false、thinking=null。
          </p>
          <p>
            supportsTools=null 仅保存待确认档案，不参与入口发布。发布前必须明确工具能力；Pi 需填
            true，OMP 可填 true 或 false。
          </p>
        </div>

        {stale ? (
          <div
            role="alert"
            className="flex items-center gap-2 rounded-lg border border-amber-400/60 bg-amber-50/50 p-3 text-xs text-amber-900 dark:border-amber-700/60 dark:bg-amber-950/20 dark:text-amber-300"
          >
            <AlertTriangle
              className="h-4 w-4 shrink-0 text-amber-600 dark:text-amber-400"
              aria-hidden="true"
            />
            <span>模型声明已在编辑期间变化，请重新读取后核对。</span>
          </div>
        ) : null}

        {query.data?.stale ? (
          <div
            role="alert"
            className="flex items-center gap-2 rounded-lg border border-amber-400/60 bg-amber-50/50 p-3 text-xs text-amber-900 dark:border-amber-700/60 dark:bg-amber-950/20 dark:text-amber-300"
          >
            <AlertTriangle
              className="h-4 w-4 shrink-0 text-amber-600 dark:text-amber-400"
              aria-hidden="true"
            />
            <span>
              上游协议、地址或模型路由已改变。现有声明已停止参与发布，请核对后保存重新绑定。
            </span>
          </div>
        ) : null}

        {query.isError ? (
          <div
            role="alert"
            className="flex items-center gap-2 rounded-lg border border-destructive/30 bg-destructive/5 p-3 text-xs text-destructive"
          >
            <AlertCircle className="h-4 w-4 shrink-0" aria-hidden="true" />
            <span>读取模型声明失败；已禁用保存，不能用空数组覆盖。</span>
          </div>
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

        <FormField label="模型能力声明 JSON" hint="允许空数组明确撤销所有模型声明">
          {(id) => (
            <Textarea
              id={id}
              rows={16}
              spellCheck={false}
              className="font-mono text-xs rounded-xl"
              disabled={blocked}
              value={raw}
              onChange={(e) => {
                change(e.target.value);
                setError(null);
              }}
            />
          )}
        </FormField>

        <div className="flex flex-wrap items-center justify-between gap-2 border-t border-border pt-3">
          <div className="flex flex-wrap items-center gap-2">
            <Button
              variant="secondary"
              size="sm"
              className="h-8 text-xs gap-1.5"
              disabled={blocked}
              onClick={() => {
                try {
                  const current: unknown = JSON.parse(raw);
                  if (!Array.isArray(current)) return;
                  change(
                    JSON.stringify(
                      [
                        ...current,
                        {
                          requestModelId: "",
                          displayName: "",
                          input: [],
                          contextWindow: null,
                          maxTokens: null,
                          reasoning: false,
                          thinking: null,
                          supportsTools: null,
                        },
                      ],
                      null,
                      2
                    )
                  );
                } catch {
                  setError("请先修复现有 JSON。");
                }
              }}
            >
              <Plus className="h-3.5 w-3.5" aria-hidden="true" />
              添加待填写模型
            </Button>
            <Button
              variant="secondary"
              size="sm"
              className="h-8 text-xs gap-1.5"
              disabled={query.isFetching || mutation.isPending}
              onClick={() => {
                setDraft(null);
                setError(null);
                void query.refetch();
              }}
            >
              <RefreshCw
                className={cn("h-3.5 w-3.5", query.isFetching && "animate-spin")}
                aria-hidden="true"
              />
              重新读取
            </Button>
          </div>

          <div className="flex items-center gap-2">
            <Button variant="secondary" disabled={mutation.isPending} onClick={onClose}>
              取消
            </Button>
            <Button variant="primary" disabled={blocked} onClick={() => void save()}>
              {mutation.isPending ? "保存中…" : "保存模型声明"}
            </Button>
          </div>
        </div>
      </div>
    </Dialog>
  );
}
