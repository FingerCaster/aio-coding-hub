import { useEffect, useMemo, useState } from "react";
import { ChevronDown, Pencil, Plus, ShieldAlert, Trash2 } from "lucide-react";
import { toast } from "sonner";
import { CLI_REGISTRY } from "../../constants/clis";
import {
  cloneUpstreamErrorResponseRules,
  createUpstreamErrorResponseRule,
  MAX_UPSTREAM_ERROR_RESPONSE_RULES,
  type UpstreamErrorResponseRule,
  validateUpstreamErrorResponseRules,
} from "../../services/gateway/upstreamErrorResponseRules";
import { providersList, type ProviderSummary } from "../../services/providers/providers";
import type { AppSettings } from "../../services/settings/settings";
import { Button } from "../../ui/Button";
import { ConfirmDialog } from "../../ui/ConfirmDialog";
import { Dialog } from "../../ui/Dialog";
import { Input } from "../../ui/Input";
import { Select } from "../../ui/Select";
import { Switch } from "../../ui/Switch";
import { Textarea } from "../../ui/Textarea";
import { Tooltip } from "../../ui/Tooltip";
import { cn } from "../../utils/cn";

type Props = {
  rules: readonly UpstreamErrorResponseRule[];
  disabled: boolean;
  onPersist: (rules: UpstreamErrorResponseRule[]) => Promise<AppSettings | null>;
};

type EditorState = {
  rule: UpstreamErrorResponseRule;
  statusCodesText: string;
  keywordsText: string;
};

type ProviderLoadState = "idle" | "loading" | "ready" | "error";

function createEditorState(rule: UpstreamErrorResponseRule): EditorState {
  return {
    rule: cloneUpstreamErrorResponseRules([rule])[0] ?? rule,
    statusCodesText: rule.status_codes.join(", "),
    keywordsText: rule.keywords.join("\n"),
  };
}

function parseStatusCodes(value: string): number[] | null {
  const parts = value
    .split(/[\s,，]+/u)
    .map((part) => part.trim())
    .filter(Boolean);
  if (parts.some((part) => !/^\d+$/u.test(part))) return null;
  return Array.from(new Set(parts.map(Number)));
}

function parseKeywords(value: string): string[] {
  return Array.from(
    new Set(
      value
        .split(/\r?\n/u)
        .map((keyword) => keyword.trim().toLowerCase())
        .filter(Boolean)
    )
  );
}

function toggleValue<T>(values: readonly T[], value: T): T[] {
  return values.includes(value) ? values.filter((item) => item !== value) : [...values, value];
}

function cliLabel(cliKey: string): string {
  return CLI_REGISTRY.find((cli) => cli.key === cliKey)?.name ?? cliKey;
}

function ruleMatchSummary(rule: UpstreamErrorResponseRule): string {
  const parts: string[] = [];
  if (rule.status_codes.length > 0) parts.push(rule.status_codes.join("/"));
  if (rule.keywords.length > 0) parts.push(`${rule.keywords.length} 个关键词`);
  return parts.join(rule.match_mode === "all" ? " 且 " : " 或 ");
}

function ruleScopeSummary(rule: UpstreamErrorResponseRule, providers: ProviderSummary[]): string {
  const cliScope = rule.cli_keys.length === 0 ? "全部 CLI" : rule.cli_keys.map(cliLabel).join("/");
  if (rule.provider_ids.length === 0) return `${cliScope} · 全部供应商`;
  const providerNames = new Map(providers.map((provider) => [provider.id, provider.name]));
  const names = rule.provider_ids.map(
    (id) => providerNames.get(id) ?? `已删除供应商 #${String(id)}`
  );
  return `${cliScope} · ${names.length <= 2 ? names.join("/") : `${names.length} 个供应商`}`;
}

function ruleActionSummary(rule: UpstreamErrorResponseRule): string {
  const status =
    rule.status_behavior.mode === "override"
      ? `状态 ${String(rule.status_behavior.status_code)}`
      : "状态透传";
  const message = rule.message_behavior.mode === "override" ? "自定义信息" : "信息透传";
  return `${status} · ${message}`;
}

export function UpstreamErrorResponseRulesCard({ rules, disabled, onPersist }: Props) {
  const [open, setOpen] = useState(false);
  const [editor, setEditor] = useState<EditorState | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<UpstreamErrorResponseRule | null>(null);
  const [saving, setSaving] = useState(false);
  const [providers, setProviders] = useState<ProviderSummary[]>([]);
  const [providerLoadState, setProviderLoadState] = useState<ProviderLoadState>("idle");
  const stableRules = useMemo(() => cloneUpstreamErrorResponseRules(rules), [rules]);
  const sortedRules = useMemo(
    () =>
      stableRules
        .map((rule, index) => ({ rule, index }))
        .sort((left, right) => {
          const priority = left.rule.priority - right.rule.priority;
          return priority === 0 ? left.index - right.index : priority;
        }),
    [stableRules]
  );
  const busy = disabled || saving;

  useEffect(() => {
    if (!editor || providerLoadState !== "idle") return;
    let active = true;
    setProviderLoadState("loading");
    void Promise.all(CLI_REGISTRY.map((cli) => providersList(cli.key)))
      .then((groups) => {
        if (!active) return;
        setProviders(groups.flat());
        setProviderLoadState("ready");
      })
      .catch(() => {
        if (active) setProviderLoadState("error");
      });
    return () => {
      active = false;
    };
  }, [editor, providerLoadState]);

  function openCreateEditor() {
    if (busy || stableRules.length >= MAX_UPSTREAM_ERROR_RESPONSE_RULES) return;
    if (providerLoadState === "error") setProviderLoadState("idle");
    setEditor(createEditorState(createUpstreamErrorResponseRule(stableRules)));
  }

  function openEditEditor(rule: UpstreamErrorResponseRule) {
    if (providerLoadState === "error") setProviderLoadState("idle");
    setEditor(createEditorState(rule));
  }

  async function persistRules(nextRules: UpstreamErrorResponseRule[], message: string) {
    const validationMessage = validateUpstreamErrorResponseRules(nextRules);
    if (validationMessage) {
      toast(validationMessage);
      return false;
    }
    setSaving(true);
    try {
      const updated = await onPersist(nextRules);
      if (!updated) return false;
      toast.success(message);
      return true;
    } catch (error) {
      toast.error(String(error));
      return false;
    } finally {
      setSaving(false);
    }
  }

  async function saveEditor() {
    if (!editor || busy) return;
    const statusCodes = parseStatusCodes(editor.statusCodesText);
    if (!statusCodes) {
      toast("匹配状态码只能包含数字，并以逗号或空格分隔");
      return;
    }
    const normalizedRule: UpstreamErrorResponseRule = {
      ...editor.rule,
      name: editor.rule.name.trim(),
      description: editor.rule.description.trim(),
      status_codes: statusCodes,
      keywords: parseKeywords(editor.keywordsText),
      cli_keys: Array.from(new Set(editor.rule.cli_keys)),
      provider_ids: Array.from(new Set(editor.rule.provider_ids)),
      message_behavior:
        editor.rule.message_behavior.mode === "override"
          ? { mode: "override", message: editor.rule.message_behavior.message.trim() }
          : { mode: "passthrough" },
    };
    const existingIndex = stableRules.findIndex((rule) => rule.id === normalizedRule.id);
    const nextRules = [...stableRules];
    if (existingIndex >= 0) nextRules[existingIndex] = normalizedRule;
    else nextRules.push(normalizedRule);
    if (await persistRules(nextRules, existingIndex >= 0 ? "规则已更新" : "规则已创建")) {
      setEditor(null);
    }
  }

  async function toggleRule(rule: UpstreamErrorResponseRule, enabled: boolean) {
    const nextRules = stableRules.map((item) =>
      item.id === rule.id ? { ...item, enabled } : item
    );
    await persistRules(nextRules, enabled ? "规则已启用" : "规则已停用");
  }

  async function deleteRule() {
    if (!deleteTarget || busy) return;
    const nextRules = stableRules.filter((rule) => rule.id !== deleteTarget.id);
    if (await persistRules(nextRules, "规则已删除")) setDeleteTarget(null);
  }

  return (
    <>
      <div className="overflow-hidden rounded-lg border border-border bg-white dark:bg-secondary">
        <div className="flex items-center gap-3 px-6 py-5">
          <button
            type="button"
            className="flex min-w-0 flex-1 items-center gap-4 text-left"
            aria-expanded={open}
            onClick={() => setOpen((value) => !value)}
          >
            <div className="flex h-12 w-12 shrink-0 items-center justify-center rounded-full bg-rose-500">
              <ShieldAlert className="h-5 w-5 text-white" />
            </div>
            <div className="min-w-0 flex-1">
              <div className="flex flex-wrap items-center gap-2">
                <span className="text-base font-semibold text-foreground">上游错误响应规则</span>
                <span className="rounded-full bg-secondary px-2 py-0.5 text-xs text-muted-foreground">
                  {stableRules.length}
                </span>
              </div>
              <div className="mt-1 text-sm text-muted-foreground">
                匹配最终上游错误，并调整返回客户端的状态码或错误信息。
              </div>
            </div>
          </button>
          <Tooltip content="新建规则">
            <Button
              size="icon"
              variant="secondary"
              aria-label="新建上游错误响应规则"
              disabled={busy || stableRules.length >= MAX_UPSTREAM_ERROR_RESPONSE_RULES}
              onClick={openCreateEditor}
            >
              <Plus className="h-4 w-4" />
            </Button>
          </Tooltip>
          <button
            type="button"
            aria-label={open ? "收起上游错误响应规则" : "展开上游错误响应规则"}
            aria-expanded={open}
            className="rounded-md p-1 text-muted-foreground hover:bg-secondary/70"
            onClick={() => setOpen((value) => !value)}
          >
            <ChevronDown className={cn("h-5 w-5 transition-transform", open && "rotate-180")} />
          </button>
        </div>

        {open ? (
          <div className="border-t border-border px-5 py-4">
            {sortedRules.length === 0 ? (
              <div className="py-5 text-center text-sm text-muted-foreground">暂无规则</div>
            ) : (
              <div className="divide-y divide-border">
                {sortedRules.map(({ rule }) => (
                  <div key={rule.id} className="flex items-center gap-3 py-3 first:pt-0 last:pb-0">
                    <Switch
                      checked={rule.enabled}
                      disabled={busy}
                      aria-label={`${rule.enabled ? "停用" : "启用"}规则 ${rule.name}`}
                      onCheckedChange={(checked) => void toggleRule(rule, checked)}
                    />
                    <div className="min-w-0 flex-1">
                      <div className="flex flex-wrap items-center gap-2">
                        <span className="truncate text-sm font-medium text-foreground">
                          {rule.name}
                        </span>
                        <span className="rounded bg-secondary px-1.5 py-0.5 text-[11px] text-muted-foreground">
                          P{rule.priority}
                        </span>
                        {!rule.enabled ? (
                          <span className="rounded bg-secondary px-1.5 py-0.5 text-[11px] text-muted-foreground">
                            已停用
                          </span>
                        ) : null}
                      </div>
                      <div className="mt-1 flex flex-wrap gap-x-3 gap-y-1 text-xs text-muted-foreground">
                        <span>{ruleMatchSummary(rule)}</span>
                        <span>{ruleScopeSummary(rule, providers)}</span>
                        <span>{ruleActionSummary(rule)}</span>
                      </div>
                    </div>
                    <Tooltip content="编辑规则">
                      <Button
                        size="icon"
                        variant="ghost"
                        aria-label={`编辑规则 ${rule.name}`}
                        disabled={busy}
                        onClick={() => openEditEditor(rule)}
                      >
                        <Pencil className="h-4 w-4" />
                      </Button>
                    </Tooltip>
                    <Tooltip content="删除规则">
                      <Button
                        size="icon"
                        variant="ghost"
                        aria-label={`删除规则 ${rule.name}`}
                        disabled={busy}
                        onClick={() => setDeleteTarget(rule)}
                      >
                        <Trash2 className="h-4 w-4 text-destructive" />
                      </Button>
                    </Tooltip>
                  </div>
                ))}
              </div>
            )}
          </div>
        ) : null}
      </div>

      <RuleEditorDialog
        editor={editor}
        providers={providers}
        providerLoadState={providerLoadState}
        saving={busy}
        onChange={setEditor}
        onClose={() => setEditor(null)}
        onSave={() => void saveEditor()}
      />
      <ConfirmDialog
        open={deleteTarget != null}
        title="删除响应规则"
        description={deleteTarget ? `确认删除“${deleteTarget.name}”？` : undefined}
        confirming={saving}
        confirmLabel="删除"
        confirmingLabel="删除中…"
        confirmVariant="danger"
        onClose={() => setDeleteTarget(null)}
        onConfirm={() => void deleteRule()}
      />
    </>
  );
}

function RuleEditorDialog({
  editor,
  providers,
  providerLoadState,
  saving,
  onChange,
  onClose,
  onSave,
}: {
  editor: EditorState | null;
  providers: ProviderSummary[];
  providerLoadState: ProviderLoadState;
  saving: boolean;
  onChange: (editor: EditorState) => void;
  onClose: () => void;
  onSave: () => void;
}) {
  if (!editor) return null;
  const { rule } = editor;
  const knownProviderIds = new Set(providers.map((provider) => provider.id));
  const deletedProviderIds =
    providerLoadState === "ready"
      ? rule.provider_ids.filter((id) => !knownProviderIds.has(id))
      : [];
  const unavailableProviderIds =
    providerLoadState === "error"
      ? rule.provider_ids.filter((id) => !knownProviderIds.has(id))
      : [];
  const setRule = (patch: Partial<UpstreamErrorResponseRule>) =>
    onChange({ ...editor, rule: { ...rule, ...patch } });

  return (
    <Dialog
      open={true}
      title={rule.name.trim() ? `编辑规则 · ${rule.name.trim()}` : "新建响应规则"}
      className="max-w-3xl"
      onOpenChange={(nextOpen) => {
        if (!nextOpen && !saving) onClose();
      }}
    >
      <div className="space-y-5">
        <div className="grid gap-4 sm:grid-cols-[minmax(0,1fr)_8rem]">
          <Field label="名称">
            <Input
              aria-label="规则名称"
              value={rule.name}
              maxLength={100}
              disabled={saving}
              onChange={(event) => setRule({ name: event.currentTarget.value })}
            />
          </Field>
          <Field label="优先级">
            <Input
              aria-label="规则优先级"
              type="number"
              min={0}
              max={9999}
              value={rule.priority}
              disabled={saving}
              onChange={(event) => {
                const priority = event.currentTarget.valueAsNumber;
                if (Number.isFinite(priority)) setRule({ priority });
              }}
            />
          </Field>
        </div>
        <Field label="说明">
          <Input
            aria-label="规则说明"
            value={rule.description}
            maxLength={256}
            disabled={saving}
            onChange={(event) => setRule({ description: event.currentTarget.value })}
          />
        </Field>

        <section className="border-t border-border pt-4">
          <div className="mb-3 flex flex-wrap items-center justify-between gap-3">
            <h3 className="text-sm font-semibold text-foreground">匹配条件</h3>
            <div className="inline-flex rounded-md border border-border p-0.5">
              {(["any", "all"] as const).map((mode) => (
                <button
                  key={mode}
                  type="button"
                  disabled={saving}
                  className={cn(
                    "rounded px-3 py-1 text-xs",
                    rule.match_mode === mode
                      ? "bg-state-selected text-state-selected-foreground"
                      : "text-muted-foreground hover:bg-secondary"
                  )}
                  onClick={() => setRule({ match_mode: mode })}
                >
                  {mode === "any" ? "任一匹配" : "全部匹配"}
                </button>
              ))}
            </div>
          </div>
          <div className="grid gap-4 sm:grid-cols-2">
            <Field label="上游状态码" hint="400-599，以逗号分隔">
              <Input
                aria-label="匹配上游状态码"
                value={editor.statusCodesText}
                placeholder="例如 400, 429, 503"
                disabled={saving}
                onChange={(event) =>
                  onChange({ ...editor, statusCodesText: event.currentTarget.value })
                }
              />
            </Field>
            <Field label="错误关键词" hint="每行一个，不区分大小写">
              <Textarea
                aria-label="匹配错误关键词"
                value={editor.keywordsText}
                rows={3}
                placeholder={"quota exhausted\ninvalid request"}
                disabled={saving}
                onChange={(event) =>
                  onChange({ ...editor, keywordsText: event.currentTarget.value })
                }
              />
            </Field>
          </div>
        </section>

        <section className="border-t border-border pt-4">
          <h3 className="mb-3 text-sm font-semibold text-foreground">生效范围</h3>
          <div className="grid gap-4 sm:grid-cols-2">
            <Field label="CLI" hint="不选即全部">
              <div className="flex flex-wrap gap-2">
                {CLI_REGISTRY.map((cli) => (
                  <ScopeCheckbox
                    key={cli.key}
                    checked={rule.cli_keys.includes(cli.key)}
                    disabled={saving}
                    label={cli.name}
                    onChange={() => setRule({ cli_keys: toggleValue(rule.cli_keys, cli.key) })}
                  />
                ))}
              </div>
            </Field>
            <Field label="供应商" hint="不选即全部">
              <div className="max-h-40 space-y-2 overflow-y-auto rounded-md border border-border p-2">
                {providerLoadState === "loading" ? (
                  <div className="px-1 py-2 text-xs text-muted-foreground">正在读取…</div>
                ) : null}
                {providers.map((provider) => (
                  <ScopeCheckbox
                    key={provider.id}
                    checked={rule.provider_ids.includes(provider.id)}
                    disabled={saving}
                    label={`${cliLabel(provider.cli_key)} · ${provider.name}`}
                    onChange={() =>
                      setRule({ provider_ids: toggleValue(rule.provider_ids, provider.id) })
                    }
                  />
                ))}
                {deletedProviderIds.map((id) => (
                  <ScopeCheckbox
                    key={`deleted-${String(id)}`}
                    checked={true}
                    disabled={saving}
                    label={`已删除供应商 #${String(id)}`}
                    onChange={() =>
                      setRule({ provider_ids: rule.provider_ids.filter((value) => value !== id) })
                    }
                  />
                ))}
                {unavailableProviderIds.map((id) => (
                  <ScopeCheckbox
                    key={`unavailable-${String(id)}`}
                    checked={true}
                    disabled={saving}
                    label={`供应商 #${String(id)}（数据不可用）`}
                    onChange={() =>
                      setRule({ provider_ids: rule.provider_ids.filter((value) => value !== id) })
                    }
                  />
                ))}
                {providerLoadState === "error" && providers.length === 0 ? (
                  <div className="px-1 py-2 text-xs text-muted-foreground">供应商数据不可用</div>
                ) : null}
                {providerLoadState === "ready" && providers.length === 0 ? (
                  <div className="px-1 py-2 text-xs text-muted-foreground">暂无供应商</div>
                ) : null}
              </div>
            </Field>
          </div>
        </section>

        <section className="border-t border-border pt-4">
          <h3 className="mb-3 text-sm font-semibold text-foreground">响应行为</h3>
          <div className="grid gap-4 sm:grid-cols-2">
            <Field label="状态码">
              <div className="flex gap-2">
                <Select
                  aria-label="响应状态码行为"
                  value={rule.status_behavior.mode}
                  disabled={saving}
                  onChange={(event) =>
                    setRule({
                      status_behavior:
                        event.currentTarget.value === "override"
                          ? { mode: "override", status_code: 500 }
                          : { mode: "passthrough" },
                    })
                  }
                >
                  <option value="passthrough">透传上游状态码</option>
                  <option value="override">自定义状态码</option>
                </Select>
                {rule.status_behavior.mode === "override" ? (
                  <Input
                    aria-label="自定义响应状态码"
                    type="number"
                    min={400}
                    max={599}
                    className="w-24"
                    value={rule.status_behavior.status_code}
                    disabled={saving}
                    onChange={(event) => {
                      const statusCode = event.currentTarget.valueAsNumber;
                      if (Number.isFinite(statusCode)) {
                        setRule({
                          status_behavior: { mode: "override", status_code: statusCode },
                        });
                      }
                    }}
                  />
                ) : null}
              </div>
            </Field>
            <Field label="错误信息">
              <Select
                aria-label="响应错误信息行为"
                value={rule.message_behavior.mode}
                disabled={saving}
                onChange={(event) =>
                  setRule({
                    message_behavior:
                      event.currentTarget.value === "override"
                        ? { mode: "override", message: "" }
                        : { mode: "passthrough" },
                  })
                }
              >
                <option value="passthrough">提取并透传上游信息</option>
                <option value="override">自定义错误信息</option>
              </Select>
            </Field>
          </div>
          {rule.message_behavior.mode === "override" ? (
            <div className="mt-4">
              <Field label="自定义错误信息">
                <Textarea
                  aria-label="自定义响应错误信息"
                  rows={4}
                  maxLength={4096}
                  value={rule.message_behavior.message}
                  disabled={saving}
                  onChange={(event) =>
                    setRule({
                      message_behavior: {
                        mode: "override",
                        message: event.currentTarget.value,
                      },
                    })
                  }
                />
              </Field>
            </div>
          ) : null}
        </section>

        <div className="flex items-center justify-between gap-3 border-t border-border pt-4">
          <div className="flex items-center gap-2 text-sm text-foreground">
            <Switch
              checked={rule.enabled}
              disabled={saving}
              onCheckedChange={(enabled) => setRule({ enabled })}
            />
            启用规则
          </div>
          <div className="flex gap-2">
            <Button variant="secondary" disabled={saving} onClick={onClose}>
              取消
            </Button>
            <Button variant="primary" disabled={saving} onClick={onSave}>
              {saving ? "保存中…" : "保存规则"}
            </Button>
          </div>
        </div>
      </div>
    </Dialog>
  );
}

function Field({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children: React.ReactNode;
}) {
  return (
    <div className="block">
      <span className="mb-1 flex items-center gap-2 text-xs font-medium text-muted-foreground">
        {label}
        {hint ? <span className="font-normal opacity-80">{hint}</span> : null}
      </span>
      {children}
    </div>
  );
}

function ScopeCheckbox({
  checked,
  disabled,
  label,
  onChange,
}: {
  checked: boolean;
  disabled: boolean;
  label: string;
  onChange: () => void;
}) {
  return (
    <label className="flex min-w-0 cursor-pointer items-center gap-2 rounded-md border border-border px-2.5 py-1.5 text-xs text-foreground hover:bg-secondary/60">
      <input
        type="checkbox"
        className="h-3.5 w-3.5 shrink-0 accent-[var(--accent)]"
        checked={checked}
        disabled={disabled}
        onChange={onChange}
      />
      <span className="truncate">{label}</span>
    </label>
  );
}
