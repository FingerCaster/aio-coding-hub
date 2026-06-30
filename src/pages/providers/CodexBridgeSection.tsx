import { FormField } from "../../ui/FormField";
import { Input } from "../../ui/Input";
import { Select } from "../../ui/Select";
import { TabList } from "../../ui/TabList";
import { TagsField } from "./TagsField";
import type { UseProviderEditorFormReturn } from "./useProviderEditorForm";

export function CodexBridgeSection({ form }: { form: UseProviderEditorFormReturn }) {
  const { register, saving, tags, setTags, tagInput, setTagInput } = form;
  const sourceCliKey = form.codexBridgeTarget === "anthropic_messages" ? "claude" : "codex";
  const sourceOptions = form.codexBridgeSourceProviders
    .filter(
      (provider) =>
        provider.enabled &&
        provider.cli_key === sourceCliKey &&
        provider.id !== form.editingProviderId &&
        provider.source_provider_id == null &&
        !provider.bridge_type
    )
    .map((provider) => ({
      value: String(provider.id),
      label: provider.name,
    }));

  return (
    <div className="space-y-4">
      <div className="grid gap-3 sm:grid-cols-2">
        <FormField label="名称">
          {(id) => {
            const field = register("name");
            return <Input id={id} placeholder="default" {...field} />;
          }}
        </FormField>

        <TagsField
          tags={tags}
          setTags={setTags}
          tagInput={tagInput}
          setTagInput={setTagInput}
          saving={saving}
        />
      </div>

      <FormField label="备注">
        {(id) => {
          const field = register("note");
          return <Input id={id} placeholder="可选备注信息" disabled={saving} {...field} />;
        }}
      </FormField>

      <div className="space-y-4 rounded-lg border border-border bg-secondary/40 p-4">
        <FormField label="上游端点">
          <TabList<"openai_chat" | "anthropic_messages">
            ariaLabel="上游端点"
            items={[
              { key: "openai_chat", label: "Chat Completions" },
              { key: "anthropic_messages", label: "Anthropic Messages" },
            ]}
            value={form.codexBridgeTarget}
            onChange={form.setCodexBridgeTarget}
          />
        </FormField>

        <FormField label="上游来源" hint="使用所选普通 Provider 的 Base URL 和凭据">
          {(id) => (
            <Select
              id={id}
              value={form.cx2ccSourceValue}
              onChange={(event) => form.setCx2ccSourceValue(event.currentTarget.value)}
              disabled={form.saving}
            >
              <option value="">
                {sourceCliKey === "claude" ? "选择 Claude 上游来源" : "选择 Codex 上游来源"}
              </option>
              {sourceOptions.map((option) => (
                <option key={option.value} value={option.value}>
                  {option.label}
                </option>
              ))}
            </Select>
          )}
        </FormField>
      </div>
    </div>
  );
}
