import { useState } from "react";
import { AlertCircle, Code2, FileCode } from "lucide-react";
import type { NativeCliKey } from "../../../constants/clis";
import type { NativeProviderEdit, NativeProvidersList } from "../../../services/nativeCli";
import { useNativeCliProviderMutation } from "../../../query/nativeCli";
import { Button } from "../../../ui/Button";
import { Dialog } from "../../../ui/Dialog";
import { FormField } from "../../../ui/FormField";
import { Input } from "../../../ui/Input";
import { Textarea } from "../../../ui/Textarea";
import { NativeModelFields } from "./NativeModelFields";
import {
  isNativeNode,
  nativeFailureMessage,
  nativeNodePatches,
  nativeString,
  parseNativeNode,
  setNativeField,
  type NativeNode,
} from "./nativeProviderDraft";

export function NativeProviderEditor({
  client,
  snapshot,
  edit,
  readOnly = false,
  onClose,
}: {
  client: NativeCliKey;
  snapshot: NativeProvidersList;
  edit: NativeProviderEdit | null;
  readOnly?: boolean;
  onClose: () => void;
}) {
  const original = edit && isNativeNode(edit.node) ? edit.node : {};
  const [text, setText] = useState(() => JSON.stringify(original, null, 2));
  const [nativeKey, setNativeKey] = useState(edit?.provider.nativeKey ?? "");
  const [displayName, setDisplayName] = useState(edit?.provider.displayName ?? "");
  const [error, setError] = useState<string | null>(null);
  const mutation = useNativeCliProviderMutation();

  let node: NativeNode | null = null;
  try {
    node = parseNativeNode(text);
  } catch {
    /* Keep invalid raw draft visible and block save. */
  }

  const revision = edit?.revision ?? snapshot.revision;
  const stale = revision !== snapshot.revision;
  const contentChanged = node !== null && nativeNodePatches(original, node).length > 0;
  const archiveBlocked = edit?.provider.state === "present" && contentChanged;
  const blocked =
    readOnly ||
    !snapshot.target.writable ||
    (snapshot.parseStatus !== "ready" && snapshot.parseStatus !== "missing") ||
    mutation.isPending ||
    !node ||
    !revision ||
    stale ||
    !nativeKey.trim() ||
    Boolean(edit && !isNativeNode(edit.node));

  function update(next: NativeNode) {
    setText(JSON.stringify(next, null, 2));
    setError(null);
  }

  async function save(apply: boolean) {
    if (blocked || (!apply && archiveBlocked) || !node || !revision) return;
    setError(null);
    try {
      await mutation.mutateAsync({
        action: "save",
        input: {
          targetId: snapshot.target.targetId,
          nativeKey: nativeKey.trim(),
          displayName: displayName.trim() || nativeKey.trim(),
          expectedRevision: revision,
          expectedNodeDigest: edit?.provider.nodeDigest ?? null,
          expectedProfileRevision: edit?.provider.profileRevision ?? null,
          node: edit ? null : node,
          patch: edit ? nativeNodePatches(original, node) : [],
          apply,
        },
      });
      onClose();
    } catch (cause) {
      setError(nativeFailureMessage(cause));
    }
  }

  return (
    <Dialog
      open
      onOpenChange={(open) => {
        if (!open && !mutation.isPending) onClose();
      }}
      title={edit ? "编辑原生供应商" : "新增原生供应商"}
      description="保存到档案或明确加入原生 CLI。不会修改默认模型、登录数据或当前会话。"
      className="max-w-4xl"
    >
      <div className="space-y-4">
        <div className="flex items-center gap-2 rounded-lg bg-muted/40 p-2.5 text-xs text-muted-foreground">
          <FileCode className="h-3.5 w-3.5 shrink-0" aria-hidden="true" />
          <span className="break-all font-mono">{snapshot.target.modelsPath}</span>
        </div>

        {stale ? (
          <div
            role="alert"
            className="rounded-lg border border-amber-400/60 bg-amber-50/50 p-3 text-xs text-amber-900 dark:border-amber-700/60 dark:bg-amber-950/20 dark:text-amber-300"
          >
            配置已在编辑期间变化，请关闭并重新读取后编辑。
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

        <div className="grid gap-3 sm:grid-cols-2">
          <FormField label="原生标识" hint="创建后不可改名">
            {(id) => (
              <Input
                id={id}
                value={nativeKey}
                disabled={Boolean(edit) || mutation.isPending}
                onChange={(e) => setNativeKey(e.target.value)}
                placeholder="例如：my-provider"
                className="h-9"
              />
            )}
          </FormField>
          <FormField label="档案显示名称">
            {(id) => (
              <Input
                id={id}
                value={displayName}
                disabled={mutation.isPending}
                onChange={(e) => setDisplayName(e.target.value)}
                placeholder="例如：My Provider"
                className="h-9"
              />
            )}
          </FormField>
        </div>

        {node ? (
          <div className="space-y-4">
            <div className="grid gap-3 sm:grid-cols-3">
              <FormField label="原生 API" hint="允许原生扩展协议；网关范围另验">
                {(id) => (
                  <Input
                    id={id}
                    disabled={mutation.isPending}
                    value={nativeString(node, "api")}
                    onChange={(e) =>
                      update(setNativeField(node!, "api", e.target.value || undefined))
                    }
                    placeholder="例如：anthropic-messages"
                    className="h-9 text-xs"
                  />
                )}
              </FormField>
              <FormField label="原生 Base URL">
                {(id) => (
                  <Input
                    id={id}
                    disabled={mutation.isPending}
                    value={nativeString(node, "baseUrl")}
                    onChange={(e) =>
                      update(setNativeField(node!, "baseUrl", e.target.value || undefined))
                    }
                    placeholder="https://api.example.com"
                    className="h-9 text-xs"
                  />
                )}
              </FormField>
              <FormField label="原生 API Key 或表达式" hint="只存文本，不执行命令">
                {(id) => (
                  <Input
                    id={id}
                    type="password"
                    autoComplete="off"
                    disabled={mutation.isPending}
                    value={nativeString(node, "apiKey")}
                    onChange={(e) =>
                      update(setNativeField(node!, "apiKey", e.target.value || undefined))
                    }
                    placeholder="sk-..."
                    className="h-9 text-xs"
                  />
                )}
              </FormField>
            </div>

            <NativeModelFields
              client={client}
              models={node.models}
              disabled={mutation.isPending}
              onChange={(models) => update({ ...node, models })}
            />
          </div>
        ) : (
          <div
            role="alert"
            className="rounded-lg border border-destructive/30 bg-destructive/5 p-3 text-xs text-destructive"
          >
            原文不是有效 JSON 对象；修复后才可保存或使用结构化编辑。
          </div>
        )}

        <details className="group rounded-xl border border-border/70 bg-surface-panel/30 p-3.5">
          <summary className="cursor-pointer text-xs font-semibold text-muted-foreground hover:text-foreground flex items-center gap-1.5 select-none">
            <Code2 className="h-3.5 w-3.5" aria-hidden="true" />
            <span>原文编辑（包含敏感字段）</span>
          </summary>
          <div className="mt-3">
            <FormField
              label="完整原生节点 JSON"
              hint="请求头、模型覆盖及未知字段保留；只修改你明确编辑的字段"
            >
              {(id) => (
                <Textarea
                  id={id}
                  rows={14}
                  className="font-mono text-xs"
                  spellCheck={false}
                  disabled={mutation.isPending}
                  value={text}
                  onChange={(e) => setText(e.target.value)}
                />
              )}
            </FormField>
          </div>
        </details>

        <div className="flex flex-wrap items-center justify-end gap-2 border-t border-border pt-3">
          <Button variant="secondary" disabled={mutation.isPending} onClick={onClose}>
            取消
          </Button>
          <Button
            variant="secondary"
            disabled={blocked || archiveBlocked}
            title={archiveBlocked ? "已加入的节点内容变更必须明确写回原生配置" : undefined}
            onClick={() => void save(false)}
          >
            仅保存档案
          </Button>
          <Button variant="primary" disabled={blocked} onClick={() => void save(true)}>
            {mutation.isPending ? "保存中…" : `保存并加入 ${client === "pi" ? "Pi" : "OMP"}`}
          </Button>
        </div>
      </div>
    </Dialog>
  );
}
