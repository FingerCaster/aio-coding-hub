import { fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { describe, expect, it } from "vitest";
import {
  cloneUpstreamRetryPolicy,
  DEFAULT_UPSTREAM_RETRY_POLICY,
} from "../../services/gateway/upstreamRetryPolicy";
import type { UpstreamRetryPolicy } from "../../services/settings/settings";
import { RetryPolicyFields } from "./RetryPolicyFields";

function Harness({
  initialPolicy = DEFAULT_UPSTREAM_RETRY_POLICY,
  sharesBudgetWithCodexStreamErrors = false,
}: {
  initialPolicy?: UpstreamRetryPolicy;
  sharesBudgetWithCodexStreamErrors?: boolean;
}) {
  const [policy, setPolicy] = useState<UpstreamRetryPolicy>(() =>
    cloneUpstreamRetryPolicy(initialPolicy)
  );
  return (
    <>
      <RetryPolicyFields
        policy={policy}
        disabled={false}
        onChange={setPolicy}
        sharesBudgetWithCodexStreamErrors={sharesBudgetWithCodexStreamErrors}
      />
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
    expect(screen.getByRole("group", { name: "HTTP 规则 2" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "收起 HTTP 规则 2" })).toHaveAttribute(
      "aria-expanded",
      "true"
    );
    fireEvent.change(screen.getByLabelText("规则 2 · 错误码"), {
      target: { value: "429" },
    });
    fireEvent.change(screen.getByLabelText("HTTP 规则 2 描述"), {
      target: { value: "Quota retry" },
    });
    const bodyInput = screen.getByLabelText("HTTP 规则 2 匹配内容");
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
    fireEvent.click(screen.getByRole("switch", { name: "启用 HTTP 规则 2" }));

    const edited = JSON.parse(screen.getByTestId("policy-state").textContent ?? "{}") as {
      http_rules: Array<Record<string, unknown>>;
    };
    expect(edited.http_rules[1]).toEqual({
      enabled: false,
      status_code: 429,
      body_contains: ["quota exhausted", "rate,limit", "*.json"],
      description: "Quota retry",
    });

    fireEvent.click(screen.getByRole("button", { name: "删除 HTTP 规则 2" }));
    const deleted = JSON.parse(screen.getByTestId("policy-state").textContent ?? "{}") as {
      http_rules: unknown[];
    };
    expect(deleted.http_rules).toHaveLength(1);
  });

  it("hides Codex stream fields and preserves them while editing common fields", () => {
    const hiddenStreamPolicy = {
      enabled: false,
      passthrough_keywords: ["keep-passthrough"],
      legacy_retry_keywords: ["keep-legacy"],
    };
    render(
      <Harness
        initialPolicy={{
          ...DEFAULT_UPSTREAM_RETRY_POLICY,
          stream_internal_errors: hiddenStreamPolicy,
        }}
      />
    );

    expect(screen.getByText("网络与传输失败")).toBeInTheDocument();
    expect(screen.getByText("HTTP / 网络共享")).toBeInTheDocument();
    expect(screen.queryByText("Codex 流终态防火墙")).not.toBeInTheDocument();
    expect(
      screen.queryByRole("switch", { name: "启用 Codex 流终态防火墙" })
    ).not.toBeInTheDocument();

    const retrySwitches = screen.getAllByRole("switch", { name: "启用瞬时错误重试" });
    fireEvent.click(retrySwitches[retrySwitches.length - 1] as HTMLElement);
    const maxRetriesInputs = screen.getAllByLabelText("每个供应商最多重试次数");
    fireEvent.change(maxRetriesInputs[maxRetriesInputs.length - 1] as HTMLElement, {
      target: { value: "3" },
    });

    const policyStates = screen.getAllByTestId("policy-state");
    const edited = JSON.parse(policyStates[policyStates.length - 1]?.textContent ?? "{}") as {
      stream_internal_errors: typeof hiddenStreamPolicy;
    };
    expect(edited.stream_internal_errors).toEqual(hiddenStreamPolicy);
  });

  it("describes the Codex stream budget only when the caller opts in", () => {
    render(<Harness sharesBudgetWithCodexStreamErrors />);
    expect(screen.getByText("HTTP / 网络 / 流内部共享")).toBeInTheDocument();
  });

  it("allows the backend character limit for astral descriptions", () => {
    render(<Harness />);
    expect(screen.queryByLabelText("HTTP 规则 1 描述")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "编辑 HTTP 规则 1" }));
    const description = screen.getByLabelText("HTTP 规则 1 描述");
    const value = "😀".repeat(256);

    expect(description).not.toHaveAttribute("maxlength");
    fireEvent.change(description, { target: { value } });

    const edited = JSON.parse(screen.getByTestId("policy-state").textContent ?? "{}") as {
      http_rules: Array<{ description: string }>;
    };
    expect(edited.http_rules[0].description).toBe(value);
  });

  it("keeps HTTP rules compact until their detail editor is opened", () => {
    render(<Harness />);

    const firstRule = screen.getByRole("button", { name: "编辑 HTTP 规则 1" });
    expect(firstRule).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByLabelText("HTTP 规则 1 匹配内容")).not.toBeInTheDocument();
    expect(firstRule).toHaveTextContent("HTTP 400");

    fireEvent.click(firstRule);
    expect(screen.getByRole("button", { name: "收起 HTTP 规则 1" })).toHaveAttribute(
      "aria-expanded",
      "true"
    );
    expect(screen.getByLabelText("HTTP 规则 1 匹配内容")).toHaveAttribute("rows", "3");

    fireEvent.click(screen.getByRole("button", { name: "收起 HTTP 规则 1" }));
    expect(screen.queryByLabelText("HTTP 规则 1 匹配内容")).not.toBeInTheDocument();
  });
});
