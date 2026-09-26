import { useState } from "react";
import { render, screen, fireEvent } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { JsonValue } from "../../../../generated/bindings";
import { NativeModelFields } from "../NativeModelFields";

describe("NativeModelFields", () => {
  it("edits real capability fields while retaining Pi mappings and unknown fields", () => {
    const changed = vi.fn();
    function Form() {
      const [models, setModels] = useState<JsonValue[]>([
        { id: "explicit", thinkingLevelMap: { high: "medium" }, future: true },
      ]);
      return (
        <NativeModelFields
          client="pi"
          models={models}
          disabled={false}
          onChange={(next) => {
            changed(next);
            setModels(next);
          }}
        />
      );
    }
    render(<Form />);
    fireEvent.change(screen.getByLabelText("上下文窗口"), { target: { value: "64000" } });
    expect(changed).toHaveBeenLastCalledWith([
      { id: "explicit", thinkingLevelMap: { high: "medium" }, future: true, contextWindow: 64000 },
    ]);
    expect(screen.getByLabelText("思考能力")).toHaveValue("unset");
    expect(screen.getByLabelText("输入类型")).toHaveValue("");
  });

  it("shows OMP thinking semantics and refuses an unsupported model shape", () => {
    const { rerender } = render(
      <NativeModelFields client="omp" models={[]} disabled={false} onChange={vi.fn()} />
    );
    expect(screen.getByText(/OMP 使用 thinking 对象/)).toBeInTheDocument();
    rerender(
      <NativeModelFields
        client="omp"
        models={{ opaque: true }}
        disabled={false}
        onChange={vi.fn()}
      />
    );
    expect(screen.getByRole("alert")).toHaveTextContent("原文编辑");
    expect(screen.queryByText("添加模型")).not.toBeInTheDocument();
  });
});
