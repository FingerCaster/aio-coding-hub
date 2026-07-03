import { Plus, Trash2 } from "lucide-react";
import { FormField } from "../../ui/FormField";
import { Input } from "../../ui/Input";
import { Select } from "../../ui/Select";
import { TabList } from "../../ui/TabList";
import { TagsField } from "./TagsField";
import type { UseProviderEditorFormReturn } from "./useProviderEditorForm";
import type { CodexBridgeTarget } from "./providerEditorUtils";

export function CodexBridgeSection({ form }: { form: UseProviderEditorFormReturn }) {
  const { register, saving, tags, setTags, tagInput, setTagInput } = form;
  const exactEntries = Object.entries(form.modelMapping.exact ?? {});
  const sourceOptions = form.codexBridgeSourceProviders
    .filter(
      (provider) =>
        provider.enabled &&
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
          <TabList<CodexBridgeTarget>
            ariaLabel="上游端点"
            items={[
              { key: "openai_responses", label: "Responses" },
              { key: "openai_chat", label: "Chat Completions" },
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
              <option value="">选择上游来源</option>
              {sourceOptions.map((option) => (
                <option key={option.value} value={option.value}>
                  {option.label}
                </option>
              ))}
            </Select>
          )}
        </FormField>

        <FormField label="默认上游模型" hint="未命中精确映射时使用，例如 deepseek-reasoner">
          {(id) => (
            <Input
              id={id}
              value={form.modelMapping.default_model ?? ""}
              onChange={(event) =>
                form.setModelMapping({
                  ...form.modelMapping,
                  default_model: event.currentTarget.value,
                })
              }
              placeholder="deepseek-reasoner"
              disabled={saving}
            />
          )}
        </FormField>

        <div className="space-y-3">
          <div className="flex items-center justify-between gap-3">
            <div className="text-sm font-medium text-foreground">精确模型映射</div>
            <button
              type="button"
              className="inline-flex h-8 items-center gap-1 rounded-md border border-border bg-background px-2 text-xs font-medium text-foreground transition-colors hover:bg-muted disabled:cursor-not-allowed disabled:opacity-50"
              onClick={() => addModelMappingRow(form)}
              disabled={saving}
            >
              <Plus className="h-3.5 w-3.5" aria-hidden="true" />
              添加
            </button>
          </div>

          <div className="space-y-2">
            {exactEntries.length === 0 ? (
              <div className="rounded-md border border-dashed border-border bg-background/50 px-3 py-3 text-sm text-muted-foreground">
                可按 Codex 请求模型精确映射到上游模型，例如 gpt-5.5 到 deepseek-chat。
              </div>
            ) : (
              exactEntries.map(([sourceModel, targetModel], index) => (
                <div
                  key={`${sourceModel}-${index}`}
                  className="grid gap-2 sm:grid-cols-[minmax(0,1fr)_minmax(0,1fr)_2rem]"
                >
                  <Input
                    aria-label={`Codex 模型 ${index + 1}`}
                    value={sourceModel}
                    onChange={(event) =>
                      updateModelMappingSource(form, index, event.currentTarget.value)
                    }
                    placeholder="gpt-5.5"
                    disabled={saving}
                  />
                  <Input
                    aria-label={`上游模型 ${index + 1}`}
                    value={targetModel}
                    onChange={(event) =>
                      updateModelMappingTarget(form, sourceModel, event.currentTarget.value)
                    }
                    placeholder="deepseek-chat"
                    disabled={saving}
                  />
                  <button
                    type="button"
                    className="inline-flex h-10 w-10 items-center justify-center rounded-md border border-border bg-background text-muted-foreground transition-colors hover:bg-muted hover:text-foreground disabled:cursor-not-allowed disabled:opacity-50"
                    onClick={() => removeModelMappingRow(form, sourceModel)}
                    disabled={saving}
                    aria-label={`删除模型映射 ${index + 1}`}
                    title="删除模型映射"
                  >
                    <Trash2 className="h-4 w-4" aria-hidden="true" />
                  </button>
                </div>
              ))
            )}
          </div>
        </div>
      </div>
    </div>
  );
}

function addModelMappingRow(form: UseProviderEditorFormReturn) {
  const exact = { ...(form.modelMapping.exact ?? {}) };
  let key = "";
  let index = 1;
  while (Object.prototype.hasOwnProperty.call(exact, key)) {
    key = `gpt-5.5 ${index}`;
    index += 1;
  }
  exact[key] = "";
  form.setModelMapping({ ...form.modelMapping, exact });
}

function updateModelMappingSource(
  form: UseProviderEditorFormReturn,
  rowIndex: number,
  nextSourceModel: string
) {
  const entries = Object.entries(form.modelMapping.exact ?? {});
  if (!entries[rowIndex]) return;
  entries[rowIndex] = [nextSourceModel, entries[rowIndex][1]];
  form.setModelMapping({
    ...form.modelMapping,
    exact: Object.fromEntries(entries),
  });
}

function updateModelMappingTarget(
  form: UseProviderEditorFormReturn,
  sourceModel: string,
  nextTargetModel: string
) {
  form.setModelMapping({
    ...form.modelMapping,
    exact: {
      ...(form.modelMapping.exact ?? {}),
      [sourceModel]: nextTargetModel,
    },
  });
}

function removeModelMappingRow(form: UseProviderEditorFormReturn, sourceModel: string) {
  const exact = { ...(form.modelMapping.exact ?? {}) };
  delete exact[sourceModel];
  form.setModelMapping({ ...form.modelMapping, exact });
}
