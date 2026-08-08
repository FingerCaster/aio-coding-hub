import { useId, useState } from "react";
import { ChevronDown, Eye, EyeOff, Loader2, Play, ShieldAlert, Trash2 } from "lucide-react";
import { Button } from "../../ui/Button";
import { FormField } from "../../ui/FormField";
import { Input } from "../../ui/Input";
import { Switch } from "../../ui/Switch";
import { Textarea } from "../../ui/Textarea";
import { RadioButtonGroup } from "./RadioButtonGroup";
import type { UseProviderEditorFormReturn } from "./useProviderEditorForm";
import {
  PROVIDER_ACCOUNT_USAGE_MAX_CUSTOM_ALLOWED_ORIGINS,
  PROVIDER_ACCOUNT_USAGE_MAX_CUSTOM_SCRIPT_BYTES,
  PROVIDER_ACCOUNT_USAGE_MAX_CUSTOM_TIMEOUT_SECONDS,
  PROVIDER_ACCOUNT_USAGE_MAX_REFRESH_INTERVAL_SECONDS,
  PROVIDER_ACCOUNT_USAGE_MIN_CUSTOM_TIMEOUT_SECONDS,
  PROVIDER_ACCOUNT_USAGE_MIN_REFRESH_INTERVAL_SECONDS,
  getProviderAccountUsageCustomScriptUtf8ByteLength,
  type ProviderAccountUsageAdapterKind,
  type ProviderAccountUsageNewApiQueryMode,
} from "../../services/providers/providerAccountUsageConfig";

type AccountUsageTestResult = NonNullable<
  UseProviderEditorFormReturn["accountUsageCustomTestResult"]
>;

function accountUsageStatusLabel(status: AccountUsageTestResult["status"]) {
  switch (status) {
    case "available":
      return "可用";
    case "zero_balance":
      return "无可用额度";
    case "expired":
      return "已过期";
    case "auth_failed":
      return "认证失败";
    case "configuration_required":
      return "需配置";
    case "query_failed":
      return "查询失败";
    default:
      return "未支持";
  }
}

function formatTestAmount(value: number | null, unit: string | null) {
  if (value == null || !Number.isFinite(value)) return null;
  const formatted = new Intl.NumberFormat("zh-CN", { maximumFractionDigits: 4 }).format(value);
  return unit ? `${formatted} ${unit}` : formatted;
}

function formatTestMetric(
  label: string,
  usedValue: number | null,
  totalValue: number | null,
  unit: string | null
) {
  const used = formatTestAmount(usedValue, unit);
  const total = formatTestAmount(totalValue, unit);
  if (used && total) return `${label} ${used} / ${total}`;
  if (used) return `${label}已用 ${used}`;
  if (total) return `${label}总额 ${total}`;
  return null;
}

function formatTestExpiry(value: number | null) {
  if (value == null || !Number.isSafeInteger(value) || value <= 0) return null;
  const date = new Date(value * 1000);
  if (!Number.isFinite(date.getTime())) return null;
  return `到期 ${date.toLocaleString("zh-CN")}`;
}

function accountUsageTestDetails(result: AccountUsageTestResult) {
  const unit = result.unit;
  const details = [
    result.plan_name ? `套餐 ${result.plan_name}` : null,
    formatTestAmount(result.balance, unit)
      ? `余额 ${formatTestAmount(result.balance, unit)}`
      : null,
    formatTestAmount(result.plan_remaining, unit)
      ? `套餐剩余 ${formatTestAmount(result.plan_remaining, unit)}`
      : null,
    formatTestMetric("用量", result.used, result.total, unit),
    formatTestMetric("日", result.daily_used, result.daily_total, unit),
    formatTestMetric("周", result.weekly_used, result.weekly_total, unit),
    formatTestMetric("月", result.monthly_used, result.monthly_total, unit),
    formatTestExpiry(result.expires_at),
    result.unit_note,
  ];
  return details.filter((detail): detail is string => Boolean(detail));
}

function accountUsageTestTone(status: AccountUsageTestResult["status"]) {
  switch (status) {
    case "available":
      return "border-emerald-500 bg-emerald-50/70 text-emerald-900 dark:bg-emerald-950/20 dark:text-emerald-200";
    case "zero_balance":
    case "expired":
    case "auth_failed":
      return "border-rose-500 bg-rose-50/70 text-rose-800 dark:bg-rose-950/20 dark:text-rose-300";
    default:
      return "border-amber-500 bg-amber-50/70 text-amber-900 dark:bg-amber-950/20 dark:text-amber-200";
  }
}

function describedBy(...ids: Array<string | undefined>) {
  const value = ids.filter(Boolean).join(" ");
  return value || undefined;
}

export function ProviderAccountUsageSection({ form }: { form: UseProviderEditorFormReturn }) {
  const [showAccessToken, setShowAccessToken] = useState(false);
  const customSecurityWarningId = useId();
  const customOriginsErrorId = useId();
  if (form.authMode !== "api_key") return null;
  const accountUsageEnabled = form.accountUsageAdapterKind !== "disabled";
  const accountMode =
    form.accountUsageAdapterKind === "newapi" && form.accountUsageNewApiQueryMode === "account";
  const customMode = form.accountUsageAdapterKind === "custom";
  const accountUsageSummary =
    form.accountUsageAdapterKind === "disabled"
      ? "关闭"
      : form.accountUsageAdapterKind === "sub2api"
        ? "Sub2Api"
        : form.accountUsageAdapterKind === "custom"
          ? `自定义 JS · ${form.accountUsageCustomEnabled ? "已启用" : "未启用"}`
          : form.accountUsageNewApiQueryMode === "billing"
            ? "NewApi · 模型令牌额度"
            : "NewApi · 用户账户余额";
  const accessTokenHint = form.accountUsageNewApiAccessTokenConfigured
    ? "已配置。留空表示不改，输入新值表示替换。"
    : "当前未配置。可留空保存。";
  const customTestDetails = form.accountUsageCustomTestResult
    ? accountUsageTestDetails(form.accountUsageCustomTestResult)
    : [];
  const customTestHint =
    form.editingProviderId == null
      ? "保存供应商后可测试"
      : !form.apiKeyConfigured
        ? "需先保存 API Key"
        : form.accountUsageCustomAllowedOriginsError
          ? "请先修正额外 HTTPS Origin"
          : form.accountUsageCustomTestInFlight
            ? "请等待当前测试完成"
            : undefined;

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
          {customMode && form.accountUsageCustomAllowedOriginsError ? (
            <span className="text-xs font-medium text-rose-700 dark:text-rose-300">
              Origin 配置有误
            </span>
          ) : null}
        </div>
        <ChevronDown className="h-4 w-4 shrink-0 text-muted-foreground transition-transform group-open:rotate-180" />
      </summary>

      <div className="space-y-3 border-t border-border px-4 py-4 dark:border-border">
        <div role="group" aria-label="账户用量选择设置" className="grid gap-3 sm:grid-cols-2">
          <FormField label="账户用量" className="min-w-0">
            <RadioButtonGroup<ProviderAccountUsageAdapterKind>
              items={[
                { value: "disabled", label: "关闭" },
                { value: "sub2api", label: "Sub2Api" },
                { value: "newapi", label: "NewApi" },
                { value: "custom", label: "自定义 JS" },
              ]}
              ariaLabel="账户用量适配器"
              value={form.accountUsageAdapterKind}
              onChange={(next) => form.setAccountUsageAdapterKind(next)}
              disabled={form.saving}
              size="compact"
            />
          </FormField>

          {form.accountUsageAdapterKind === "newapi" ? (
            <FormField
              label="NewApi 查询方式"
              hint={
                form.accountUsageCredentialsRequired ? (
                  <span className="text-amber-700 dark:text-amber-400">需配置账户凭据</span>
                ) : undefined
              }
              className="min-w-0"
            >
              <RadioButtonGroup<ProviderAccountUsageNewApiQueryMode>
                items={[
                  { value: "billing", label: "模型令牌额度" },
                  { value: "account", label: "用户账户余额" },
                ]}
                ariaLabel="NewApi 查询方式"
                value={form.accountUsageNewApiQueryMode}
                onChange={form.setAccountUsageNewApiQueryMode}
                disabled={form.saving}
                size="compact"
              />
            </FormField>
          ) : null}
        </div>

        {customMode ? (
          <div role="group" aria-label="自定义账户用量设置" className="space-y-3">
            <FormField
              label="JavaScript"
              hint={`${getProviderAccountUsageCustomScriptUtf8ByteLength(form.accountUsageCustomScript)}/${PROVIDER_ACCOUNT_USAGE_MAX_CUSTOM_SCRIPT_BYTES} 字节`}
            >
              {(id, hintId) => (
                <Textarea
                  id={id}
                  aria-label="账户用量 JavaScript"
                  aria-describedby={describedBy(hintId, customSecurityWarningId)}
                  mono
                  rows={14}
                  maxLength={PROVIDER_ACCOUNT_USAGE_MAX_CUSTOM_SCRIPT_BYTES}
                  value={form.accountUsageCustomScript}
                  onChange={(event) => form.setAccountUsageCustomScript(event.currentTarget.value)}
                  spellCheck={false}
                  autoComplete="off"
                  disabled={form.saving}
                  className="min-h-64 text-xs leading-5"
                />
              )}
            </FormField>

            <div className="grid gap-3 sm:grid-cols-2">
              <FormField
                label="额外 HTTPS Origin"
                hint={
                  <span
                    className={
                      form.accountUsageCustomAllowedOriginsError
                        ? "text-rose-700 dark:text-rose-300"
                        : undefined
                    }
                  >
                    {form.accountUsageCustomAllowedOriginsCount}/
                    {PROVIDER_ACCOUNT_USAGE_MAX_CUSTOM_ALLOWED_ORIGINS}
                  </span>
                }
                className="min-w-0"
              >
                {(id, hintId) => (
                  <>
                    <Textarea
                      id={id}
                      aria-label="额外 HTTPS Origin，每行一个"
                      aria-describedby={describedBy(
                        hintId,
                        form.accountUsageCustomAllowedOriginsError
                          ? customOriginsErrorId
                          : undefined,
                        customSecurityWarningId
                      )}
                      aria-invalid={Boolean(form.accountUsageCustomAllowedOriginsError)}
                      mono
                      rows={4}
                      value={form.accountUsageCustomAllowedOrigins.join("\n")}
                      onChange={(event) =>
                        form.setAccountUsageCustomAllowedOrigins(
                          event.currentTarget.value.split(/\r?\n/)
                        )
                      }
                      placeholder="https://usage.example.com"
                      spellCheck={false}
                      autoComplete="off"
                      disabled={form.saving}
                      className="text-xs leading-5"
                    />
                    {form.accountUsageCustomAllowedOriginsError ? (
                      <p
                        id={customOriginsErrorId}
                        role="alert"
                        className="mt-1 text-xs text-rose-700 dark:text-rose-300"
                      >
                        {form.accountUsageCustomAllowedOriginsError}
                      </p>
                    ) : null}
                  </>
                )}
              </FormField>

              <FormField label="请求超时（秒）" hint="2-15s" className="min-w-0">
                {(id, hintId) => (
                  <Input
                    id={id}
                    aria-label="自定义账户用量请求超时"
                    aria-describedby={hintId}
                    type="number"
                    min={PROVIDER_ACCOUNT_USAGE_MIN_CUSTOM_TIMEOUT_SECONDS}
                    max={PROVIDER_ACCOUNT_USAGE_MAX_CUSTOM_TIMEOUT_SECONDS}
                    step={1}
                    inputMode="numeric"
                    value={form.accountUsageCustomTimeoutSeconds}
                    onChange={(event) => {
                      const next = event.currentTarget.valueAsNumber;
                      if (Number.isFinite(next)) form.setAccountUsageCustomTimeoutSeconds(next);
                    }}
                    disabled={form.saving}
                  />
                )}
              </FormField>
            </div>

            <div
              id={customSecurityWarningId}
              className="flex items-start gap-2 border-l-2 border-amber-500 bg-amber-50/70 px-3 py-2 text-xs text-amber-900 dark:bg-amber-950/20 dark:text-amber-200"
            >
              <ShieldAlert className="mt-0.5 h-4 w-4 shrink-0" aria-hidden="true" />
              <span>
                脚本和全部目标服务都可读取或转发当前供应商 API Key；仅信任已核对的脚本、Base URL
                和额外 HTTPS Origin。
              </span>
            </div>

            <div className="grid gap-3 sm:grid-cols-2">
              <FormField label="草稿测试" hint={customTestHint} className="min-w-0">
                {(_id, hintId) => (
                  <Button
                    type="button"
                    variant="secondary"
                    size="sm"
                    className="h-10"
                    aria-describedby={describedBy(hintId, customSecurityWarningId)}
                    aria-busy={form.accountUsageCustomTestInFlight}
                    onClick={() => void form.testAccountUsageCustomScript()}
                    disabled={
                      form.saving ||
                      form.accountUsageCustomTestInFlight ||
                      Boolean(form.accountUsageCustomAllowedOriginsError) ||
                      form.editingProviderId == null ||
                      !form.apiKeyConfigured
                    }
                  >
                    {form.accountUsageCustomTestInFlight ? (
                      <Loader2 className="mr-2 h-4 w-4 animate-spin" aria-hidden="true" />
                    ) : (
                      <Play className="mr-2 h-4 w-4" aria-hidden="true" />
                    )}
                    {form.accountUsageCustomTestInFlight ? "测试中…" : "测试脚本"}
                  </Button>
                )}
              </FormField>

              <FormField
                label="启用脚本"
                hint="保存时弹出系统确认；脚本、Base URL 或额外 Origin 变更后需重新确认"
                className="min-w-0"
              >
                {(id, hintId) => (
                  <div className="flex h-10 items-center justify-between gap-3 rounded-lg border border-line bg-surface-inset px-3">
                    <span
                      role="status"
                      aria-label="自定义账户用量确认状态"
                      aria-live="polite"
                      aria-atomic="true"
                      className="text-sm text-foreground"
                    >
                      {form.accountUsageCustomEnabled ? "已启用" : "未启用"}
                    </span>
                    <Switch
                      id={id}
                      size="sm"
                      checked={form.accountUsageCustomEnabled}
                      onCheckedChange={form.setAccountUsageCustomEnabled}
                      disabled={
                        form.saving ||
                        form.accountUsageCustomTestInFlight ||
                        Boolean(form.accountUsageCustomAllowedOriginsError) ||
                        !form.accountUsageCustomScript.trim()
                      }
                      aria-label="启用自定义账户用量脚本"
                      aria-describedby={describedBy(hintId, customSecurityWarningId)}
                    />
                  </div>
                )}
              </FormField>
            </div>

            {form.accountUsageCustomTestError ? (
              <div
                role="alert"
                className="border-l-2 border-rose-500 bg-rose-50/70 px-3 py-2 text-sm text-rose-800 dark:bg-rose-950/20 dark:text-rose-300"
              >
                {form.accountUsageCustomTestError}
              </div>
            ) : null}

            {form.accountUsageCustomTestResult ? (
              <div
                role="status"
                aria-label="自定义账户用量测试结果"
                aria-live="polite"
                aria-atomic="true"
                className={`space-y-1 border-l-2 px-3 py-2 text-sm ${accountUsageTestTone(form.accountUsageCustomTestResult.status)}`}
              >
                <div className="font-medium">
                  测试结果：{accountUsageStatusLabel(form.accountUsageCustomTestResult.status)}
                </div>
                {customTestDetails.length > 0 ? (
                  <div className="flex flex-wrap gap-x-3 gap-y-1 text-xs">
                    {customTestDetails.map((detail) => (
                      <span key={detail}>{detail}</span>
                    ))}
                  </div>
                ) : null}
                {form.accountUsageCustomTestResult.message ? (
                  <div className="text-xs">{form.accountUsageCustomTestResult.message}</div>
                ) : null}
              </div>
            ) : null}
          </div>
        ) : null}

        {accountMode || form.accountUsageCredentialsPresent ? (
          <div role="group" aria-label="账户用量凭据设置" className="grid gap-3 sm:grid-cols-2">
            {accountMode ? (
              <>
                <FormField label="User ID" className="min-w-0">
                  <Input
                    value={form.accountUsageNewApiUserId}
                    onChange={(event) =>
                      form.setAccountUsageNewApiUserId(event.currentTarget.value)
                    }
                    placeholder="正整数"
                    inputMode="numeric"
                    pattern="[0-9]*"
                    autoComplete="off"
                    disabled={form.saving}
                  />
                </FormField>

                <FormField label="系统访问令牌" hint={accessTokenHint} className="min-w-0">
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
              <FormField
                label="账户凭据"
                hint={accountMode ? undefined : "已保存，当前查询不会使用"}
                className="min-w-0"
              >
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
          </div>
        ) : null}

        {accountUsageEnabled ? (
          <div role="group" aria-label="账户用量刷新设置" className="grid gap-3 sm:grid-cols-3">
            <FormField label="定时刷新" className="min-w-0">
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

            <FormField label="路由门禁" className="min-w-0">
              <div className="flex h-10 items-center justify-between gap-3 rounded-lg border border-line bg-surface-inset px-3">
                <span className="text-sm text-foreground">启用</span>
                <Switch
                  size="sm"
                  checked={form.accountUsageRouteGateEnabled}
                  onCheckedChange={(next) => form.setAccountUsageRouteGateEnabled(next)}
                  disabled={form.saving}
                  aria-label="余额不足时跳过供应商路由"
                />
              </div>
            </FormField>

            <FormField label="刷新间隔（秒）" hint="60-300s" className="min-w-0">
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
                disabled={
                  form.saving ||
                  (!form.accountUsageTimedRefreshEnabled && !form.accountUsageRouteGateEnabled)
                }
              />
            </FormField>
          </div>
        ) : null}
      </div>
    </details>
  );
}
