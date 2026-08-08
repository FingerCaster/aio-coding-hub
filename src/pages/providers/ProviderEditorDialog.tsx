import { ChevronDown } from "lucide-react";
import type {
  CliKey,
  ModelRoutingPolicy,
  ProviderSummary,
  UpstreamRetryPolicy,
} from "../../services/providers/providers";
import { Button } from "../../ui/Button";
import { Dialog } from "../../ui/Dialog";
import { FormField } from "../../ui/FormField";
import { Input } from "../../ui/Input";
import { Switch } from "../../ui/Switch";
import { TabList } from "../../ui/TabList";
import type { ProviderEditorInitialValues } from "./providerDuplicate";
import { useProviderEditorForm } from "./useProviderEditorForm";
import { OAuthSection } from "./OAuthSection";
import { Cx2ccSection } from "./Cx2ccSection";
import { ApiKeySection } from "./ApiKeySection";
import { ProviderAccountUsageSection } from "./ProviderAccountUsageSection";
import { LimitsSection } from "./LimitsSection";
import { ClaudeModelSection } from "./ClaudeModelSection";
import { RetryPolicyFields } from "../../components/gateway/RetryPolicyFields";
import { ModelRoutingPolicyFields } from "../../components/gateway/ModelRoutingPolicyFields";
import { cn } from "../../utils/cn";
import { ContributionSlot } from "../../plugins/contributions/ContributionSlot";

type ProviderEditorDialogBaseProps = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onSaved: (cliKey: CliKey) => void;
  onModelFetchFailedAfterSave?: (provider: ProviderSummary) => void;
  codexProviders?: ProviderSummary[];
};

export type ProviderEditorDialogProps =
  | (ProviderEditorDialogBaseProps & {
      mode: "create";
      cliKey: CliKey;
      initialValues?: ProviderEditorInitialValues | null;
    })
  | (ProviderEditorDialogBaseProps & {
      mode: "edit";
      provider: ProviderSummary;
    });

export function ProviderEditorDialog(props: ProviderEditorDialogProps) {
  const f = useProviderEditorForm(props);
  const saveBlocked =
    f.saving ||
    f.accountUsageCustomTestInFlight ||
    (f.accountUsageAdapterKind === "custom" && Boolean(f.accountUsageCustomAllowedOriginsError));

  return (
    <Dialog
      open={f.open}
      onOpenChange={(nextOpen) => {
        if (!nextOpen && (f.saving || f.accountUsageCustomTestInFlight)) return;
        f.onOpenChange(nextOpen);
      }}
      title={f.title}
      description={f.description}
      className="max-w-4xl"
    >
      <div className="space-y-4">
        {/* ── Auth mode selector ── */}
        {f.supportsOAuth ? (
          <FormField label="认证方式" hint="选择后下方表单会相应变化">
            <TabList<"api_key" | "oauth">
              ariaLabel="认证方式"
              items={[
                { key: "api_key", label: "API 密钥" },
                { key: "oauth", label: "OAuth 登录" },
              ]}
              value={f.authMode as "api_key" | "oauth"}
              onChange={(next) => {
                f.setAuthMode(next);
                f.setValue("auth_mode", next, { shouldDirty: true });
              }}
            />
          </FormField>
        ) : f.supportsCx2cc ? (
          <FormField label="认证方式" hint="选择后下方表单会相应变化">
            <TabList<"api_key" | "oauth" | "cx2cc">
              ariaLabel="认证方式"
              items={[
                { key: "api_key", label: "API 密钥" },
                ...(f.supportsOAuth ? [{ key: "oauth" as const, label: "OAuth 登录" }] : []),
                { key: "cx2cc", label: "CX2CC" },
              ]}
              value={f.authMode as "api_key" | "oauth" | "cx2cc"}
              onChange={(next) => {
                f.setAuthMode(next);
                f.setValue("auth_mode", next === "cx2cc" ? "api_key" : next, { shouldDirty: true });
              }}
            />
          </FormField>
        ) : null}

        {f.authMode === "oauth" ? (
          <OAuthSection form={f} />
        ) : f.authMode === "cx2cc" && f.cliKey === "claude" ? (
          <Cx2ccSection form={f} />
        ) : (
          <ApiKeySection form={f} />
        )}

        <FormField
          label="流式空闲超时覆盖（秒）"
          hint="留空或 0 表示沿用全局设置；仅对当前 Provider 的流式请求生效。"
        >
          <Input
            type="number"
            min="0"
            max="3600"
            step="1"
            placeholder="0"
            value={f.streamIdleTimeoutSeconds}
            onChange={(e) => f.setStreamIdleTimeoutSeconds(e.currentTarget.value)}
            disabled={f.saving}
          />
        </FormField>

        <ProviderAccountUsageSection form={f} />

        <ContributionSlot
          slotId="providers.editor.sections"
          valuesByContributionKey={f.extensionValuesByContributionKey}
          onChange={(contribution, key, value) => f.setExtensionValue(contribution, key, value)}
          disabled={f.saving}
        />

        <ProviderRetryPolicySection form={f} />
        <ProviderModelRoutingPolicySection form={f} />

        <LimitsSection form={f} />
        <ClaudeModelSection form={f} />

        <div className="flex items-center justify-between border-t border-border pt-3 dark:border-border">
          <div className="flex items-center gap-2">
            <span className="text-sm text-secondary-foreground">启用</span>
            <Switch
              checked={f.enabled}
              onCheckedChange={(checked) => f.setValue("enabled", checked, { shouldDirty: true })}
              disabled={f.saving}
            />
          </div>
          <div className="flex flex-wrap items-center justify-end gap-2">
            <Button
              onClick={() => f.onOpenChange(false)}
              variant="secondary"
              disabled={f.saving || f.accountUsageCustomTestInFlight}
            >
              取消
            </Button>
            {f.canFetchProviderModels ? (
              <Button
                onClick={() => void f.saveAndFetchModels()}
                variant="secondary"
                disabled={saveBlocked}
              >
                {f.savingWithModelFetch ? "保存并获取中…" : "保存并获取模型"}
              </Button>
            ) : null}
            <Button onClick={f.save} variant="primary" disabled={saveBlocked}>
              {f.saving && !f.savingWithModelFetch ? "保存中…" : "保存"}
            </Button>
          </div>
        </div>
      </div>
    </Dialog>
  );
}

function ProviderModelRoutingPolicySection({
  form,
}: {
  form: ReturnType<typeof useProviderEditorForm>;
}) {
  const mode = form.modelRoutingMode;

  function updatePolicy(next: ModelRoutingPolicy) {
    form.setModelRoutingPolicyDraft(next);
  }

  return (
    <div className="overflow-hidden rounded-lg border border-border bg-white dark:bg-secondary">
      <div className="space-y-3 px-4 py-3">
        <div>
          <div className="text-sm font-semibold text-foreground">模型路由</div>
          <div className="mt-0.5 text-xs text-muted-foreground">
            选择当前供应商继承全局策略、使用专属规则，或明确关闭模型路由。
          </div>
        </div>
        <TabList<"inherit" | "custom" | "disabled">
          ariaLabel="供应商模型路由模式"
          items={[
            { key: "inherit", label: "继承全局", disabled: form.saving },
            { key: "custom", label: "使用专属规则", disabled: form.saving },
            { key: "disabled", label: "禁用路由", disabled: form.saving },
          ]}
          value={mode}
          onChange={form.setModelRoutingMode}
        />
      </div>
      {mode === "custom" ? (
        <div className="space-y-4 border-t border-border px-4 py-4">
          <ModelRoutingPolicyFields
            policy={{ ...form.modelRoutingPolicyDraft, enabled: true }}
            disabled={form.saving}
            showEnabled={false}
            onChange={updatePolicy}
          />
        </div>
      ) : null}
    </div>
  );
}

function ProviderRetryPolicySection({ form }: { form: ReturnType<typeof useProviderEditorForm> }) {
  const enabled = form.upstreamRetryPolicyOverrideEnabled;
  const policy = form.upstreamRetryPolicyDraft;

  function updatePolicy(next: UpstreamRetryPolicy) {
    form.setUpstreamRetryPolicyDraft(next);
  }

  return (
    <div className="overflow-hidden rounded-lg border border-border bg-white dark:bg-secondary">
      <div className="flex w-full items-center justify-between gap-3 px-4 py-3 transition-colors hover:bg-secondary/50 dark:hover:bg-secondary/40">
        <button
          type="button"
          className="min-w-0 flex-1 text-left"
          onClick={() => form.setUpstreamRetryPolicyOverrideEnabled(!enabled)}
          aria-expanded={enabled}
        >
          <div className="text-sm font-semibold text-foreground">覆盖全局重试策略</div>
          <div className="mt-0.5 text-xs text-muted-foreground">
            关闭时继承全局；开启后当前供应商使用自己的瞬时错误重试规则。
          </div>
        </button>
        <div className="flex items-center gap-2">
          <Switch
            checked={enabled}
            aria-label="覆盖全局重试策略"
            onCheckedChange={(checked) => form.setUpstreamRetryPolicyOverrideEnabled(checked)}
            disabled={form.saving}
          />
          <ChevronDown
            className={cn(
              "h-4 w-4 text-muted-foreground transition-transform",
              enabled && "rotate-180"
            )}
          />
        </div>
      </div>
      {enabled ? (
        <div className="space-y-4 border-t border-border px-4 py-4">
          <RetryPolicyFields policy={policy} disabled={form.saving} onChange={updatePolicy} />
        </div>
      ) : null}
    </div>
  );
}
