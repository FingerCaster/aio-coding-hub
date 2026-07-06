import { FormField } from "../../ui/FormField";
import { Input } from "../../ui/Input";
import { Switch } from "../../ui/Switch";
import { RadioButtonGroup } from "./RadioButtonGroup";
import type { UseProviderEditorFormReturn } from "./useProviderEditorForm";
import {
  PROVIDER_ACCOUNT_USAGE_MAX_REFRESH_INTERVAL_SECONDS,
  PROVIDER_ACCOUNT_USAGE_MIN_REFRESH_INTERVAL_SECONDS,
  type ProviderAccountUsageAdapterKind,
} from "../../services/providers/providerAccountUsageConfig";

export function ProviderAccountUsageSection({ form }: { form: UseProviderEditorFormReturn }) {
  if (form.authMode !== "api_key") return null;
  const accountUsageEnabled = form.accountUsageAdapterKind !== "disabled";

  return (
    <div className="grid gap-3 sm:grid-cols-2">
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
        <FormField label="NewAPI User">
          <Input
            value={form.accountUsageNewApiUserId}
            onChange={(event) => form.setAccountUsageNewApiUserId(event.currentTarget.value)}
            placeholder="New-Api-User"
            disabled={form.saving}
          />
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
  );
}
