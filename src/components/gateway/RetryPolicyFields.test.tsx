import { fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { describe, expect, it } from "vitest";
import {
  cloneUpstreamRetryPolicy,
  DEFAULT_UPSTREAM_RETRY_POLICY,
} from "../../services/gateway/upstreamRetryPolicy";
import type { UpstreamRetryPolicy } from "../../services/settings/settings";
import { RetryPolicyFields } from "./RetryPolicyFields";

function Harness() {
  const [policy, setPolicy] = useState<UpstreamRetryPolicy>(() =>
    cloneUpstreamRetryPolicy(DEFAULT_UPSTREAM_RETRY_POLICY)
  );
  return (
    <>
      <RetryPolicyFields policy={policy} disabled={false} onChange={setPolicy} />
      <output data-testid="policy-state">{JSON.stringify(policy)}</output>
    </>
  );
}

describe("RetryPolicyFields", () => {
  it("adds, edits, disables, and deletes HTTP rules", () => {
    render(<Harness />);

    expect(screen.getByRole("switch", { name: "启用瞬时错误重试" })).toBeInTheDocument();
    expect(screen.getByRole("switch", { name: "配置型重试计入熔断" })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "新增规则" }));
    expect(screen.getByRole("group", { name: "HTTP 规则 4" })).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("规则 4 · 错误码"), {
      target: { value: "429" },
    });
    fireEvent.change(screen.getAllByLabelText("描述")[3], {
      target: { value: "Quota retry" },
    });
    const bodyInput = screen.getAllByLabelText("匹配内容（每行一项）")[3];
    fireEvent.change(bodyInput, {
      target: { value: "quota exhausted" },
    });
    fireEvent.change(bodyInput, {
      target: { value: "quota exhausted\n" },
    });
    expect(bodyInput).toHaveValue("quota exhausted\n");
    fireEvent.change(bodyInput, {
      target: { value: "quota exhausted\nrate,limit\n*.json" },
    });
    fireEvent.click(screen.getByRole("switch", { name: "启用 HTTP 规则 4" }));

    const edited = JSON.parse(screen.getByTestId("policy-state").textContent ?? "{}") as {
      http_rules: Array<Record<string, unknown>>;
    };
    expect(edited.http_rules[3]).toEqual({
      enabled: false,
      status_code: 429,
      body_contains: ["quota exhausted", "rate,limit", "*.json"],
      description: "Quota retry",
    });

    fireEvent.click(screen.getByRole("button", { name: "删除 HTTP 规则 4" }));
    const deleted = JSON.parse(screen.getByTestId("policy-state").textContent ?? "{}") as {
      http_rules: unknown[];
    };
    expect(deleted.http_rules).toHaveLength(3);
  });

  it("allows the backend character limit for astral descriptions", () => {
    render(<Harness />);
    const description = screen.getAllByLabelText("描述")[0];
    const value = "😀".repeat(256);

    expect(description).not.toHaveAttribute("maxlength");
    fireEvent.change(description, { target: { value } });

    const edited = JSON.parse(screen.getByTestId("policy-state").textContent ?? "{}") as {
      http_rules: Array<{ description: string }>;
    };
    expect(edited.http_rules[0].description).toBe(value);
  });
});
