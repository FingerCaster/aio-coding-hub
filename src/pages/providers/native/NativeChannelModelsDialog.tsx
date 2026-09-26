import { useEffect, useRef, useState } from "react";
import { AlertCircle, Code, Plus, RefreshCw, Sliders, Trash2 } from "lucide-react";
import type { NativeCliKey } from "../../../constants/clis";
import type {
  ChannelProvider,
  GatewayProtocol,
  ModelCapabilitySuggestion,
  NativeModelSpec,
} from "../../../services/nativeChannels";
import {
  useNativeChannelModelsDiscoverQuery,
  useNativeChannelModelsQuery,
  useNativeChannelModelsMutation,
} from "../../../query/nativeChannels";
import { Button } from "../../../ui/Button";
import { Dialog } from "../../../ui/Dialog";
import { FormField } from "../../../ui/FormField";
import { Input } from "../../../ui/Input";
import { Select } from "../../../ui/Select";
import { Textarea } from "../../../ui/Textarea";
import { parseGatewayModels } from "./nativeGatewayModelDraft";
import { nativeFailureMessage } from "./nativeProviderDraft";
import { NativeThinkingFields } from "./NativeThinkingFields";
import { NativeModelPicker } from "./NativeModelPicker";
import { fillModelSuggestion, type EditableModelItem } from "./nativeChannelModelSuggestions";
import { cn } from "../../../utils/cn";

interface DialogProps {
  client: NativeCliKey;
  targetId: string;
  provider: ChannelProvider;
  protocol: GatewayProtocol;
  onClose: () => void;
  onSaved: () => void;
}

function newModel(requestModelId = ""): EditableModelItem {
  return {
    id: crypto.randomUUID(),
    requestModelId,
    displayName: requestModelId,
    input: ["text"],
    contextWindow: "",
    maxTokens: "",
    supportsTools: null,
    reasoning: null,
    thinking: null,
    edited: {},
  };
}
function fromSpec(spec: NativeModelSpec): EditableModelItem {
  return {
    ...newModel(spec.requestModelId),
    ...spec,
    input: spec.input as ("text" | "image")[],
    edited: {
      displayName: true,
      input: true,
      contextWindow: true,
      maxTokens: true,
      supportsTools: true,
      reasoning: true,
      thinking: true,
    },
  };
}
function toSpec(item: EditableModelItem): NativeModelSpec {
  return {
    requestModelId: item.requestModelId.trim(),
    displayName: (item.displayName || item.requestModelId).trim(),
    input: item.input,
    contextWindow: item.contextWindow || 0,
    maxTokens: item.maxTokens || 0,
    supportsTools: item.supportsTools,
    reasoning: item.reasoning === true,
    thinking: item.reasoning ? item.thinking : null,
  };
}
function parseChannelModels(raw: string, client: NativeCliKey) {
  const models = parseGatewayModels(raw, client);
  if (models.some((model) => model.supportsTools === null))
    throw new Error(
      client === "omp"
        ? "OMP 发布模型 tools 必须明确为 true 或 false，不可为 null。"
        : "Pi 发布模型必须明确确认支持工具调用 (supportsTools=true)。"
    );
  return models;
}

export function NativeChannelModelsDialog(props: DialogProps) {
  const identity = [
    props.targetId,
    props.provider.providerId,
    props.provider.providerUuid,
    props.protocol,
  ].join(":");
  return <NativeChannelModelsEditor key={identity} {...props} />;
}

function NativeChannelModelsEditor({
  client,
  targetId,
  provider,
  protocol,
  onClose,
  onSaved,
}: DialogProps) {
  const query = useNativeChannelModelsQuery(
    targetId,
    provider.providerId,
    provider.providerUuid,
    protocol
  );
  const mutation = useNativeChannelModelsMutation(
    targetId,
    provider.providerId,
    provider.providerUuid,
    protocol
  );
  const discovery = useNativeChannelModelsDiscoverQuery(
    targetId,
    provider.providerId,
    provider.providerUuid,
    protocol,
    query.data != null && !query.isError && !provider.blockedReason
  );
  const [mode, setMode] = useState<"form" | "json">("form");
  const [items, setItems] = useState<EditableModelItem[]>([]);
  const [rawJson, setRawJson] = useState("");
  const [revision, setRevision] = useState("");
  const [initialized, setInitialized] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [suggestions, setSuggestions] = useState<{
    revision: string;
    models: ModelCapabilitySuggestion[];
  }>({ revision: "", models: [] });
  const alive = useRef(true);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);
  useEffect(() => {
    if (!query.data || initialized) return;
    setItems(query.data.models.map(fromSpec));
    setRawJson(JSON.stringify(query.data.models, null, 2));
    setRevision(query.data.revision);
    setInitialized(true);
  }, [query.data, initialized]);
  useEffect(() => {
    const result = discovery.data;
    if (!result || result.revision !== revision) return;
    setSuggestions((previous) => {
      const all = new Map(
        (previous.revision === revision ? previous.models : []).map((model) => [
          model.modelId,
          model,
        ])
      );
      result.models.forEach((model) => all.set(model.modelId, model));
      return { revision, models: [...all.values()] };
    });
  }, [discovery.data, revision]);
  useEffect(() => {
    if (!initialized || mode !== "form" || suggestions.revision !== revision) return;
    const all = new Map(suggestions.models.map((model) => [model.modelId, model]));
    setItems((previous) =>
      previous.map((item) =>
        fillModelSuggestion(item, all.get(item.requestModelId.trim()), client, protocol)
      )
    );
  }, [suggestions, initialized, mode, revision, client, protocol]);

  const stale = initialized && query.data != null && revision !== query.data.revision;
  const blocked =
    !initialized ||
    query.isError ||
    !query.data ||
    mutation.isPending ||
    stale ||
    Boolean(provider.blockedReason);
  const currentSuggestions = suggestions.revision === revision ? suggestions.models : [];
  const existing = new Set(items.map((item) => item.requestModelId.trim()));
  const allIds = [
    ...new Set([...provider.modelIds, ...currentSuggestions.map((model) => model.modelId)]),
  ]
    .filter((id) => id.trim() && !id.includes("*") && !id.startsWith("aio/"))
    .sort();
  const candidateIds = allIds.filter((id) => !existing.has(id));
  const status = discovery.data?.discovery.status;
  const discoveryMessage = discovery.isFetching
    ? "正在自动获取模型与能力…"
    : discovery.isError || status === "error"
      ? "自动获取失败，已保留当前候选和编辑内容，可重试或手动补充。"
      : status === "unsupported"
        ? "该来源暂未提供自动发现接口，已复用可用的来源配置，其余请手动补充。"
        : status === "empty"
          ? "上游暂无可获取的模型，仍可从已有候选选择或手动添加。"
          : status === "ready"
            ? "已获取 " +
              currentSuggestions.length +
              " 个模型；支持批量选择，已知参数自动填写，可随时修改。"
            : "将自动获取来源模型与能力。";

  function addModel(id = "") {
    setError(null);
    setItems((previous) => [
      ...previous,
      fillModelSuggestion(
        newModel(id),
        currentSuggestions.find((model) => model.modelId === id),
        client,
        protocol
      ),
    ]);
  }
  function addModels(ids: string[]) {
    setError(null);
    setItems((previous) => {
      const known = new Set(previous.map((item) => item.requestModelId.trim()));
      const additions = [...new Set(ids)].filter(
        (id) => candidateIds.includes(id) && !known.has(id)
      );
      return [
        ...previous,
        ...additions.map((id) =>
          fillModelSuggestion(
            newModel(id),
            currentSuggestions.find((model) => model.modelId === id),
            client,
            protocol
          )
        ),
      ];
    });
  }
  function updateItem(id: string, patch: Partial<EditableModelItem>) {
    setError(null);
    setItems((previous) =>
      previous.map((item) => {
        if (item.id !== id) return item;
        if (patch.requestModelId !== undefined && patch.requestModelId !== item.requestModelId) {
          const fresh = { ...newModel(patch.requestModelId), id };
          return fillModelSuggestion(
            fresh,
            currentSuggestions.find((model) => model.modelId === patch.requestModelId?.trim()),
            client,
            protocol
          );
        }
        return {
          ...item,
          ...patch,
          edited: {
            ...item.edited,
            ...Object.fromEntries(Object.keys(patch).map((field) => [field, true])),
          },
        };
      })
    );
  }
  function switchMode(next: "form" | "json") {
    if (next === mode) return;
    setError(null);
    if (next === "json") {
      setRawJson(
        JSON.stringify(
          items.map((item) => ({ ...toSpec(item), reasoning: item.reasoning })),
          null,
          2
        )
      );
      setMode(next);
    } else {
      try {
        setItems(parseChannelModels(rawJson, client).map(fromSpec));
        setMode(next);
      } catch (cause) {
        setError(cause instanceof Error ? cause.message : "JSON 格式无效。");
      }
    }
  }
  async function save() {
    if (blocked) return;
    setError(null);
    let submitted = false;
    try {
      if (mode === "form")
        for (const item of items) {
          const id = item.requestModelId.trim();
          if (!id) throw new Error("模型 ID 不能为空。");
          if (!Number.isInteger(item.contextWindow) || Number(item.contextWindow) <= 0)
            throw new Error("模型 " + id + " 的上下文窗口必须填写正整数容量。");
          if (!Number.isInteger(item.maxTokens) || Number(item.maxTokens) <= 0)
            throw new Error("模型 " + id + " 的最大输出 Token 必须填写正整数。");
          if (typeof item.supportsTools !== "boolean" || (client === "pi" && !item.supportsTools))
            throw new Error(
              "模型 " + id + " 的工具调用必须明确为 true 或 false；Pi 必须支持工具调用。"
            );
          if (typeof item.reasoning !== "boolean")
            throw new Error("模型 " + id + " 的推理能力必须明确选择支持或不支持。");
          if (item.reasoning && !item.thinking)
            throw new Error("请配置模型 " + id + " 的思考等级。");
        }
      const models = parseChannelModels(
        mode === "json" ? rawJson : JSON.stringify(items.map(toSpec)),
        client
      );
      submitted = true;
      await mutation.mutateAsync({ expectedRevision: revision, models });
      if (alive.current) {
        onSaved();
        onClose();
      }
    } catch (cause) {
      if (alive.current)
        setError(
          !submitted && cause instanceof Error ? cause.message : nativeFailureMessage(cause)
        );
    }
  }
  async function reload() {
    const result = await query.refetch();
    if (!alive.current || !result.isSuccess || !result.data) return;
    setItems(result.data.models.map(fromSpec));
    setRawJson(JSON.stringify(result.data.models, null, 2));
    setRevision(result.data.revision);
    setInitialized(true);
    setError(null);
  }

  return (
    <Dialog
      open
      title="来源上游模型能力声明"
      description={provider.name + " · " + protocol + "。选择模型，自动填写可获取的能力。"}
      className="max-w-3xl"
      onOpenChange={(open) => {
        if (!open && !mutation.isPending) onClose();
      }}
    >
      <div className="space-y-4">
        <div className="rounded-xl border border-line bg-surface-inset/40 p-3 space-y-2">
          <div className="flex items-start justify-between gap-3">
            <div className="space-y-1">
              <p className="text-sm font-medium">自动获取来源能力</p>
              <p role="status" className="text-xs text-muted-foreground">
                {discoveryMessage}
              </p>
            </div>
            <Button
              type="button"
              variant="secondary"
              size="sm"
              disabled={
                discovery.isFetching ||
                mutation.isPending ||
                !query.data ||
                Boolean(provider.blockedReason)
              }
              onClick={() => void discovery.refetch()}
              className="shrink-0 gap-1"
            >
              <RefreshCw
                className={cn("h-3.5 w-3.5", discovery.isFetching && "animate-spin")}
                aria-hidden="true"
              />
              刷新模型与能力
            </Button>
          </div>
          <p className="text-xs text-muted-foreground">
            自动填写仅用于当前草稿，手动修改优先；确认保存后生效。
          </p>
        </div>
        {stale || (discovery.data && discovery.data.revision !== revision) ? (
          <p role="alert" className="text-xs text-amber-700">
            模型声明或来源配置已变化，请重新读取后刷新能力并核对。
          </p>
        ) : null}
        {query.data?.stale && (
          <p role="alert" className="text-xs text-amber-700">
            上游协议或配置已改变。现有声明已过期，请核对后保存重新绑定。
          </p>
        )}
        {query.isError && (
          <p role="alert" className="text-xs text-destructive">
            读取模型声明失败；已禁用保存，不能用空数组覆盖。
          </p>
        )}
        {error && (
          <div
            role="alert"
            className="flex items-center gap-2 rounded-lg border border-destructive/30 bg-destructive/5 p-3 text-xs text-destructive"
          >
            <AlertCircle className="h-4 w-4 shrink-0" aria-hidden="true" />
            {error}
          </div>
        )}
        {query.isPending ? (
          <p role="status" className="py-6 text-center text-sm text-muted-foreground">
            读取渠道模型声明…
          </p>
        ) : (
          <fieldset disabled={blocked} className="space-y-4 min-w-0">
            <div className="flex flex-wrap items-center justify-between gap-2 border-b border-line pb-2">
              <div role="tablist" aria-label="编辑模式" className="flex gap-1">
                {(["form", "json"] as const).map((tab) => (
                  <button
                    type="button"
                    role="tab"
                    key={tab}
                    aria-selected={mode === tab}
                    onClick={() => switchMode(tab)}
                    className={cn(
                      "inline-flex items-center gap-1.5 rounded-lg px-3 py-1.5 text-xs font-medium",
                      mode === tab
                        ? "bg-accent text-white"
                        : "text-muted-foreground hover:bg-surface-inset"
                    )}
                  >
                    {tab === "form" ? (
                      <Sliders className="h-3.5 w-3.5" />
                    ) : (
                      <Code className="h-3.5 w-3.5" />
                    )}
                    {tab === "form" ? "表单编辑 (推荐)" : "高级 JSON 编辑"}
                  </button>
                ))}
              </div>
              <span className="text-xs text-muted-foreground">
                已配置 {items.length} 个模型声明
              </span>
            </div>
            {mode === "form" ? (
              <div className="space-y-4">
                <NativeModelPicker candidates={candidateIds} onAdd={addModels} />
                <datalist id="native-channel-model-options">
                  {allIds.map((id) => (
                    <option key={id} value={id} />
                  ))}
                </datalist>
                {!items.length && (
                  <div className="rounded-xl border border-dashed border-line p-5 text-center text-xs text-muted-foreground">
                    请选择来源模型，或手动添加自定义模型。
                  </div>
                )}
                <div className="max-h-[50vh] space-y-3 overflow-y-auto pr-1">
                  {items.map((item, index) => {
                    const label = "模型 " + (index + 1);
                    const suggestion = currentSuggestions.find(
                      (model) => model.modelId === item.requestModelId.trim()
                    );
                    const missing = [
                      item.contextWindow === "",
                      item.maxTokens === "",
                      item.supportsTools === null,
                      item.reasoning === null,
                      item.reasoning === true &&
                        (!item.thinking ||
                          (item.thinking.client === "omp"
                            ? !item.thinking.mode || !item.thinking.efforts.length
                            : !Object.entries(item.thinking.levelMap).some(
                                ([level, value]) => level !== "off" && Boolean(value)
                              ))),
                    ].filter(Boolean).length;
                    return (
                      <div
                        key={item.id}
                        className="rounded-xl border border-line bg-surface-panel/40 p-3.5 space-y-3"
                      >
                        <div className="flex items-center justify-between gap-2 border-b border-line pb-2">
                          <div className="min-w-0">
                            <p className="truncate text-sm font-medium">
                              {item.displayName || item.requestModelId || label}
                            </p>
                            <p className="text-xs text-muted-foreground">
                              {suggestion?.sources.includes("routing_confirmation")
                                ? "模型或思考等级存在路由改写，请核对实际路由后的能力"
                                : suggestion
                                  ? suggestion.sources.includes("configured")
                                    ? "已复用来源配置与可获取能力"
                                    : suggestion.sources.includes("upstream_conflict")
                                      ? "上游返回了冲突能力，请手动核对"
                                      : suggestion.sources.some((source) =>
                                            source.includes("_catalog:")
                                          )
                                        ? "已用 Pi / OMP 内置目录补全默认参数，可修改"
                                        : "已自动填入上游返回的能力"
                                  : "手动配置"}
                              {missing ? " · 还有 " + missing + " 项必填能力待补充" : ""}
                            </p>
                          </div>
                          <Button
                            type="button"
                            variant="danger"
                            size="sm"
                            aria-label={"移除模型 " + (item.requestModelId || index + 1)}
                            onClick={() =>
                              setItems((previous) =>
                                previous.filter((model) => model.id !== item.id)
                              )
                            }
                          >
                            <Trash2 className="h-3.5 w-3.5" />
                          </Button>
                        </div>
                        <p className="text-xs text-muted-foreground">
                          上下文：{item.contextWindow || "待补充"} · 输出：
                          {item.maxTokens || "待补充"}
                          {item.thinking?.client === "omp" && item.thinking.defaultLevel
                            ? " · 默认思考：" + item.thinking.defaultLevel
                            : item.reasoning === true
                              ? item.thinking
                                ? " · 思考参数已配置"
                                : " · 思考参数待补充"
                              : ""}
                        </p>
                        <details open={items.length === 1 || missing > 0} className="space-y-3">
                          <summary className="cursor-pointer text-xs font-medium text-accent">
                            修改模型参数
                          </summary>
                          <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
                            <label className="space-y-1 text-xs">
                              <span>模型 ID (requestModelId)</span>
                              <Input
                                aria-label={label + " 模型 ID"}
                                list="native-channel-model-options"
                                value={item.requestModelId}
                                placeholder="如 gpt-4o 或自定义模型 ID"
                                onChange={(e) =>
                                  updateItem(item.id, { requestModelId: e.target.value })
                                }
                              />
                            </label>
                            <label className="space-y-1 text-xs">
                              <span>显示名称</span>
                              <Input
                                aria-label={label + " 显示名称"}
                                value={item.displayName}
                                onChange={(e) =>
                                  updateItem(item.id, { displayName: e.target.value })
                                }
                              />
                            </label>
                            <label className="space-y-1 text-xs">
                              <span>上下文窗口</span>
                              <Input
                                aria-label={label + " 上下文窗口"}
                                type="number"
                                min={1}
                                max={10000000}
                                step={1}
                                value={item.contextWindow}
                                placeholder="必填，如 128000；自动获取或补充"
                                onChange={(e) =>
                                  updateItem(item.id, {
                                    contextWindow: e.target.value ? Number(e.target.value) : "",
                                  })
                                }
                              />
                            </label>
                            <label className="space-y-1 text-xs">
                              <span>最大输出 Token</span>
                              <Input
                                aria-label={label + " 最大输出 Token"}
                                type="number"
                                min={1}
                                step={1}
                                value={item.maxTokens}
                                placeholder="必填，如 4096；不超过上下文"
                                onChange={(e) =>
                                  updateItem(item.id, {
                                    maxTokens: e.target.value ? Number(e.target.value) : "",
                                  })
                                }
                              />
                            </label>
                            <label className="space-y-1 text-xs">
                              <span>输入类型</span>
                              <Select
                                aria-label={label + " 输入类型"}
                                value={item.input.includes("image") ? "image" : "text"}
                                onChange={(e) =>
                                  updateItem(item.id, {
                                    input:
                                      e.target.value === "image" ? ["text", "image"] : ["text"],
                                  })
                                }
                              >
                                <option value="text">文本</option>
                                <option value="image">文本与图像</option>
                              </Select>
                            </label>
                            <label className="space-y-1 text-xs">
                              <span>工具调用</span>
                              <Select
                                aria-label={label + " 工具调用"}
                                value={
                                  item.supportsTools === null ? "" : String(item.supportsTools)
                                }
                                onChange={(e) =>
                                  updateItem(item.id, {
                                    supportsTools:
                                      e.target.value === "" ? null : e.target.value === "true",
                                  })
                                }
                              >
                                <option value="">请选择工具能力</option>
                                <option value="true">支持工具调用</option>
                                <option value="false">
                                  不支持工具调用{client === "pi" ? "（Pi 无法发布）" : ""}
                                </option>
                              </Select>
                            </label>
                            <label className="space-y-1 text-xs">
                              <span>推理能力</span>
                              <Select
                                aria-label={label + " 推理能力"}
                                value={item.reasoning === null ? "" : String(item.reasoning)}
                                onChange={(e) => {
                                  const reasoning =
                                    e.target.value === "" ? null : e.target.value === "true";
                                  updateItem(item.id, {
                                    reasoning,
                                    thinking: reasoning ? item.thinking : null,
                                  });
                                }}
                              >
                                <option value="">请选择推理能力</option>
                                <option value="false">不支持推理</option>
                                <option value="true">支持推理</option>
                              </Select>
                            </label>
                          </div>
                          {item.reasoning && (
                            <NativeThinkingFields
                              client={client}
                              value={item.thinking}
                              supported={suggestion?.reasoningEfforts}
                              label={label}
                              onChange={(thinking) => updateItem(item.id, { thinking })}
                            />
                          )}
                        </details>
                      </div>
                    );
                  })}
                </div>
                <Button
                  type="button"
                  variant="secondary"
                  size="sm"
                  onClick={() => addModel()}
                  className="gap-1"
                >
                  <Plus className="h-3.5 w-3.5" />
                  添加模型待填项
                </Button>
              </div>
            ) : (
              <FormField label="模型能力声明 JSON" hint="允许空数组明确撤销所有模型声明">
                {(id) => (
                  <Textarea
                    id={id}
                    rows={14}
                    spellCheck={false}
                    className="font-mono text-xs"
                    value={rawJson}
                    onChange={(e) => {
                      setRawJson(e.target.value);
                      setError(null);
                    }}
                  />
                )}
              </FormField>
            )}
          </fieldset>
        )}
        <div className="flex items-center justify-between gap-2 border-t border-line pt-3">
          <Button
            type="button"
            variant="secondary"
            size="sm"
            disabled={query.isFetching || mutation.isPending}
            onClick={() => void reload()}
          >
            重新读取
          </Button>
          <div className="flex gap-2">
            <Button
              type="button"
              variant="secondary"
              disabled={mutation.isPending}
              onClick={onClose}
            >
              取消
            </Button>
            <Button type="button" disabled={blocked} onClick={() => void save()}>
              {mutation.isPending ? "保存中…" : "保存模型声明"}
            </Button>
          </div>
        </div>
      </div>
    </Dialog>
  );
}
