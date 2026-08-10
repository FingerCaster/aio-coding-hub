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

function LegacyKeywordHarness() {
  const [policy, setPolicy] = useState<UpstreamRetryPolicy>(() => {
    const initial = cloneUpstreamRetryPolicy(DEFAULT_UPSTREAM_RETRY_POLICY);
    initial.stream_internal_errors.legacy_retry_keywords = ["legacy one", "legacy two"];
    return initial;
  });
  return <RetryPolicyFields policy={policy} disabled={false} onChange={setPolicy} />;
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

  it("edits and disables the stream terminal firewall passthrough policy", () => {
    render(<Harness />);

    expect(screen.getByText("网络与传输失败")).toBeInTheDocument();
    expect(screen.getByText("无法与供应商建立连接。")).toBeInTheDocument();
    expect(screen.getByText("每个供应商最多重试次数")).toBeInTheDocument();
    expect(screen.queryByLabelText("终态帧透传例外（每行一项）")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "查看 Codex 流终态防火墙设置" }));
    expect(screen.getByText(/502 \/ GW_FAKE_200/)).toBeInTheDocument();
    const passthroughKeywords = screen.getByLabelText("终态帧透传例外（每行一项）");
    expect(passthroughKeywords).toHaveValue("high-risk cyber");
    fireEvent.change(passthroughKeywords, { target: { value: "ticket-123\nvendor oddity" } });

    let edited = JSON.parse(screen.getByTestId("policy-state").textContent ?? "{}") as {
      stream_internal_errors: {
        enabled: boolean;
        passthrough_keywords: string[];
        legacy_retry_keywords: string[];
      };
    };
    expect(edited.stream_internal_errors).toEqual({
      enabled: true,
      passthrough_keywords: ["ticket-123", "vendor oddity"],
      legacy_retry_keywords: [],
    });

    fireEvent.click(screen.getByRole("switch", { name: "启用 Codex 流终态防火墙" }));
    edited = JSON.parse(screen.getByTestId("policy-state").textContent ?? "{}") as typeof edited;
    expect(edited.stream_internal_errors.enabled).toBe(false);
    expect(passthroughKeywords).toBeDisabled();
  });

  it("shows migrated retry keywords as a read-only compatibility notice", () => {
    render(<LegacyKeywordHarness />);

    fireEvent.click(screen.getByRole("button", { name: "查看 Codex 流终态防火墙设置" }));
    expect(screen.getByText(/已迁移 2 条旧版重试词/)).toBeInTheDocument();
    expect(screen.queryByLabelText(/旧版兼容重试词/)).not.toBeInTheDocument();
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
