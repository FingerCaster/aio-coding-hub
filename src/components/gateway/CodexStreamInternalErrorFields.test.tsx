import { fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { describe, expect, it } from "vitest";
import type { UpstreamStreamInternalErrorPolicy } from "../../services/settings/settings";
import { DEFAULT_UPSTREAM_RETRY_POLICY } from "../../services/gateway/upstreamRetryPolicy";
import { CodexStreamInternalErrorFields } from "./CodexStreamInternalErrorFields";

function Harness({ showGuard = true }: { showGuard?: boolean }) {
  const [policy, setPolicy] = useState<UpstreamStreamInternalErrorPolicy>(() => ({
    ...DEFAULT_UPSTREAM_RETRY_POLICY.stream_internal_errors,
    passthrough_keywords: ["initial"],
    legacy_retry_keywords: [],
  }));
  const [guardMs, setGuardMs] = useState(500);

  return (
    <>
      <CodexStreamInternalErrorFields
        policy={policy}
        disabled={false}
        onChange={setPolicy}
        {...(showGuard
          ? {
              streamInternalErrorGuardMs: guardMs,
              maxStreamInternalErrorGuardMs: 5000,
              onStreamInternalErrorGuardMsChange: setGuardMs,
            }
          : {})}
      />
      <output data-testid="stream-policy-state">{JSON.stringify(policy)}</output>
      <output data-testid="guard-state">{guardMs}</output>
    </>
  );
}

describe("CodexStreamInternalErrorFields", () => {
  it("edits passthrough keywords and disables the firewall", () => {
    render(<Harness />);

    expect(screen.queryByLabelText("终态帧透传例外（每行一项）")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "查看 Codex 流终态防火墙设置" }));

    const passthrough = screen.getByLabelText("终态帧透传例外（每行一项）");
    fireEvent.change(passthrough, { target: { value: "ticket-123\nvendor oddity" } });
    expect(JSON.parse(screen.getByTestId("stream-policy-state").textContent ?? "{}")).toMatchObject(
      {
        enabled: true,
        passthrough_keywords: ["ticket-123", "vendor oddity"],
        legacy_retry_keywords: [],
      }
    );

    fireEvent.click(screen.getByRole("switch", { name: "启用 Codex 流终态防火墙" }));
    expect(JSON.parse(screen.getByTestId("stream-policy-state").textContent ?? "{}")).toMatchObject(
      {
        enabled: false,
      }
    );
    expect(passthrough).toBeDisabled();
  });

  it("shows migrated legacy keywords without allowing edits", () => {
    function LegacyHarness() {
      const policy: UpstreamStreamInternalErrorPolicy = {
        ...DEFAULT_UPSTREAM_RETRY_POLICY.stream_internal_errors,
        legacy_retry_keywords: ["legacy one", "legacy two"],
      };
      return (
        <CodexStreamInternalErrorFields
          policy={policy}
          disabled={false}
          onChange={() => undefined}
        />
      );
    }

    render(<LegacyHarness />);
    fireEvent.click(screen.getByRole("button", { name: "查看 Codex 流终态防火墙设置" }));
    expect(screen.getByText(/已迁移 2 条旧版重试词/)).toBeInTheDocument();
    expect(screen.queryByLabelText(/旧版兼容重试词/)).not.toBeInTheDocument();
  });

  it("supports both observation-window boundaries and omits provider overrides", () => {
    render(<Harness />);
    fireEvent.click(screen.getByRole("button", { name: "查看 Codex 流终态防火墙设置" }));

    const guard = screen.getByRole("spinbutton", { name: "流内部错误观察窗口" });
    expect(guard).toHaveAttribute("min", "0");
    expect(guard).toHaveAttribute("max", "5000");
    fireEvent.change(guard, { target: { value: "0" } });
    expect(screen.getByTestId("guard-state")).toHaveTextContent("0");
    fireEvent.change(guard, { target: { value: "5000" } });
    expect(screen.getByTestId("guard-state")).toHaveTextContent("5000");

    render(<Harness showGuard={false} />);
    const buttons = screen.getAllByRole("button", { name: "查看 Codex 流终态防火墙设置" });
    fireEvent.click(buttons[buttons.length - 1] as HTMLElement);
    expect(screen.queryAllByRole("spinbutton", { name: "流内部错误观察窗口" })).toHaveLength(1);
  });
});
