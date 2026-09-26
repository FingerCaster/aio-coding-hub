import { useState } from "react";
import { Button } from "../../../ui/Button";
import { Input } from "../../../ui/Input";

/** Selection is local to the picker; adding never replaces existing drafts. */
export function NativeModelPicker({
  candidates,
  onAdd,
}: {
  candidates: string[];
  onAdd: (ids: string[]) => void;
}) {
  const [search, setSearch] = useState("");
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const visible = candidates.filter((id) => id.toLowerCase().includes(search.trim().toLowerCase()));
  const selectedIds = candidates.filter((id) => selected.has(id));
  if (!candidates.length) {
    return (
      <p
        role="status"
        className="rounded-lg border border-line px-3 py-2 text-xs text-muted-foreground"
      >
        暂无可添加候选，可刷新来源或手动添加模型。
      </p>
    );
  }
  return (
    <section
      aria-label="从来源批量添加模型"
      className="rounded-xl border border-line p-3 space-y-3"
    >
      <div className="flex flex-wrap items-center justify-between gap-2">
        <span className="text-sm font-medium">从来源添加模型（支持多选）</span>
        <span className="text-xs text-muted-foreground">
          已选 {selectedIds.length} / 可添加 {candidates.length}
        </span>
      </div>
      <Input
        aria-label="搜索来源模型"
        placeholder="搜索模型名称或 ID…"
        value={search}
        onChange={(e) => setSearch(e.target.value)}
      />
      <div className="flex flex-wrap gap-2">
        <Button
          type="button"
          size="sm"
          variant="secondary"
          disabled={!visible.length}
          onClick={() => setSelected((old) => new Set([...old, ...visible]))}
        >
          全选当前结果
        </Button>
        <Button
          type="button"
          size="sm"
          variant="secondary"
          disabled={!selectedIds.length}
          onClick={() => setSelected(new Set())}
        >
          清空选择
        </Button>
      </div>
      <div className="max-h-44 overflow-y-auto grid grid-cols-1 sm:grid-cols-2 gap-1.5">
        {visible.map((id) => (
          <label
            key={id}
            className="flex min-w-0 items-center gap-2 rounded-lg border border-line px-2.5 py-2 text-xs cursor-pointer hover:bg-surface-inset"
          >
            <input
              type="checkbox"
              aria-label={"选择来源模型 " + id}
              checked={selected.has(id)}
              className="h-4 w-4 shrink-0 accent-accent"
              onChange={(e) => {
                const checked = e.target.checked;
                setSelected((old) => {
                  const next = new Set(old);
                  if (checked) next.add(id);
                  else next.delete(id);
                  return next;
                });
              }}
            />
            <span className="truncate font-mono" title={id}>
              {id}
            </span>
          </label>
        ))}
        {!visible.length && (
          <p className="text-xs text-muted-foreground sm:col-span-2 py-2">
            {candidates.length ? "没有匹配的模型，请调整搜索。" : "暂无新候选，可刷新或手动添加。"}
          </p>
        )}
      </div>
      <div className="flex flex-wrap items-center justify-between gap-2">
        <p className="text-xs text-muted-foreground">
          批量填入已知容量、工具和思考参数，添加后仍可修改。
        </p>
        <Button
          type="button"
          size="sm"
          disabled={!selectedIds.length}
          onClick={() => {
            onAdd(selectedIds);
            setSelected(new Set());
          }}
        >
          添加所选模型（{selectedIds.length}）
        </Button>
      </div>
    </section>
  );
}
