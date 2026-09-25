// Usage: Collects the model + prompt used by the provider availability probe.
// Suggestions are read-only; the probe call and its loading state stay in the view model.

import { useEffect, useId, useRef, useState } from "react";
import type { ProviderSummary } from "../../services/providers/providers";
import { Button } from "../../ui/Button";
import { Dialog } from "../../ui/Dialog";
import { FormField } from "../../ui/FormField";
import { Input } from "../../ui/Input";
import {
  DEFAULT_PROBE_PROMPT,
  defaultProbeModel,
  probeModelCandidates,
} from "./providerProbeDefaults";

export function ProviderTestDialog({
  provider,
  testing,
  catalogModels = [],
  onDiscover,
  onClose,
  onConfirm,
}: {
  provider: ProviderSummary | null;
  testing: boolean;
  catalogModels?: string[];
  onDiscover?: () => Promise<string[]>;
  onClose: () => void;
  onConfirm: (input: { model: string; prompt: string }) => void;
}) {
  const candidateListId = useId();
  const [model, setModel] = useState("");
  const [discovered, setDiscovered] = useState<string[]>([]);
  const [discovering, setDiscovering] = useState(false);
  const [discoveryMessage, setDiscoveryMessage] = useState("");
  const generation = useRef(0);
  const [prompt, setPrompt] = useState(DEFAULT_PROBE_PROMPT);

  // Reseed per provider; the dialog intentionally keeps no draft between openings.
  useEffect(() => {
    generation.current += 1;
    setDiscovered([]);
    setDiscovering(false);
    setDiscoveryMessage("");
    if (!provider) return;
    setModel(defaultProbeModel(provider));
    setPrompt(DEFAULT_PROBE_PROMPT);
    return () => {
      generation.current += 1;
    };
  }, [provider]);

  const candidates = provider
    ? probeModelCandidates(provider, [...catalogModels, ...discovered])
    : [];

  return (
    <Dialog
      open={!!provider}
      onOpenChange={(nextOpen) => {
        if (!nextOpen && !testing) onClose();
      }}
      title="测试供应商可用性"
      description={provider ? `将测试：${provider.name}` : undefined}
      className="max-w-lg"
    >
      <div className="space-y-3">
        <datalist id={candidateListId}>
          {candidates.map((candidate) => (
            <option key={candidate} value={candidate} />
          ))}
        </datalist>

        <FormField label="模型" hint="留空则使用该供应商已配置的模型">
          {(id) => (
            <Input
              id={id}
              list={candidateListId}
              value={model}
              onChange={(event) => setModel(event.currentTarget.value)}
              placeholder="例如: deepseek-v4-flash"
              disabled={testing}
            />
          )}
        </FormField>

        {onDiscover && (
          <div className="space-y-1">
            <Button
              variant="secondary"
              disabled={testing || discovering}
              onClick={async () => {
                if (discovering) return;
                const requestGeneration = generation.current;
                setDiscovering(true);
                setDiscoveryMessage("");
                try {
                  const models = await onDiscover();
                  if (requestGeneration !== generation.current) return;
                  setDiscovered(models);
                  setDiscoveryMessage(
                    models.length
                      ? "模型候选已更新，可在模型输入框中选择。"
                      : "未发现模型，可手动输入。"
                  );
                } catch (error) {
                  if (requestGeneration === generation.current)
                    setDiscoveryMessage(error instanceof Error ? error.message : "模型发现失败");
                } finally {
                  if (requestGeneration === generation.current) setDiscovering(false);
                }
              }}
            >
              {discovering ? "获取中…" : "获取模型候选"}
            </Button>
            {discoveryMessage && (
              <p role="status" className="text-xs text-slate-500">
                {discoveryMessage}
              </p>
            )}
          </div>
        )}

        <FormField label="提示词" hint={`留空则使用默认 ${DEFAULT_PROBE_PROMPT}`}>
          {(id) => (
            <Input
              id={id}
              value={prompt}
              onChange={(event) => setPrompt(event.currentTarget.value)}
              placeholder={DEFAULT_PROBE_PROMPT}
              disabled={testing}
            />
          )}
        </FormField>

        <div className="flex flex-wrap items-center justify-end gap-2">
          <Button onClick={onClose} variant="secondary" disabled={testing}>
            取消
          </Button>
          <Button onClick={() => onConfirm({ model, prompt })} variant="primary" disabled={testing}>
            {testing ? "测试中…" : "开始测试"}
          </Button>
        </div>
      </div>
    </Dialog>
  );
}
