import { useEffect, useRef, useState } from "react";
import { useMutation } from "@tanstack/react-query";
import { toast } from "sonner";
import { ompAgentRead, ompAgentSave } from "../../../services/ompSettings";
import { confirmDesktopDialog } from "../../../services/desktop/confirm";
import { Dialog } from "../../../ui/Dialog";
import { Button } from "../../../ui/Button";
import { Input } from "../../../ui/Input";
import { Textarea } from "../../../ui/Textarea";
import { FormField } from "../../../ui/FormField";

const TEMPLATE =
  '---\nname: my-agent\ndescription: 描述这个 Agent 的用途\nmodel: "@task"\nthinking-level: auto\n# tools: read, find, grep, glob\n# spawns: scout\n# blocking: false\n# prewalk: false\n# advisor: false\n---\n\n在这里填写 Agent 的系统提示词。\n';

export function OmpAgentEditor({
  targetId,
  fileName: originalFileName,
  onClose,
  onSaved,
}: {
  targetId: string;
  fileName: string | null;
  onClose: () => void;
  onSaved: () => void;
}) {
  const [fileName, setFileName] = useState(originalFileName ?? "my-agent.md");
  const [content, setContent] = useState(originalFileName ? "" : TEMPLATE);
  const [original, setOriginal] = useState(originalFileName ? "" : TEMPLATE);
  const [revision, setRevision] = useState(originalFileName ? "" : "missing");
  const [loading, setLoading] = useState(Boolean(originalFileName));
  const [error, setError] = useState("");
  const mounted = useRef(true);
  const saving = useRef(false);
  const mutation = useMutation({
    mutationFn: (input: Parameters<typeof ompAgentSave>[0]) => ompAgentSave(input),
    retry: false,
  });
  useEffect(() => {
    mounted.current = true;
    let active = true;
    if (originalFileName) {
      void ompAgentRead(targetId, originalFileName)
        .then((doc) => {
          if (!active) return;
          if (!doc || doc.revision === "missing")
            throw new Error("Agent 文件已被移除，请关闭后刷新列表");
          setContent(doc.content);
          setOriginal(doc.content);
          setRevision(doc.revision);
        })
        .catch((e) => {
          if (active) setError(e instanceof Error ? e.message : "读取失败");
        })
        .finally(() => {
          if (active) setLoading(false);
        });
    }
    return () => {
      active = false;
      mounted.current = false;
    };
  }, [targetId, originalFileName]);
  async function close() {
    if (saving.current) return;
    if (content !== original && !(await confirmDesktopDialog("Agent 定义尚未保存，确认放弃修改？")))
      return;
    if (mounted.current) onClose();
  }
  async function save() {
    if (saving.current || !revision || loading) return;
    saving.current = true;
    setError("");
    try {
      await mutation.mutateAsync({ targetId, fileName, expectedRevision: revision, content });
      if (!mounted.current) return;
      toast.success("Agent 定义已保存；后续原生任务发现时生效");
      onSaved();
      onClose();
    } catch (e) {
      if (mounted.current) setError(e instanceof Error ? e.message : "保存失败");
    } finally {
      saving.current = false;
    }
  }
  return (
    <Dialog
      open
      title={originalFileName ? "编辑自定义 Agent" : "创建自定义 Agent"}
      onOpenChange={(open) => {
        if (!open) void close();
      }}
      className="max-w-3xl"
    >
      <div className="space-y-4">
        <p className="text-xs leading-relaxed text-muted-foreground">
          使用 OMP 原生 Markdown：上方 YAML 声明名称、说明、模型、工具、思考等级与可派生
          Agent，下方为系统提示词。未改动的高级字段会随原文保留。使用与内置 Agent 相同的 name
          会覆盖该内置定义。
        </p>
        <FormField label="Agent 文件名">
          {(id) => (
            <Input
              id={id}
              value={fileName}
              disabled={Boolean(originalFileName) || mutation.isPending}
              onChange={(e) => setFileName(e.target.value)}
            />
          )}
        </FormField>
        {loading ? (
          <p role="status">正在读取 Agent…</p>
        ) : (
          <FormField label="Agent 定义（Markdown）">
            {(id) => (
              <Textarea
                id={id}
                rows={18}
                className="font-mono text-xs leading-6"
                value={content}
                disabled={!revision || mutation.isPending}
                onChange={(e) => setContent(e.target.value)}
              />
            )}
          </FormField>
        )}
        {error && (
          <p role="alert" className="text-sm text-destructive">
            {error}
          </p>
        )}
        <div className="flex justify-end gap-2">
          <Button variant="secondary" disabled={mutation.isPending} onClick={() => void close()}>
            取消
          </Button>
          <Button
            disabled={
              loading || !revision || !fileName.trim() || !content.trim() || mutation.isPending
            }
            onClick={() => void save()}
          >
            {mutation.isPending ? "保存中…" : "保存 Agent 定义"}
          </Button>
        </div>
      </div>
    </Dialog>
  );
}
