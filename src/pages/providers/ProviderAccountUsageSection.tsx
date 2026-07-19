import { useState } from "react";
import { ChevronDown, Eye, EyeOff, Trash2 } from "lucide-react";
import { Button } from "../../ui/Button";
import { FormField } from "../../ui/FormField";
import { Input } from "../../ui/Input";
import { Switch } from "../../ui/Switch";
import { RadioButtonGroup } from "./RadioButtonGroup";
import type { UseProviderEditorFormReturn } from "./useProviderEditorForm";
import {
  PROVIDER_ACCOUNT_USAGE_MAX_REFRESH_INTERVAL_SECONDS,
  PROVIDER_ACCOUNT_USAGE_MIN_REFRESH_INTERVAL_SECONDS,
  type ProviderAccountUsageAdapterKind,
  type ProviderAccountUsageNewApiQueryMode,
} from "../../services/providers/providerAccountUsageConfig";

export function ProviderAccountUsageSection({ form }: { form: UseProviderEditorFormReturn }) {
  const [showAccessToken, setShowAccessToken] = useState(false);
  if (form.authMode !== "api_key") return null;
  const accountUsageEnabled = form.accountUsageAdapterKind !== "disabled";
  const accountMode =
    form.accountUsageAdapterKind === "newapi" && form.accountUsageNewApiQueryMode === "account";
  const accountUsageSummary =
    form.accountUsageAdapterKind === "disabled"
      ? "关闭"
      : form.accountUsageAdapterKind === "sub2api"
        ? "sub2api"
        : form.accountUsageNewApiQueryMode === "billing"
          ? "NewAPI · 模型令牌额度"
          : "NewAPI · 用户账户余额";
  const accessTokenHint = form.accountUsageNewApiAccessTokenConfigured
    ? "已配置。留空表示不改，输入新值表示替换。"
    : "当前未配置。可留空保存。";

  return (
    <details className="group rounded-xl border border-border bg-white shadow-sm open:ring-2 open:ring-accent/10 transition-all dark:border-border dark:bg-secondary">
      <summary className="flex cursor-pointer items-center justify-between gap-3 px-4 py-3 select-none">
        <div className="flex min-w-0 flex-wrap items-center gap-x-3 gap-y-1">
          <span className="text-sm font-medium text-secondary-foreground group-open:text-accent dark:text-secondary-foreground">
            账户用量
          </span>
          <span className="text-xs text-muted-foreground">{accountUsageSummary}</span>
          {form.accountUsageCredentialsRequired ? (
            <span className="text-xs font-medium text-amber-700 dark:text-amber-400">
              需配置账户凭据
            </span>
          ) : null}
        </div>
        <ChevronDown className="h-4 w-4 shrink-0 text-muted-foreground transition-transform group-open:rotate-180" />
      </summary>

      <div className="grid gap-3 border-t border-border px-4 py-4 sm:grid-cols-2 dark:border-border">
        <FormField label="账户用量">
          <RadioButtonGroup<ProviderAccountUsageAdapterKind>
            items={[
              { value: "disabled", label: "关闭" },
              { value: "sub2api", label: "sub2api" },
              { value: "newapi", label: "NewAPI" },
            ]}
            ariaLabel="账户用量适配器"
            value={form.accountUsageAdapterKind}
            onChange={(next) => form.setAccountUsageAdapterKind(next)}
            disabled={form.saving}
            size="compact"
            fullWidth={false}
          />
        </FormField>

        {form.accountUsageAdapterKind === "newapi" ? (
          <FormField
            label="NewAPI 查询方式"
            hint={
              form.accountUsageCredentialsRequired ? (
                <span className="text-amber-700 dark:text-amber-400">需配置账户凭据</span>
              ) : undefined
            }
          >
            <RadioButtonGroup<ProviderAccountUsageNewApiQueryMode>
              items={[
                { value: "billing", label: "模型令牌额度" },
                { value: "account", label: "用户账户余额" },
              ]}
              ariaLabel="NewAPI 查询方式"
              value={form.accountUsageNewApiQueryMode}
              onChange={form.setAccountUsageNewApiQueryMode}
              disabled={form.saving}
              size="compact"
            />
          </FormField>
        ) : null}

        {accountMode ? (
          <>
            <FormField label="User ID">
              <Input
                value={form.accountUsageNewApiUserId}
                onChange={(event) => form.setAccountUsageNewApiUserId(event.currentTarget.value)}
                placeholder="正整数"
                inputMode="numeric"
                pattern="[0-9]*"
                autoComplete="off"
                disabled={form.saving}
              />
            </FormField>

            <FormField label="系统访问令牌" hint={accessTokenHint}>
              <div className="flex min-w-0 items-center gap-2">
                <Input
                  type={showAccessToken ? "text" : "password"}
                  value={form.accountUsageNewApiAccessToken}
                  onChange={(event) =>
                    form.setAccountUsageNewApiAccessToken(event.currentTarget.value)
                  }
                  placeholder={
                    form.accountUsageNewApiAccessTokenConfigured ? "留空表示不改" : "可留空"
                  }
                  autoComplete="new-password"
                  disabled={form.saving}
                  className="min-w-0"
                />
                <Button
                  type="button"
                  variant="secondary"
                  size="icon"
                  className="h-10 w-10 shrink-0"
                  onClick={() => setShowAccessToken((visible) => !visible)}
                  disabled={form.saving}
                  aria-label={showAccessToken ? "隐藏系统访问令牌" : "显示系统访问令牌"}
                  title={showAccessToken ? "隐藏系统访问令牌" : "显示系统访问令牌"}
                >
                  {showAccessToken ? (
                    <EyeOff className="h-4 w-4" aria-hidden="true" />
                  ) : (
                    <Eye className="h-4 w-4" aria-hidden="true" />
                  )}
                </Button>
              </div>
            </FormField>
          </>
        ) : null}

        {form.accountUsageCredentialsPresent ? (
          <FormField label="账户凭据" hint={accountMode ? undefined : "已保存，当前查询不会使用"}>
            <Button
              type="button"
              variant="secondary"
              size="sm"
              className="h-10"
              onClick={form.clearAccountUsageCredentials}
              disabled={form.saving}
            >
              <Trash2 className="mr-2 h-4 w-4" aria-hidden="true" />
              清除账户凭据
            </Button>
          </FormField>
        ) : null}

        {accountUsageEnabled ? (
          <>
            <FormField label="定时刷新">
              <div className="flex h-10 items-center justify-between gap-3 rounded-lg border border-line bg-surface-inset px-3">
                <span className="text-sm text-foreground">启用</span>
                <Switch
                  size="sm"
                  checked={form.accountUsageTimedRefreshEnabled}
                  onCheckedChange={(next) => form.setAccountUsageTimedRefreshEnabled(next)}
                  disabled={form.saving}
                  aria-label="定时刷新账户用量"
                />
              </div>
            </FormField>

            <FormField label="刷新间隔（秒）" hint="60-300s">
              <Input
                type="number"
                min={PROVIDER_ACCOUNT_USAGE_MIN_REFRESH_INTERVAL_SECONDS}
                max={PROVIDER_ACCOUNT_USAGE_MAX_REFRESH_INTERVAL_SECONDS}
                step={1}
                inputMode="numeric"
                value={form.accountUsageRefreshIntervalSeconds}
                onChange={(event) => {
                  const next = event.currentTarget.valueAsNumber;
                  if (Number.isFinite(next)) form.setAccountUsageRefreshIntervalSeconds(next);
                }}
                disabled={form.saving || !form.accountUsageTimedRefreshEnabled}
              />
            </FormField>
          </>
        ) : null}
      </div>
    </details>
  );
}
