import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ProviderTestDialog } from "../ProviderTestDialog";
import type { ProviderSummary } from "../../../services/providers/providers";

function makeProvider(partial: Partial<ProviderSummary> = {}): ProviderSummary {
  return {
    id: 1,
    cli_key: "codex",
    name: "Zen",
    base_urls: ["https://api.example.com/v1"],
    base_url_mode: "order",
    claude_models: {},
    enabled: true,
    priority: 0,
    cost_multiplier: 1.0,
    limit_5h_usd: null,
    limit_daily_usd: null,
    daily_reset_mode: "fixed",
    daily_reset_time: "00:00:00",
    limit_weekly_usd: null,
    limit_monthly_usd: null,
    limit_total_usd: null,
    tags: [],
    note: "",
    created_at: 0,
    updated_at: 0,
    auth_mode: "api_key",
    oauth_provider_type: null,
    oauth_email: null,
    oauth_expires_at: null,
    oauth_last_error: null,
    source_provider_id: null,
    bridge_type: null,
    gateway_protocol: null,
    provider_uuid: "33333333-3333-4333-8333-333333333333",
    availability_test_model: "deepseek-v4-flash",
    upstream_retry_policy_override: null,
    model_routing_policy_override: null,
    newapi_account_user_id: null,
    newapi_account_access_token_configured: false,
    api_key_configured: true,
    stream_idle_timeout_seconds: null,
    extension_values: [],
    ...partial,
  };
}

function modelInput() {
  return screen.getByLabelText("模型") as HTMLInputElement;
}

function promptInput() {
  return screen.getByLabelText("提示词") as HTMLInputElement;
}

describe("ProviderTestDialog", () => {
  beforeEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it("opens on the first configured model with the default prompt and confirms them", () => {
    const onConfirm = vi.fn();
    render(
      <ProviderTestDialog
        provider={makeProvider()}
        testing={false}
        onClose={vi.fn()}
        onConfirm={onConfirm}
      />
    );

    expect(modelInput().value).toBe("deepseek-v4-flash");
    expect(promptInput().value).toBe("hi");

    fireEvent.change(modelInput(), { target: { value: "grok-4.6" } });
    fireEvent.change(promptInput(), { target: { value: "你好" } });
    fireEvent.click(screen.getByRole("button", { name: "开始测试" }));

    expect(onConfirm).toHaveBeenCalledWith({ model: "grok-4.6", prompt: "你好" });
  });

  it("stays usable when the provider has no concrete model configured", () => {
    const onConfirm = vi.fn();
    render(
      <ProviderTestDialog
        provider={makeProvider({
          availability_test_model: null,
        })}
        testing={false}
        onClose={vi.fn()}
        onConfirm={onConfirm}
      />
    );

    expect(modelInput().value).toBe("");

    fireEvent.change(promptInput(), { target: { value: "  " } });
    fireEvent.click(screen.getByRole("button", { name: "开始测试" }));

    // Blank values travel as-is; the service layer nulls them so the backend picks defaults.
    expect(onConfirm).toHaveBeenCalledWith({ model: "", prompt: "  " });
  });

  it("cancels without confirming and locks input while testing", () => {
    const onClose = vi.fn();
    const onConfirm = vi.fn();
    const { rerender } = render(
      <ProviderTestDialog
        provider={makeProvider()}
        testing={false}
        onClose={onClose}
        onConfirm={onConfirm}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: "取消" }));
    expect(onClose).toHaveBeenCalledTimes(1);
    expect(onConfirm).not.toHaveBeenCalled();

    rerender(
      <ProviderTestDialog
        provider={makeProvider()}
        testing={true}
        onClose={onClose}
        onConfirm={onConfirm}
      />
    );

    expect(modelInput()).toBeDisabled();
    expect(screen.getByRole("button", { name: "测试中…" })).toBeDisabled();
  });
});

describe("read-only model discovery", () => {
  beforeEach(() => {
    cleanup();
    vi.clearAllMocks();
  });
  it("merges concrete candidates without changing the selected model", async () => {
    const onDiscover = vi
      .fn()
      .mockResolvedValue(["remote-model", "remote-model", "aio/managed", "prefix-*"]);
    render(
      <ProviderTestDialog
        provider={makeProvider()}
        testing={false}
        catalogModels={["saved-model"]}
        onDiscover={onDiscover}
        onClose={vi.fn()}
        onConfirm={vi.fn()}
      />
    );
    fireEvent.click(screen.getByRole("button", { name: "获取模型候选" }));
    await screen.findByText("模型候选已更新，可在模型输入框中选择。");
    expect(modelInput().value).toBe("deepseek-v4-flash");
    const list = document.getElementById(modelInput().getAttribute("list") ?? "");
    expect([...list!.querySelectorAll("option")].map((option) => option.value)).toEqual([
      "deepseek-v4-flash",
      "saved-model",
      "remote-model",
    ]);
    expect(onDiscover).toHaveBeenCalledTimes(1);
  });

  it("ignores stale discovery results after switching providers", async () => {
    let resolve!: (models: string[]) => void;
    const pending = new Promise<string[]>((done) => {
      resolve = done;
    });
    const props = {
      testing: false,
      onClose: vi.fn(),
      onConfirm: vi.fn(),
      onDiscover: vi.fn().mockReturnValue(pending),
    };
    const { rerender } = render(<ProviderTestDialog {...props} provider={makeProvider()} />);
    fireEvent.click(screen.getByRole("button", { name: "获取模型候选" }));
    rerender(
      <ProviderTestDialog
        {...props}
        provider={makeProvider({ id: 2, availability_test_model: "new-provider-model" })}
      />
    );
    await act(async () => {
      resolve(["stale-model"]);
      await pending;
    });
    expect(modelInput().value).toBe("new-provider-model");
    expect(screen.queryByText("模型候选已更新，可在模型输入框中选择。")).not.toBeInTheDocument();
    expect(document.querySelector('option[value="stale-model"]')).toBeNull();
  });

  it("keeps configured candidates when discovery fails", async () => {
    render(
      <ProviderTestDialog
        provider={makeProvider()}
        testing={false}
        onClose={vi.fn()}
        onConfirm={vi.fn()}
        onDiscover={vi.fn().mockRejectedValue(new Error("discovery failed"))}
      />
    );
    fireEvent.click(screen.getByRole("button", { name: "获取模型候选" }));
    await waitFor(() => expect(screen.getByRole("status")).toHaveTextContent("discovery failed"));
    expect(modelInput().value).toBe("deepseek-v4-flash");
  });
});
