import { useEffect, useRef, useState } from "react";
import { Check, FolderTree, ShieldCheck, SlidersHorizontal } from "lucide-react";
import type {
  NativeClient,
  NativeTarget,
  NativeTargetSelection,
} from "../../../services/nativeCli";
import {
  useNativeCliTargetSelectMutation,
  useNativeCliTargetValidateMutation,
} from "../../../query/nativeCli";
import { Button } from "../../../ui/Button";
import { Card } from "../../../ui/Card";
import { FormField } from "../../../ui/FormField";
import { Input } from "../../../ui/Input";
import { Select } from "../../../ui/Select";
import { nativeFailureMessage } from "./nativeProviderDraft";

export function NativeTargetPicker({
  client,
  targets,
  targetId,
  onSelect,
}: {
  client: NativeClient;
  targets: NativeTarget[];
  targetId: string | null;
  onSelect: (target: NativeTarget) => void;
}) {
  const [selection, setSelection] = useState<NativeTargetSelection>({
    client,
    mode: "default",
    agentDir: null,
    profile: null,
  });
  const [preview, setPreview] = useState<NativeTarget | null>(null);
  const [error, setError] = useState<string | null>(null);
  const validate = useNativeCliTargetValidateMutation();
  const select = useNativeCliTargetSelectMutation();
  const epoch = useRef(0);

  useEffect(
    () => () => {
      epoch.current += 1;
    },
    []
  );

  function change(next: NativeTargetSelection) {
    epoch.current += 1;
    setSelection(next);
    setPreview(null);
    setError(null);
  }

  async function run(confirm: boolean) {
    const current = ++epoch.current;
    setError(null);
    try {
      const target = await (confirm
        ? select.mutateAsync(selection)
        : validate.mutateAsync(selection));
      if (current !== epoch.current || target.client !== client) return;
      if (confirm) {
        onSelect(target);
        setPreview(null);
      } else setPreview(target);
    } catch (cause) {
      if (current === epoch.current) setError(nativeFailureMessage(cause));
    }
  }

  const pending = select.isPending || validate.isPending;
  const target = targets.find((row) => row.targetId === targetId);

  return (
    <Card padding="sm" className="shrink-0 space-y-2 rounded-xl border border-border">
      <div className="flex flex-wrap items-center gap-3">
        <h2 className="flex shrink-0 items-center gap-2 text-sm font-semibold text-foreground">
          <FolderTree className="h-4 w-4 text-muted-foreground" aria-hidden="true" />
          原生配置目标
        </h2>
        <Select
          aria-label="当前原生配置目标"
          title={target?.modelsPath}
          disabled={select.isPending}
          value={targetId ?? ""}
          className="h-9 min-w-0 flex-1 basis-60"
          onChange={(e) => {
            const next = targets.find((t) => t.targetId === e.target.value);
            if (next) {
              epoch.current += 1;
              setPreview(null);
              setError(null);
              onSelect(next);
            }
          }}
        >
          <option value="" disabled>
            请选择有效目标
          </option>
          {targets.map((t) => (
            <option key={t.targetId} value={t.targetId}>
              {t.profile
                ? "Profile " + t.profile
                : t.source === "default"
                  ? "默认目录"
                  : "自定义目录"}{" "}
              · {t.modelsPath}
            </option>
          ))}
        </Select>
        {target ? (
          <div className="flex shrink-0 items-center gap-2 text-xs">
            <span className="rounded-md bg-secondary px-2 py-1 font-mono uppercase text-muted-foreground">
              {target.format}
            </span>
            {!target.writable ? (
              <span className="text-amber-700 dark:text-amber-400">只读目标</span>
            ) : null}
          </div>
        ) : null}
      </div>
      <details className="group border-t border-border/60 pt-2">
        <summary className="flex cursor-pointer select-none items-center gap-1.5 text-xs text-muted-foreground hover:text-foreground">
          <SlidersHorizontal className="h-3.5 w-3.5" aria-hidden="true" />
          目录详情与目标设置
        </summary>
        {target ? (
          <div className="mt-3 space-y-1 rounded-lg bg-muted/40 p-3 text-xs text-muted-foreground">
            <p className="break-all font-mono text-foreground">{target.modelsPath}</p>
            <p>
              来源：{target.source} · 环境：{target.environment} ·{" "}
              {target.writable ? "目标允许写入，文档状态需另行确认" : "只读目标"}
            </p>
            {target.shadowedFiles.map((path) => (
              <p key={path} className="break-all text-amber-700 dark:text-amber-400">
                被优先级遮蔽：{path}
              </p>
            ))}
          </div>
        ) : null}
        <div className="mt-3 grid gap-3 sm:grid-cols-2">
          <FormField label="目标来源">
            {(id) => (
              <Select
                id={id}
                value={selection.mode}
                disabled={select.isPending}
                onChange={(e) =>
                  change({
                    client,
                    mode:
                      e.target.value === "custom"
                        ? "custom"
                        : e.target.value === "profile"
                          ? "profile"
                          : "default",
                    agentDir: null,
                    profile: null,
                  })
                }
                className="h-9"
              >
                <option value="default">默认目录</option>
                <option value="custom">自定义 agent 目录</option>
                {client === "omp" ? <option value="profile">OMP Profile</option> : null}
              </Select>
            )}
          </FormField>
          {selection.mode === "custom" ? (
            <FormField label="Agent 绝对目录">
              {(id) => (
                <Input
                  id={id}
                  disabled={select.isPending}
                  value={selection.agentDir ?? ""}
                  onChange={(e) => change({ ...selection, agentDir: e.target.value })}
                  placeholder="例如：/Users/name/.pi/agent"
                  className="h-9"
                />
              )}
            </FormField>
          ) : null}
          {selection.mode === "profile" ? (
            <FormField label="OMP Profile 名称">
              {(id) => (
                <Input
                  id={id}
                  disabled={select.isPending}
                  value={selection.profile ?? ""}
                  onChange={(e) => change({ ...selection, profile: e.target.value })}
                  placeholder="例如：work"
                  className="h-9"
                />
              )}
            </FormField>
          ) : null}
        </div>
        <div className="mt-3 flex flex-wrap items-center gap-2">
          <Button variant="secondary" size="sm" disabled={pending} onClick={() => void run(false)}>
            <ShieldCheck className="h-3.5 w-3.5 mr-1.5" aria-hidden="true" />
            验证目标
          </Button>
          <Button
            size="sm"
            variant="primary"
            disabled={pending || !preview}
            onClick={() => void run(true)}
          >
            <Check className="h-3.5 w-3.5 mr-1.5" aria-hidden="true" />
            使用此目标
          </Button>
        </div>
        {preview ? (
          <p className="mt-2.5 break-all text-xs font-mono text-emerald-700 dark:text-emerald-400">
            已验证：{preview.modelsPath}（{preview.writable ? "可写目标" : "只读目标"}）
          </p>
        ) : null}
      </details>

      {error ? (
        <p role="alert" className="text-xs text-destructive">
          {error}
        </p>
      ) : null}
    </Card>
  );
}
