import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { ProviderChainView } from "../ProviderChainView";

describe("components/ProviderChainView", () => {
  it("renders loading + empty + merged views", () => {
    const { rerender } = render(
      <ProviderChainView attemptLogs={[]} attemptLogsLoading attemptsJson={null} />
    );
    expect(screen.getByText("加载中…")).toBeInTheDocument();

    rerender(<ProviderChainView attemptLogs={[]} attemptLogsLoading={false} attemptsJson={null} />);
    expect(screen.getByText("无故障切换尝试。")).toBeInTheDocument();

    rerender(
      <ProviderChainView
        attemptLogs={[
          {
            attempt_index: 1,
            provider_id: 1,
            provider_name: "P1",
            base_url: "https://p1",
            outcome: "failed",
            status: 500,
          },
          {
            attempt_index: 2,
            provider_id: 2,
            provider_name: "P2",
            base_url: "https://p2",
            outcome: "success",
            status: 200,
          },
        ]}
        attemptLogsLoading={false}
        attemptsJson={"not-json"}
      />
    );
    expect(screen.getByText("尝试 JSON 解析失败")).toBeInTheDocument();
    expect(screen.getByText("起始供应商：")).toBeInTheDocument();
    expect(screen.getByText("最终供应商：")).toBeInTheDocument();
    expect(screen.getAllByText("P1 (#1)")).toHaveLength(3);
    expect(screen.getAllByText("P2 (#2)")).toHaveLength(3);

    rerender(
      <ProviderChainView
        attemptLogs={[]}
        attemptLogsLoading={false}
        attemptsJson={JSON.stringify([
          {
            provider_id: 1,
            provider_name: "P1",
            base_url: "https://p1",
            outcome: "success",
            status: 200,
            provider_index: 0,
            retry_index: 0,
          },
        ])}
      />
    );
    expect(screen.getByText("数据源：request_logs.attempts_json")).toBeInTheDocument();
    expect(screen.getByText("请求成功")).toBeInTheDocument();

    rerender(
      <ProviderChainView
        attemptLogs={[
          {
            attempt_index: 1,
            provider_id: 99,
            provider_name: "未知",
            base_url: "",
            outcome: "failed",
            status: null,
            attempt_started_ms: 10,
            attempt_duration_ms: 50,
          },
        ]}
        attemptLogsLoading={false}
        attemptsJson={JSON.stringify([
          {
            provider_id: 99,
            provider_name: "未知",
            base_url: "https://p99",
            outcome: "failed",
            status: 400,
            provider_index: 1,
            retry_index: 2,
            error_code: "E",
            decision: "skip",
            reason: "because",
          },
        ])}
      />
    );
    expect(screen.getByText("数据源：request_logs.attempts_json（结构化）")).toBeInTheDocument();
    expect(screen.getAllByText("未知供应商 (#99)")).toHaveLength(4);
    expect(screen.getByText("请求失败")).toBeInTheDocument();
    // Error reason is displayed in the error block
    expect(screen.getByText("because")).toBeInTheDocument();
    // Unified provider identity is shown in the detail body
    expect(screen.getByText("供应商：")).toBeInTheDocument();
    // Endpoint shown in detail body
    expect(screen.getByText("https://p99")).toBeInTheDocument();
  });

  it("trims the snapshot name and keeps identity visible when an empty-url attempt is collapsed", () => {
    render(
      <ProviderChainView
        attemptLogs={[
          {
            attempt_index: 1,
            provider_id: 7,
            provider_name: "  Provider A  ",
            base_url: "",
            outcome: "success",
            status: 200,
          },
        ]}
        attemptLogsLoading={false}
        attemptsJson={null}
      />
    );

    expect(screen.getAllByText("Provider A (#7)")).toHaveLength(4);
    expect(screen.queryByText("端点")).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: /请求成功 Provider A \(#7\)/ }));

    expect(screen.getAllByText("Provider A (#7)")).toHaveLength(3);
    expect(screen.queryByText("供应商：")).not.toBeInTheDocument();
  });

  it("distinguishes providers with the same name and URL by stable ID", () => {
    render(
      <ProviderChainView
        attemptLogs={[
          {
            attempt_index: 1,
            provider_id: 7,
            provider_name: "Provider A",
            base_url: "https://shared.example",
            outcome: "failed",
            status: 500,
          },
          {
            attempt_index: 2,
            provider_id: 8,
            provider_name: "Provider A",
            base_url: "https://shared.example",
            outcome: "success",
            status: 200,
          },
        ]}
        attemptLogsLoading={false}
        attemptsJson={null}
      />
    );

    expect(screen.getAllByText("Provider A (#7)")).toHaveLength(3);
    expect(screen.getAllByText("Provider A (#8)")).toHaveLength(3);

    fireEvent.click(screen.getByRole("button", { name: /Provider A \(#7\)/ }));
    fireEvent.click(screen.getByRole("button", { name: /Provider A \(#8\)/ }));

    expect(screen.getByRole("button", { name: /Provider A \(#7\)/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Provider A \(#8\)/ })).toBeInTheDocument();
  });

  it.each([
    ["empty", "", 11],
    ["whitespace", "   ", 12],
    ["Unknown", "Unknown", 13],
    ["unknown", "unknown", 14],
    ["mixed-case unknown", "uNkNoWn", 15],
    ["Chinese unknown", "未知", 16],
  ])("normalizes a %s provider name", (_caseName, providerName, providerId) => {
    render(
      <ProviderChainView
        attemptLogs={[
          {
            attempt_index: 1,
            provider_id: providerId,
            provider_name: providerName,
            base_url: "",
            outcome: "failed",
            status: null,
          },
        ]}
        attemptLogsLoading={false}
        attemptsJson={null}
      />
    );

    expect(screen.getAllByText(`未知供应商 (#${providerId})`)).toHaveLength(4);
  });

  it.each([
    ["zero", 0],
    ["negative", -1],
    ["fractional", 1.5],
    ["unsafe", Number.MAX_SAFE_INTEGER + 1],
    ["missing", null],
  ])("does not render an invalid %s provider ID", (_caseName, providerId) => {
    render(
      <ProviderChainView
        attemptLogs={[
          {
            attempt_index: 1,
            provider_id: providerId as number,
            provider_name: "Provider A",
            base_url: "",
            outcome: "failed",
            status: null,
          },
        ]}
        attemptLogsLoading={false}
        attemptsJson={null}
      />
    );

    expect(screen.getAllByText("Provider A (ID 不可用)")).toHaveLength(4);
    expect(screen.queryByText(`#${String(providerId)}`)).not.toBeInTheDocument();
  });

  it("renders skipped summary attempts with fallback values and collapsed details", () => {
    render(
      <ProviderChainView
        attemptLogs={[]}
        attemptLogsLoading={false}
        attemptsJson={JSON.stringify([
          {
            provider_id: 0,
            provider_name: "",
            base_url: "",
            outcome: "skipped",
            status: null,
            session_reuse: false,
            decision: "retry",
            selection_method: "priority",
            circuit_state_before: "closed",
          },
        ])}
      />
    );

    expect(screen.getByText("当前显示的是摘要链路，未拿到逐次尝试日志")).toBeInTheDocument();
    expect(screen.getByText("最终失败")).toBeInTheDocument();
    expect(screen.getAllByText("未知供应商 (ID 不可用)")).toHaveLength(4);
    expect(screen.getByText("跳过")).toBeInTheDocument();
    expect(screen.getByText("priority")).toBeInTheDocument();
    expect(screen.getByText("retry")).toBeInTheDocument();
    expect(screen.getByText("closed")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: /跳过/ }));
    expect(screen.queryByText("供应商：")).not.toBeInTheDocument();
  });

  it("renders reason and circuit attribution for circuit-gate skipped attempts", () => {
    const farFutureUnix = 4_102_444_800; // 2100-01-01, keeps "约 N 分钟后" stable
    render(
      <ProviderChainView
        attemptLogs={[]}
        attemptLogsLoading={false}
        attemptsJson={JSON.stringify([
          {
            provider_id: 7,
            provider_name: "Provider A",
            base_url: "https://provider-a.example",
            outcome: "skipped",
            status: null,
            error_category: "circuit_breaker",
            error_code: "GW_PROVIDER_CIRCUIT_OPEN",
            decision: "skip",
            reason: "provider skipped by circuit breaker (open)",
            reason_code: "circuit_open",
            circuit_state_before: "OPEN",
            circuit_state_after: "OPEN",
            circuit_failure_count: 5,
            circuit_failure_threshold: 5,
            circuit_recover_at_unix: farFutureUnix,
            circuit_trigger_error_code: "GW_UPSTREAM_TIMEOUT",
          },
        ])}
      />
    );

    expect(screen.getByText("跳过")).toBeInTheDocument();
    expect(screen.getByText("跳过原因")).toBeInTheDocument();
    expect(screen.getByText("provider skipped by circuit breaker (open)")).toBeInTheDocument();
    expect(screen.getByText("5/5 次失败")).toBeInTheDocument();
    expect(screen.getByText("触发：上游超时")).toBeInTheDocument();
    expect(screen.getByText(/约 \d+ 分钟后/)).toBeInTheDocument();
  });

  it("degrades gracefully for skipped attempts without circuit attribution", () => {
    render(
      <ProviderChainView
        attemptLogs={[]}
        attemptLogsLoading={false}
        attemptsJson={JSON.stringify([
          {
            provider_id: 7,
            provider_name: "Provider A",
            base_url: "https://provider-a.example",
            outcome: "skipped",
            status: null,
            decision: "skip",
            reason: "provider skipped by rate limit",
          },
        ])}
      />
    );

    expect(screen.getByText("跳过原因")).toBeInTheDocument();
    expect(screen.getByText("provider skipped by rate limit")).toBeInTheDocument();
    expect(screen.queryByText(/触发：/)).not.toBeInTheDocument();
    expect(screen.queryByText("熔断器:")).not.toBeInTheDocument();
  });

  it.each([
    ["started", "started", "试探中"],
    ["success", "success", "试探成功"],
    ["failed", "failed", "试探失败"],
    ["cooldown", "skipped", "试探冷却中"],
    ["in_flight", "skipped", "已有试探进行中"],
  ])("renders the %s probe result with trigger and generation", (probeResult, outcome, label) => {
    render(
      <ProviderChainView
        attemptLogs={[]}
        attemptLogsLoading={false}
        attemptsJson={JSON.stringify([
          {
            provider_id: 7,
            provider_name: "Provider A",
            base_url: "https://provider-a.example",
            outcome,
            status: outcome === "success" ? 200 : null,
            selection_method: "circuit_probe",
            probe: true,
            probe_trigger: "natural_compaction",
            probe_result: probeResult,
            probe_generation: 4,
          },
        ])}
      />
    );

    expect(screen.getByText(label)).toBeInTheDocument();
    expect(screen.getByText("触发：上下文压缩")).toBeInTheDocument();
    expect(screen.getByText("代次 4")).toBeInTheDocument();
    expect(screen.getByText("熔断试探")).toBeInTheDocument();
  });

  it("keeps probe retries as attempts while preserving provider identity across fallback", () => {
    render(
      <ProviderChainView
        attemptLogs={[]}
        attemptLogsLoading={false}
        attemptsJson={JSON.stringify([
          {
            provider_id: 7,
            provider_name: "Provider A",
            base_url: "https://provider-a.example",
            outcome: "failed",
            status: 502,
            selection_method: "circuit_probe",
            probe: true,
            probe_trigger: "aggressive_turn",
            probe_result: "failed",
            probe_generation: 12,
          },
          {
            provider_id: 7,
            provider_name: "Provider A",
            base_url: "https://provider-a.example",
            outcome: "failed",
            status: 502,
            selection_method: "circuit_probe",
            probe: true,
            probe_trigger: "aggressive_turn",
            probe_result: "failed",
            probe_generation: 12,
          },
          {
            provider_id: 8,
            provider_name: "Provider B",
            base_url: "https://provider-b.example",
            outcome: "success",
            status: 200,
            selection_method: "ordered",
          },
        ])}
      />
    );

    expect(screen.getByText("共尝试 3 次")).toBeInTheDocument();
    expect(screen.getAllByText("Provider A (#7)").length).toBeGreaterThan(1);
    expect(screen.getAllByText("Provider B (#8)").length).toBeGreaterThan(1);
    expect(screen.getByText("最终成功")).toBeInTheDocument();
  });

  it("colors circuit badges by normalized uppercase backend states", () => {
    render(
      <ProviderChainView
        attemptLogs={[]}
        attemptLogsLoading={false}
        attemptsJson={JSON.stringify([
          {
            provider_id: 1,
            provider_name: "A",
            base_url: "https://a",
            outcome: "failed",
            status: 500,
            circuit_state_after: "OPEN",
          },
          {
            provider_id: 2,
            provider_name: "B",
            base_url: "https://b",
            outcome: "failed",
            status: 500,
            circuit_state_after: "HALF_OPEN",
          },
          {
            provider_id: 3,
            provider_name: "C",
            base_url: "https://c",
            outcome: "success",
            status: 200,
            circuit_state_after: "CLOSED",
          },
        ])}
      />
    );

    expect(screen.getByText("OPEN").className).toContain("bg-rose-500");
    expect(screen.getByText("HALF_OPEN").className).toContain("bg-amber-500");
    expect(screen.getByText("CLOSED").className).toContain("bg-emerald-500");
  });

  it("renders structured failures with circuit transitions and gateway labels", () => {
    render(
      <ProviderChainView
        attemptLogs={[
          {
            attempt_index: 2,
            provider_id: null as any,
            provider_name: "",
            base_url: "",
            outcome: "",
            status: null,
          },
          {
            attempt_index: 1,
            provider_id: 7,
            provider_name: "First",
            base_url: "https://first",
            outcome: "failed",
            status: 502,
            attempt_duration_ms: 31,
          },
        ]}
        attemptLogsLoading={false}
        attemptsJson={JSON.stringify([
          {
            provider_id: 7,
            provider_name: "First",
            base_url: "https://first-json",
            outcome: "failed",
            status: 500,
            error_category: "upstream",
            error_code: "upstream_5xx",
            decision: "switch",
            reason: "first failed",
            reason_code: "HTTP_502",
            selection_method: "weighted",
            circuit_state_before: "closed",
            circuit_state_after: "open",
            circuit_failure_count: 3,
            circuit_failure_threshold: 3,
          },
          {
            provider_id: 8,
            provider_name: "Fallback",
            base_url: "https://fallback",
            outcome: "success",
            status: 200,
            decision: "other",
            selection_method: "fallback",
            circuit_state_after: "half_open",
          },
        ])}
      />
    );

    expect(screen.getByText("起始供应商：")).toBeInTheDocument();
    expect(screen.getByText("最终供应商：")).toBeInTheDocument();
    expect(screen.getAllByText("Fallback (#8)").length).toBeGreaterThan(0);
    expect(screen.getByText("最终成功")).toBeInTheDocument();
    expect(screen.getByText("重试 #1")).toBeInTheDocument();
    expect(screen.getByText("+31ms")).toBeInTheDocument();
    expect(screen.getByText("HTTP 502")).toBeInTheDocument();
    expect(screen.getByText("first failed")).toBeInTheDocument();
    expect(screen.getByText("HTTP_502")).toBeInTheDocument();
    expect(screen.getByText("upstream")).toBeInTheDocument();
    expect(screen.getByText("switch")).toBeInTheDocument();
    expect(screen.getByText("weighted")).toBeInTheDocument();
    expect(screen.getByText("3/3 次失败")).toBeInTheDocument();
    expect(screen.getByText("open")).toBeInTheDocument();
    expect(screen.getByText("half_open")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "结构化错误详情" }));
    const structuredDetails = screen.getByText("熔断器变化:").closest("div")!;
    expect(structuredDetails).toHaveTextContent("closed");
    expect(structuredDetails).toHaveTextContent("open");
  });
});
