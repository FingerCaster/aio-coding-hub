import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { providerShareImportConfirm } from "../../services/providers/providerShare";
import type { ProviderSummary } from "../../services/providers/providers";
import { createTestQueryClient, createQueryWrapper } from "../../test/utils/reactQuery";
import { providersKeys } from "../keys";
import { useProviderShareImportMutation } from "../providerShare";

vi.mock("../../services/providers/providerShare", () => ({
  providerShareImportConfirm: vi.fn(),
}));

function provider(id: number, cliKey: ProviderSummary["cli_key"], name: string): ProviderSummary {
  return {
    id,
    provider_uuid: `00000000-0000-4000-8000-${String(id).padStart(12, "0")}`,
    cli_key: cliKey,
    name,
    base_urls: [],
    base_url_mode: "order",
    claude_models: {},
    model_mapping: { default_model: null, exact: {} },
    availability_test_model: null,
    enabled: false,
    priority: 0,
    cost_multiplier: 1,
    limit_5h_usd: null,
    limit_daily_usd: null,
    daily_reset_mode: "fixed",
    daily_reset_time: "00:00:00",
    limit_weekly_usd: null,
    limit_monthly_usd: null,
    limit_total_usd: null,
    tags: [],
    note: "",
    created_at: 1,
    updated_at: 1,
    auth_mode: "api_key",
    oauth_provider_type: null,
    oauth_email: null,
    oauth_expires_at: null,
    oauth_last_error: null,
    source_provider_id: null,
    bridge_type: null,
    stream_idle_timeout_seconds: null,
    extension_values: [],
    upstream_retry_policy_override: null,
    api_key_configured: true,
    newapi_account_user_id: null,
    newapi_account_access_token_configured: false,
  };
}

describe("query/providerShare", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("deduplicates the target CLI cache, invalidates it, and preserves other CLIs", async () => {
    const imported = provider(9, "claude", "Imported");
    vi.mocked(providerShareImportConfirm).mockResolvedValueOnce(imported);
    const client = createTestQueryClient();
    const existingClaude = [provider(1, "claude", "Existing"), provider(9, "claude", "Stale")];
    const existingCodex = [provider(2, "codex", "Codex")];
    client.setQueryData(providersKeys.list("claude"), existingClaude);
    client.setQueryData(providersKeys.list("codex"), existingCodex);
    const invalidate = vi.spyOn(client, "invalidateQueries");
    const wrapper = createQueryWrapper(client);
    const { result } = renderHook(() => useProviderShareImportMutation(), { wrapper });

    await act(async () => {
      await result.current.mutateAsync({ previewToken: "a".repeat(64) });
    });

    expect(providerShareImportConfirm).toHaveBeenCalledWith("a".repeat(64));
    expect(client.getQueryData(providersKeys.list("claude"))).toEqual([
      existingClaude[0],
      imported,
    ]);
    expect(client.getQueryData(providersKeys.list("codex"))).toEqual(existingCodex);
    expect(invalidate).toHaveBeenCalledWith({ queryKey: providersKeys.list("claude") });
    expect(invalidate).not.toHaveBeenCalledWith({ queryKey: providersKeys.list("codex") });
  });
});
