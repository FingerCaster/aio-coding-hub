import { FormField } from "../../ui/FormField";
import { Input } from "../../ui/Input";
import { RadioButtonGroup } from "./RadioButtonGroup";
import type { UseProviderEditorFormReturn } from "./useProviderEditorForm";
import type { ProviderAccountUsageAdapterKind } from "../../services/providers/providerAccountUsageConfig";

export function ProviderAccountUsageSection({ form }: { form: UseProviderEditorFormReturn }) {
  if (form.authMode !== "api_key") return null;

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
    </div>
  );
}
