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
    expect(screen.getByRole("group", { name: "HTTP 规则 5" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "收起 HTTP 规则 5" })).toHaveAttribute(
      "aria-expanded",
      "true"
    );
    fireEvent.change(screen.getByLabelText("规则 5 · 错误码"), {
      target: { value: "429" },
    });
    fireEvent.change(screen.getByLabelText("HTTP 规则 5 描述"), {
      target: { value: "Quota retry" },
    });
    const bodyInput = screen.getByLabelText("HTTP 规则 5 匹配内容");
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
    fireEvent.click(screen.getByRole("switch", { name: "启用 HTTP 规则 5" }));

    const edited = JSON.parse(screen.getByTestId("policy-state").textContent ?? "{}") as {
      http_rules: Array<Record<string, unknown>>;
    };
    expect(edited.http_rules[4]).toEqual({
      enabled: false,
      status_code: 429,
      body_contains: ["quota exhausted", "rate,limit", "*.json"],
      description: "Quota retry",
    });

    fireEvent.click(screen.getByRole("button", { name: "删除 HTTP 规则 5" }));
    const deleted = JSON.parse(screen.getByTestId("policy-state").textContent ?? "{}") as {
      http_rules: unknown[];
    };
    expect(deleted.http_rules).toHaveLength(4);
  });

  it("edits and disables the stream-internal-error keyword policy", () => {
    render(<Harness />);

    expect(screen.getByText("网络与传输失败")).toBeInTheDocument();
    expect(screen.getByText("无法与供应商建立连接。")).toBeInTheDocument();
    expect(screen.getByText("每个供应商最多重试次数")).toBeInTheDocument();
    expect(screen.queryByLabelText("重试关键词（每行一项）")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "查看 Codex 200 流内部错误规则" }));
    expect(screen.getByText(/网关返回 502 \/ GW_FAKE_200/)).toBeInTheDocument();
    const retryKeywords = screen.getByLabelText("重试关键词（每行一项）");
    const nonRetryKeywords = screen.getByLabelText("不重试关键词（每行一项）");
    fireEvent.change(retryKeywords, { target: { value: "capacity\noverloaded" } });
    fireEvent.change(nonRetryKeywords, { target: { value: "policy\nsafety" } });

    let edited = JSON.parse(screen.getByTestId("policy-state").textContent ?? "{}") as {
      stream_internal_errors: {
        enabled: boolean;
        retry_keywords: string[];
        non_retry_keywords: string[];
      };
    };
    expect(edited.stream_internal_errors).toEqual({
      enabled: true,
      retry_keywords: ["capacity", "overloaded"],
      non_retry_keywords: ["policy", "safety"],
    });

    fireEvent.click(screen.getByRole("switch", { name: "启用流内部错误重试" }));
    edited = JSON.parse(screen.getByTestId("policy-state").textContent ?? "{}") as typeof edited;
    expect(edited.stream_internal_errors.enabled).toBe(false);
    expect(retryKeywords).toBeDisabled();
    expect(nonRetryKeywords).toBeDisabled();
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
    expect(firstRule).toHaveTextContent("HTTP 502");

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
