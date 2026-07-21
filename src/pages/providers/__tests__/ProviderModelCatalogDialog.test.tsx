import { QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { toast } from "sonner";
import { cliProxyStatusAll } from "../../../services/cli/cliProxy";
import {
  codexManagedProfileCreate,
  codexManagedProfileDelete,
  codexManagedProfilesList,
  type CodexManagedProfile,
} from "../../../services/providers/codexManagedProfiles";
import {
  providerModelManualDelete,
  providerModelManualUpsert,
  providerModelsGet,
  providerModelsRefresh,
  type ProviderModelCatalog,
} from "../../../services/providers/providerModels";
import type { ProviderSummary } from "../../../services/providers/providers";
import { createTestQueryClient } from "../../../test/utils/reactQuery";
import { ProviderModelCatalogDialog } from "../ProviderModelCatalogDialog";

vi.mock("sonner", () => ({ toast: vi.fn() }));

vi.mock("../../../services/cli/cliProxy", async () => {
  const actual = await vi.importActual<typeof import("../../../services/cli/cliProxy")>(
    "../../../services/cli/cliProxy"
  );
  return { ...actual, cliProxyStatusAll: vi.fn() };
});

vi.mock("../../../services/providers/providerModels", async () => {
  const actual = await vi.importActual<typeof import("../../../services/providers/providerModels")>(
    "../../../services/providers/providerModels"
  );
  return {
    ...actual,
    providerModelsGet: vi.fn(),
    providerModelsRefresh: vi.fn(),
    providerModelManualUpsert: vi.fn(),
    providerModelManualDelete: vi.fn(),
  };
});

vi.mock("../../../services/providers/codexManagedProfiles", async () => {
  const actual = await vi.importActual<
    typeof import("../../../services/providers/codexManagedProfiles")
  >("../../../services/providers/codexManagedProfiles");
  return {
    ...actual,
    codexManagedProfilesList: vi.fn(),
    codexManagedProfileCreate: vi.fn(),
    codexManagedProfileDelete: vi.fn(),
  };
});

const PROVIDER_UUID = "11111111-1111-4111-8111-111111111111";
const MODEL_UUID = "22222222-2222-4222-8222-222222222222";
const OTHER_MODEL_UUID = "33333333-3333-4333-8333-333333333333";
const PROFILE_UUID = "44444444-4444-4444-8444-444444444444";

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

function makeProvider(partial: Partial<ProviderSummary> = {}): ProviderSummary {
  return {
    id: 7,
    provider_uuid: partial.provider_uuid ?? PROVIDER_UUID,
    cli_key: "codex",
    name: "Grok Provider",
    base_urls: ["https://example.com/v1"],
    base_url_mode: "order",
    claude_models: {},
    model_mapping: { default_model: null, exact: {} },
    enabled: true,
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
    created_at: 0,
    updated_at: 0,
    auth_mode: "api_key",
    oauth_provider_type: null,
    oauth_email: null,
    oauth_expires_at: null,
    oauth_last_error: null,
    source_provider_id: null,
    bridge_type: null,
    availability_test_model: null,
    api_key_configured: true,
    newapi_account_user_id: null,
    newapi_account_access_token_configured: false,
    stream_idle_timeout_seconds: null,
    extension_values: [],
    upstream_retry_policy_override: null,
    ...partial,
  };
}

function makeCatalog(overrides: Partial<ProviderModelCatalog> = {}): ProviderModelCatalog {
  return {
    providerId: 7,
    providerUuid: PROVIDER_UUID,
    protocol: "openai_compatible",
    stale: false,
    lastAttemptAt: 100,
    lastSuccessAt: 100,
    lastErrorCode: null,
    models: [
      {
        modelUuid: MODEL_UUID,
        providerId: 7,
        remoteModelId: "same-model",
        source: "discovered",
        stale: false,
        lastSeenAt: 100,
        createdAt: 90,
        updatedAt: 100,
      },
    ],
    ...overrides,
  };
}

function makeProfile(overrides: Partial<CodexManagedProfile> = {}): CodexManagedProfile {
  return {
    profileUuid: PROFILE_UUID,
    profileName: "grok-work",
    modelUuid: MODEL_UUID,
    providerId: 7,
    providerUuid: PROVIDER_UUID,
    providerName: "Grok Provider",
    remoteModelId: "same-model",
    canonicalModel: "aio/grok-work",
    fileStatus: "managed",
    createdAt: 100,
    updatedAt: 100,
    ...overrides,
  };
}

function renderDialog(provider = makeProvider()) {
  const client = createTestQueryClient();
  return render(
    <QueryClientProvider client={client}>
      <ProviderModelCatalogDialog open provider={provider} onOpenChange={vi.fn()} />
    </QueryClientProvider>
  );
}

describe("pages/providers/ProviderModelCatalogDialog", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(providerModelsGet).mockResolvedValue(makeCatalog());
    vi.mocked(providerModelsRefresh).mockRejectedValue(new Error("unused"));
    vi.mocked(providerModelManualUpsert).mockRejectedValue(new Error("unused"));
    vi.mocked(providerModelManualDelete).mockRejectedValue(new Error("unused"));
    vi.mocked(codexManagedProfilesList).mockResolvedValue([]);
    vi.mocked(codexManagedProfileCreate).mockRejectedValue(new Error("unused"));
    vi.mocked(codexManagedProfileDelete).mockRejectedValue(new Error("unused"));
    vi.mocked(cliProxyStatusAll).mockResolvedValue([
      {
        cli_key: "codex",
        enabled: true,
        base_origin: "http://127.0.0.1:37123",
        current_gateway_origin: "http://127.0.0.1:37123",
        applied_to_current_gateway: true,
      },
    ]);
  });

  it("shows stale discovery state and keeps manual models usable", async () => {
    vi.mocked(providerModelsGet).mockResolvedValueOnce(
      makeCatalog({
        stale: true,
        lastErrorCode: "timeout",
        models: [
          {
            modelUuid: MODEL_UUID,
            providerId: 7,
            remoteModelId: "manual-model",
            source: "manual",
            stale: true,
            lastSeenAt: null,
            createdAt: 90,
            updatedAt: 100,
          },
        ],
      })
    );

    renderDialog();

    expect(await screen.findByText("manual-model")).toBeInTheDocument();
    expect(screen.getByText("手工")).toBeInTheDocument();
    expect(screen.getByText("已过期")).toBeInTheDocument();
    expect(screen.getByText(/模型接口请求超时/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "创建 Profile" })).toBeEnabled();
  });

  it("creates a profile for the selected provider-scoped model without merging a same-name model", async () => {
    vi.mocked(codexManagedProfilesList).mockResolvedValueOnce([
      makeProfile({
        profileUuid: "55555555-5555-4555-8555-555555555555",
        profileName: "other-provider-profile",
        modelUuid: OTHER_MODEL_UUID,
        providerId: 8,
        providerUuid: "22222222-2222-4222-8222-222222222222",
        providerName: "Other Provider",
        canonicalModel: "aio/other-provider-profile",
      }),
    ]);
    vi.mocked(codexManagedProfileCreate).mockResolvedValueOnce(
      makeProfile({ profileName: "same-model-work", canonicalModel: "aio/same-model-work" })
    );

    renderDialog();

    expect(await screen.findByText("same-model")).toBeInTheDocument();
    expect(screen.queryByText("other-provider-profile")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "创建 Profile" }));

    const createDialog = screen
      .getByRole("heading", { name: "创建 Codex Profile" })
      .closest('[role="dialog"]') as HTMLElement | null;
    if (!createDialog) throw new Error("创建 Profile 对话框不存在");
    const profileNameInput = within(createDialog).getByRole("textbox", {
      name: "Profile 名称",
    });
    expect(profileNameInput).toHaveValue("same-model");
    fireEvent.change(profileNameInput, { target: { value: "same-model-work" } });
    fireEvent.click(within(createDialog).getByRole("button", { name: "创建" }));

    await waitFor(() =>
      expect(codexManagedProfileCreate).toHaveBeenCalledWith("same-model-work", MODEL_UUID)
    );
    expect(codexManagedProfileCreate).not.toHaveBeenCalledWith(expect.anything(), OTHER_MODEL_UUID);
    await waitFor(() =>
      expect(vi.mocked(toast)).toHaveBeenCalledWith(
        "Profile same-model-work 已创建；请新建或重启 Codex 会话，然后通过 /model 选择 aio/same-model-work"
      )
    );
  });

  it("warns that an externally modified profile file will be preserved when deleting", async () => {
    vi.mocked(codexManagedProfilesList).mockResolvedValueOnce([
      makeProfile({
        profileName: "modified-profile",
        canonicalModel: "aio/modified-profile",
        fileStatus: "modified",
      }),
    ]);
    vi.mocked(codexManagedProfileDelete).mockResolvedValueOnce({
      deleted: true,
      externalFilePreserved: true,
    });

    renderDialog();

    expect(await screen.findByText("aio/modified-profile")).toBeInTheDocument();
    expect(screen.getByText("文件已修改")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "删除 Profile modified-profile" }));

    const deleteDialog = screen
      .getByRole("heading", { name: "删除 Codex Profile" })
      .closest('[role="dialog"]') as HTMLElement | null;
    if (!deleteDialog) throw new Error("删除 Profile 对话框不存在");
    expect(within(deleteDialog).getByText(/文件已被外部修改，将保留原文件/)).toBeInTheDocument();
    fireEvent.click(within(deleteDialog).getByRole("button", { name: "删除" }));

    await waitFor(() => expect(codexManagedProfileDelete).toHaveBeenCalledWith(PROFILE_UUID));
    await waitFor(() =>
      expect(vi.mocked(toast)).toHaveBeenCalledWith(
        "Profile modified-profile 已解除管理；外部修改的文件已保留"
      )
    );
  });

  it("disables every write entry while a catalog refresh is pending", async () => {
    const refresh = deferred<ProviderModelCatalog>();
    vi.mocked(providerModelsGet).mockResolvedValueOnce(
      makeCatalog({
        models: [
          {
            modelUuid: MODEL_UUID,
            providerId: 7,
            remoteModelId: "manual-model",
            source: "manual",
            stale: false,
            lastSeenAt: null,
            createdAt: 90,
            updatedAt: 100,
          },
        ],
      })
    );
    vi.mocked(providerModelsRefresh).mockReturnValueOnce(refresh.promise);

    renderDialog();

    expect(await screen.findByText("manual-model")).toBeInTheDocument();
    const manualInput = screen.getByRole("textbox", { name: "手工输入远端模型 ID" });
    fireEvent.change(manualInput, { target: { value: "another-model" } });
    fireEvent.click(screen.getByRole("button", { name: "刷新模型" }));

    await waitFor(() => expect(screen.getByRole("button", { name: "获取中…" })).toBeDisabled());
    expect(manualInput).toBeDisabled();
    expect(screen.getByRole("button", { name: "添加" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "创建 Profile" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "删除手工模型 manual-model" })).toBeDisabled();

    refresh.resolve(makeCatalog());
    await waitFor(() => expect(screen.getByRole("button", { name: "刷新模型" })).toBeEnabled());
  });

  it("shows a profile read error and retries without hiding the model catalog", async () => {
    vi.mocked(codexManagedProfilesList)
      .mockRejectedValueOnce(
        new Error("UNKNOWN_FAILURE: https://example.test/v1?api_key=SYNTHETIC_PROFILE_SECRET")
      )
      .mockResolvedValueOnce([makeProfile()]);

    renderDialog();

    expect(await screen.findByText("same-model")).toBeInTheDocument();
    expect(screen.getByText("读取 Codex Profile 失败")).toBeInTheDocument();
    expect(screen.getByText("请稍后重试")).toBeInTheDocument();
    expect(screen.queryByText(/SYNTHETIC_PROFILE_SECRET/)).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "重试 Profile" }));

    expect(await screen.findByText("aio/grok-work")).toBeInTheDocument();
    expect(screen.queryByText("读取 Codex Profile 失败")).not.toBeInTheDocument();
    expect(codexManagedProfilesList).toHaveBeenCalledTimes(2);
  });

  it("requires the Codex CLI proxy before opening profile creation", async () => {
    vi.mocked(cliProxyStatusAll).mockResolvedValueOnce([
      {
        cli_key: "codex",
        enabled: false,
        base_origin: null,
        current_gateway_origin: "http://127.0.0.1:37123",
        applied_to_current_gateway: null,
      },
    ]);

    renderDialog();

    expect(await screen.findByText(/请先在 CLI 代理中开启 Codex 代理/)).toBeInTheDocument();
    expect(await screen.findByText("same-model")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "创建 Profile" })).toBeDisabled();
  });
});
