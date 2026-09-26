import { useEffect, useRef, useState } from "react";
import { useMutation } from "@tanstack/react-query";
import { Bot, RefreshCw, Save, Settings2 } from "lucide-react";
import { toast } from "sonner";
import { ompSettingsSave, type OmpSettingsSnapshot } from "../../../services/ompSettings";
import { confirmDesktopDialog } from "../../../services/desktop/confirm";
import { useOmpSettingsQuery } from "../../../query/ompSettings";
import { Button } from "../../../ui/Button";
import { Card } from "../../../ui/Card";
import { Input } from "../../../ui/Input";
import { OmpModelPicker } from "./OmpModelPicker";
import { ompRoleModelPreview } from "./ompModelPreview";
import { OmpSettingsFields } from "./OmpSettingsFields";
import { OmpAgentSettings } from "./OmpAgentSettings";
import { OmpAgentEditor } from "./OmpAgentEditor";
import {
  record,
  ROLE_LABELS,
  selectorText,
  settingsPatches,
  validateDraft,
  type SettingsValues,
} from "./ompSettingsForm";

export function OmpSettingsPanel({
  targetId,
  onDirtyChange,
}: {
  targetId: string;
  onDirtyChange?: (dirty: boolean) => void;
}) {
  const query = useOmpSettingsQuery(targetId);
  if (!query.data)
    return (
      <Card className="space-y-3" aria-label="OMP 原生设置">
        <h3 className="flex items-center gap-2 text-sm font-semibold">
          <Settings2 className="h-4 w-4" />
          OMP 原生设置
        </h3>
        {query.isPending ? (
          <p role="status" className="text-sm text-muted-foreground">
            正在读取 OMP 配置…
          </p>
        ) : (
          <>
            <p role="alert" className="text-sm text-destructive">
              {query.error instanceof Error
                ? query.error.message
                : "无法读取原生设置，请检查目标文件。"}
            </p>
            <Button size="sm" variant="secondary" onClick={() => void query.refetch()}>
              重新读取设置
            </Button>
          </>
        )}
      </Card>
    );
  return (
    <OmpSettingsForm
      key={targetId}
      snapshot={query.data}
      refreshing={query.isFetching}
      onDirtyChange={onDirtyChange}
      reload={async () => {
        const result = await query.refetch();
        if (result.isError || !result.data) throw result.error ?? new Error("重新读取设置失败");
        return result.data;
      }}
    />
  );
}

function OmpSettingsForm({
  snapshot,
  refreshing,
  reload,
  onDirtyChange,
}: {
  snapshot: OmpSettingsSnapshot;
  refreshing: boolean;
  reload: () => Promise<OmpSettingsSnapshot>;
  onDirtyChange?: (dirty: boolean) => void;
}) {
  const [baseline, setBaseline] = useState(snapshot);
  const [values, setValues] = useState<SettingsValues>(snapshot.values);
  const [tab, setTab] = useState("models");
  const [newRole, setNewRole] = useState("");
  const [addedRoles, setAddedRoles] = useState<string[]>([]);
  const [editor, setEditor] = useState<{ fileName: string | null } | null>(null);
  const [error, setError] = useState("");
  const [backup, setBackup] = useState<string | null>(null);
  const [reloading, setReloading] = useState(false);
  const saving = useRef(false);
  const mounted = useRef(true);
  const mutation = useMutation({
    mutationFn: (input: Parameters<typeof ompSettingsSave>[0]) => ompSettingsSave(input),
    retry: false,
  });
  const patches = settingsPatches(baseline.values, values);
  const dirty = patches.length > 0;
  const validation = validateDraft(values, baseline);
  const disabled = !baseline.writable || !snapshot.writable || mutation.isPending || reloading;
  const roles = [
    ...new Set([
      ...Object.keys(ROLE_LABELS),
      ...Object.keys(record(values.modelRoles)),
      ...addedRoles,
    ]),
  ];
  useEffect(() => {
    onDirtyChange?.(dirty);
  }, [dirty, onDirtyChange]);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      onDirtyChange?.(false);
    };
  }, [onDirtyChange]);
  function change(key: string, value: unknown) {
    setValues((previous) => {
      const next = { ...previous };
      if (value === undefined) delete next[key];
      else next[key] = value as SettingsValues[string];
      return next;
    });
  }
  function changeRecord(key: string, member: string, value: unknown) {
    setValues((previous) => {
      const map = { ...record(previous[key]) };
      if (value === undefined) delete map[member];
      else map[member] = value;
      return { ...previous, [key]: map as SettingsValues[string] };
    });
  }
  async function refresh() {
    if (saving.current || reloading) return;
    if (dirty && !(await confirmDesktopDialog("重新读取会放弃当前尚未保存的 OMP 设置，是否继续？")))
      return;
    if (!mounted.current) return;
    setReloading(true);
    setError("");
    try {
      const latest = await reload();
      if (mounted.current) {
        setBaseline(latest);
        setValues(latest.values);
      }
    } catch (e) {
      if (mounted.current) setError(e instanceof Error ? e.message : "读取失败");
    } finally {
      if (mounted.current) setReloading(false);
    }
  }
  async function save() {
    if (saving.current || disabled || !dirty || validation) return;
    saving.current = true;
    setError("");
    try {
      const result = await mutation.mutateAsync({
        targetId: baseline.targetId,
        expectedRevision: baseline.revision,
        patches,
      });
      if (!result || !mounted.current) return;
      setBaseline((previous) => ({ ...previous, revision: result.revision, values }));
      setBackup(result.backupPath);
      toast.success(result.changed ? "OMP 设置已保存" : "配置没有变化");
      // Metadata can refresh, but a later external revision must not silently replace the draft.
      void reload().catch(() => {});
    } catch (e) {
      if (mounted.current) setError(e instanceof Error ? e.message : "保存失败");
    } finally {
      saving.current = false;
    }
  }
  return (
    <Card padding="none" aria-label="OMP 原生设置">
      <div className="space-y-3 border-b border-border px-5 py-4 sm:px-6">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <h3 className="flex items-center gap-2 text-sm font-semibold">
            <Settings2 className="h-4 w-4 text-muted-foreground" />
            OMP 原生设置
            {dirty && <span className="text-xs font-normal text-amber-600">未保存</span>}
          </h3>
          <Button
            size="sm"
            variant="ghost"
            disabled={mutation.isPending || reloading || refreshing}
            onClick={() => void refresh()}
          >
            <RefreshCw className="h-3.5 w-3.5" />
            重新读取设置
          </Button>
        </div>
        <p className="truncate font-mono text-xs text-muted-foreground" title={baseline.configPath}>
          {baseline.configPath}
        </p>
        <p className="text-xs leading-relaxed text-muted-foreground">
          编辑当前全局 / Profile
          配置。项目设置、启动参数、环境变量和运行中会话可能覆盖这些值；默认模型用于后续新会话。AIO
          入口须先在供应商页导入，模型列表不代表已登录或上游可用。
        </p>
        {snapshot.warnings.map((warning, index) => (
          <p key={index} role="status" className="text-xs text-amber-600">
            {warning}
          </p>
        ))}
        <div
          role="tablist"
          aria-label="OMP 设置分类"
          className="flex gap-1 rounded-lg bg-secondary/60 p-1"
        >
          {[
            ["models", "模型与会话"],
            ["tasks", "子任务行为"],
            ["agents", "Agent 配置"],
          ].map(([key, label]) => (
            <Button
              key={key}
              role="tab"
              aria-selected={tab === key}
              variant={tab === key ? "secondary" : "ghost"}
              size="sm"
              className="flex-1"
              onClick={() => setTab(key)}
            >
              {key === "agents" && <Bot className="h-3.5 w-3.5" />}
              {label}
            </Button>
          ))}
        </div>
      </div>
      <div className="space-y-5 p-5 sm:p-6">
        {tab === "models" && (
          <>
            <OmpModelPicker
              label="默认模型"
              models={snapshot.models}
              value={selectorText(record(values.modelRoles).default)}
              preview={ompRoleModelPreview("default", values, snapshot)}
              inheritance={ompRoleModelPreview("default", values, snapshot, true)}
              disabled={disabled}
              onChange={(value) => changeRecord("modelRoles", "default", value || undefined)}
            />
            <details className="rounded-xl border border-border">
              <summary className="cursor-pointer px-4 py-3 text-sm font-medium">
                其他模型角色与自定义角色
              </summary>
              <div className="space-y-5 border-t border-border p-4">
                <p className="text-xs text-muted-foreground">
                  角色可被 Agent 通过
                  @角色名引用。图像、语音等角色需对应能力的模型；不在目录中的模型可手动填写。
                </p>
                <div className="grid min-w-0 gap-5 xl:grid-cols-2">
                  {roles
                    .filter((role) => role !== "default")
                    .map((role) => (
                      <OmpModelPicker
                        key={role}
                        label={(ROLE_LABELS[role] ?? role) + " · " + role}
                        models={snapshot.models}
                        value={selectorText(record(values.modelRoles)[role])}
                        preview={ompRoleModelPreview(role, values, snapshot)}
                        inheritance={ompRoleModelPreview(role, values, snapshot, true)}
                        disabled={disabled}
                        onChange={(value) => changeRecord("modelRoles", role, value || undefined)}
                      />
                    ))}
                </div>
                <div className="flex gap-2">
                  <Input
                    aria-label="自定义角色名称"
                    placeholder="例如 review、fast_worker"
                    value={newRole}
                    disabled={disabled}
                    onChange={(e) => setNewRole(e.target.value)}
                  />
                  <Button
                    variant="secondary"
                    className="shrink-0 whitespace-nowrap"
                    disabled={
                      disabled ||
                      !/^[a-zA-Z0-9][a-zA-Z0-9_.-]{0,63}$/.test(newRole) ||
                      ["__proto__", "constructor", "prototype"].includes(newRole)
                    }
                    onClick={() => {
                      setAddedRoles((previous) => [...new Set([...previous, newRole])]);
                      setNewRole("");
                    }}
                  >
                    添加角色
                  </Button>
                </div>
              </div>
            </details>
            <OmpSettingsFields
              fields={baseline.fields.filter((f) => f.group === "session")}
              values={values}
              disabled={disabled}
              onChange={change}
            />
          </>
        )}
        {tab === "tasks" && (
          <OmpSettingsFields
            fields={baseline.fields.filter((f) => f.group === "tasks")}
            values={values}
            disabled={disabled}
            onChange={change}
          />
        )}
        {tab === "agents" && (
          <OmpAgentSettings
            snapshot={snapshot}
            values={values}
            roles={roles}
            disabled={disabled}
            onRecordChange={changeRecord}
            onChange={change}
            onEdit={(fileName) => setEditor({ fileName })}
          />
        )}
        {(error || validation) && (
          <p role="alert" className="whitespace-pre-wrap text-sm text-destructive">
            {error || validation}
          </p>
        )}
      </div>
      <div className="flex flex-wrap items-center justify-between gap-3 border-t border-border px-5 py-4 sm:px-6">
        <div className="min-w-0 flex-1 text-xs text-muted-foreground">
          <p>
            {dirty ? patches.length + " 项待保存变更" : "没有待保存变更"}
            ；仅保存改动字段。原文将备份，YAML 排版可能规范化。
          </p>
          {backup && (
            <p className="mt-1 truncate" title={backup}>
              上次备份：{backup}
            </p>
          )}
        </div>
        <Button disabled={disabled || !dirty || Boolean(validation)} onClick={() => void save()}>
          <Save className="h-3.5 w-3.5" />
          {mutation.isPending ? "保存中…" : "保存 OMP 设置"}
        </Button>
      </div>
      {editor && (
        <OmpAgentEditor
          key={editor.fileName ?? "new"}
          targetId={baseline.targetId}
          fileName={editor.fileName}
          onClose={() => setEditor(null)}
          onSaved={() => {
            void reload().catch(() => {});
          }}
        />
      )}
    </Card>
  );
}
