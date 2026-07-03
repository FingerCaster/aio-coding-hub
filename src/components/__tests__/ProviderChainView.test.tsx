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
    expect(screen.getAllByText("未知（id=99）").length).toBeGreaterThan(0);
    expect(screen.getByText("请求失败")).toBeInTheDocument();
    // Error reason is displayed in the error block
    expect(screen.getByText("because")).toBeInTheDocument();
    // Provider ID shown in detail body
    expect(screen.getByText("99")).toBeInTheDocument();
    // Endpoint shown in detail body
    expect(screen.getByText("https://p99")).toBeInTheDocument();
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
    expect(screen.getAllByText("未知（id=0）")).toHaveLength(2);
    expect(screen.getByText("跳过")).toBeInTheDocument();
    expect(screen.getByText("priority")).toBeInTheDocument();
    expect(screen.getByText("retry")).toBeInTheDocument();
    expect(screen.getByText("closed")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: /跳过/ }));
    expect(screen.queryByText("Provider ID:")).not.toBeInTheDocument();
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
    expect(screen.getByText("Fallback")).toBeInTheDocument();
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
