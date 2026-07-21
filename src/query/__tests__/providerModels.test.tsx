import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { CodexManagedProfile } from "../../services/providers/codexManagedProfiles";
import type { ProviderModelCatalog } from "../../services/providers/providerModels";
import {
  codexManagedProfileCreate,
  codexManagedProfileDelete,
  codexManagedProfilesList,
} from "../../services/providers/codexManagedProfiles";
import {
  providerModelManualDelete,
  providerModelManualUpsert,
  providerModelsGet,
  providerModelsRefresh,
} from "../../services/providers/providerModels";
import { createQueryWrapper, createTestQueryClient } from "../../test/utils/reactQuery";
import { codexManagedProfilesKeys, providerModelsKeys } from "../keys";
import {
  advanceProviderModelIdentityGenerationsForProviderId,
  advanceProviderModelsGlobalGeneration,
  invalidateProviderModelCatalog,
  useCodexManagedProfileCreateMutation,
  useCodexManagedProfileDeleteMutation,
  useCodexManagedProfilesQuery,
  useProviderModelCatalogQuery,
  useProviderModelsRefreshMutation,
} from "../providerModels";
import { resetAppDataQueryCaches } from "../dataManagement";

vi.mock("../../services/providers/providerModels", async () => {
  const actual = await vi.importActual<typeof import("../../services/providers/providerModels")>(
    "../../services/providers/providerModels"
  );
  return {
    ...actual,
    providerModelsGet: vi.fn(),
    providerModelsRefresh: vi.fn(),
    providerModelManualUpsert: vi.fn(),
    providerModelManualDelete: vi.fn(),
  };
});

vi.mock("../../services/providers/codexManagedProfiles", async () => {
  const actual = await vi.importActual<
    typeof import("../../services/providers/codexManagedProfiles")
  >("../../services/providers/codexManagedProfiles");
  return {
    ...actual,
    codexManagedProfilesList: vi.fn(),
    codexManagedProfileCreate: vi.fn(),
    codexManagedProfileDelete: vi.fn(),
  };
});

const PROVIDER_UUIDS = {
  7: "11111111-1111-4111-8111-111111111111",
  8: "22222222-2222-4222-8222-222222222222",
} as const;

function catalog(
  providerId: 7 | 8,
  suffix: string,
  providerUuid: string = PROVIDER_UUIDS[providerId]
): ProviderModelCatalog {
  return {
    providerId,
    providerUuid,
    protocol: "openai_compatible",
    stale: false,
    lastAttemptAt: 100,
    lastSuccessAt: 100,
    lastErrorCode: null,
    models: [
      {
        modelUuid:
          providerId === 7
            ? "33333333-3333-4333-8333-333333333333"
            : "44444444-4444-4444-8444-444444444444",
        providerId,
        remoteModelId: "same-model",
        source: "discovered",
        stale: false,
        lastSeenAt: 100,
        createdAt: 90,
        updatedAt: 100 + suffix.length,
      },
    ],
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((nextResolve) => {
    resolve = nextResolve;
  });
  return { promise, resolve };
}

describe("query/providerModels", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(codexManagedProfilesList).mockResolvedValue([]);
    vi.mocked(codexManagedProfileCreate).mockRejectedValue(new Error("unused"));
    vi.mocked(codexManagedProfileDelete).mockRejectedValue(new Error("unused"));
    vi.mocked(providerModelManualUpsert).mockRejectedValue(new Error("unused"));
    vi.mocked(providerModelManualDelete).mockRejectedValue(new Error("unused"));
  });

  it("keeps identical remote model IDs isolated by provider cache key", async () => {
    vi.mocked(providerModelsGet).mockImplementation(async (providerId) =>
      catalog(providerId as 7 | 8, "query")
    );
    const client = createTestQueryClient();
    const wrapper = createQueryWrapper(client);

    const first = renderHook(() => useProviderModelCatalogQuery(7, PROVIDER_UUIDS[7]), { wrapper });
    const second = renderHook(() => useProviderModelCatalogQuery(8, PROVIDER_UUIDS[8]), {
      wrapper,
    });
    await waitFor(() => expect(first.result.current.isSuccess).toBe(true));
    await waitFor(() => expect(second.result.current.isSuccess).toBe(true));

    expect(first.result.current.data?.models[0].remoteModelId).toBe("same-model");
    expect(second.result.current.data?.models[0].remoteModelId).toBe("same-model");
    expect(first.result.current.data?.models[0].modelUuid).not.toBe(
      second.result.current.data?.models[0].modelUuid
    );
    expect(client.getQueryData(providerModelsKeys.catalog(7, PROVIDER_UUIDS[7]))).toEqual(
      catalog(7, "query")
    );
    expect(client.getQueryData(providerModelsKeys.catalog(8, PROVIDER_UUIDS[8]))).toEqual(
      catalog(8, "query")
    );
  });

  it("does not expose the previous provider catalog while a new provider is loading", async () => {
    const pendingSecond = deferred<ProviderModelCatalog>();
    vi.mocked(providerModelsGet).mockImplementation((providerId) => {
      if (providerId === 7) return Promise.resolve(catalog(7, "first"));
      return pendingSecond.promise;
    });
    const client = createTestQueryClient();
    const wrapper = createQueryWrapper(client);
    const initialProps: { providerId: number; providerUuid: string } = {
      providerId: 7,
      providerUuid: PROVIDER_UUIDS[7],
    };
    const query = renderHook(
      ({ providerId, providerUuid }: { providerId: number; providerUuid: string }) =>
        useProviderModelCatalogQuery(providerId, providerUuid),
      {
        initialProps,
        wrapper,
      }
    );
    await waitFor(() => expect(query.result.current.data?.providerId).toBe(7));

    query.rerender({ providerId: 8, providerUuid: PROVIDER_UUIDS[8] });

    expect(query.result.current.data).toBeUndefined();
    pendingSecond.resolve(catalog(8, "second"));
    await waitFor(() => expect(query.result.current.data?.providerId).toBe(8));
  });

  it("prevents a late uncancellable catalog read from replacing a refresh result", async () => {
    const pendingRead = deferred<ProviderModelCatalog>();
    const refreshed = catalog(7, "refreshed");
    vi.mocked(providerModelsGet).mockReturnValueOnce(pendingRead.promise);
    vi.mocked(providerModelsRefresh).mockResolvedValueOnce(refreshed);
    const client = createTestQueryClient();
    const wrapper = createQueryWrapper(client);

    renderHook(() => useProviderModelCatalogQuery(7, PROVIDER_UUIDS[7]), { wrapper });
    await waitFor(() => expect(providerModelsGet).toHaveBeenCalledWith(7, PROVIDER_UUIDS[7]));
    const mutation = renderHook(() => useProviderModelsRefreshMutation(), { wrapper });

    await act(async () => {
      await mutation.result.current.mutateAsync({
        providerId: 7,
        providerUuid: PROVIDER_UUIDS[7],
      });
    });
    expect(client.getQueryData(providerModelsKeys.catalog(7, PROVIDER_UUIDS[7]))).toEqual(
      refreshed
    );

    pendingRead.resolve(catalog(7, "old"));
    await act(async () => {
      await pendingRead.promise;
      await Promise.resolve();
    });
    expect(client.getQueryData(providerModelsKeys.catalog(7, PROVIDER_UUIDS[7]))).toEqual(
      refreshed
    );
  });

  it("cancels a late catalog read and invalidates only the changed provider", async () => {
    const pendingRead = deferred<ProviderModelCatalog>();
    vi.mocked(providerModelsGet).mockReturnValueOnce(pendingRead.promise);
    const client = createTestQueryClient();
    client.setQueryData(providerModelsKeys.catalog(8, PROVIDER_UUIDS[8]), catalog(8, "other"));

    const read = client
      .fetchQuery({
        queryKey: providerModelsKeys.catalog(7, PROVIDER_UUIDS[7]),
        queryFn: () => providerModelsGet(7, PROVIDER_UUIDS[7]),
      })
      .catch(() => undefined);
    await waitFor(() => expect(providerModelsGet).toHaveBeenCalledWith(7, PROVIDER_UUIDS[7]));

    await invalidateProviderModelCatalog(client, 7, PROVIDER_UUIDS[7]);
    pendingRead.resolve(catalog(7, "old"));
    await read;

    expect(client.getQueryData(providerModelsKeys.catalog(7, PROVIDER_UUIDS[7]))).toBeUndefined();
    expect(
      client.getQueryState(providerModelsKeys.catalog(7, PROVIDER_UUIDS[7]))?.isInvalidated
    ).toBe(true);
    expect(client.getQueryData(providerModelsKeys.catalog(8, PROVIDER_UUIDS[8]))).toEqual(
      catalog(8, "other")
    );
    expect(
      client.getQueryState(providerModelsKeys.catalog(8, PROVIDER_UUIDS[8]))?.isInvalidated
    ).toBe(false);
  });

  it("keeps a provider B mutation result when provider A is invalidated", async () => {
    const pendingRefresh = deferred<ProviderModelCatalog>();
    vi.mocked(providerModelsRefresh).mockReturnValueOnce(pendingRefresh.promise);
    const client = createTestQueryClient();
    const providerAKey = providerModelsKeys.catalog(7, PROVIDER_UUIDS[7]);
    const providerBKey = providerModelsKeys.catalog(8, PROVIDER_UUIDS[8]);
    client.setQueryData(providerAKey, catalog(7, "before-edit"));
    const wrapper = createQueryWrapper(client);
    const mutation = renderHook(() => useProviderModelsRefreshMutation(), { wrapper });

    let mutationPromise!: Promise<ProviderModelCatalog>;
    act(() => {
      mutationPromise = mutation.result.current.mutateAsync({
        providerId: 8,
        providerUuid: PROVIDER_UUIDS[8],
      });
    });
    await waitFor(() => expect(providerModelsRefresh).toHaveBeenCalledWith(8, PROVIDER_UUIDS[8]));

    await invalidateProviderModelCatalog(client, 7, PROVIDER_UUIDS[7]);
    const refreshedProviderB = catalog(8, "refreshed");
    pendingRefresh.resolve(refreshedProviderB);
    await act(async () => {
      await mutationPromise;
    });

    expect(client.getQueryState(providerAKey)?.isInvalidated).toBe(true);
    expect(client.getQueryData(providerBKey)).toEqual(refreshedProviderB);
  });

  it("drops a late mutation result after its provider identity is invalidated", async () => {
    const pendingRefresh = deferred<ProviderModelCatalog>();
    vi.mocked(providerModelsRefresh).mockReturnValueOnce(pendingRefresh.promise);
    const client = createTestQueryClient();
    const key = providerModelsKeys.catalog(7, PROVIDER_UUIDS[7]);
    const postEditCatalog = { ...catalog(7, "post-edit"), stale: true };
    const wrapper = createQueryWrapper(client);
    const mutation = renderHook(() => useProviderModelsRefreshMutation(), { wrapper });

    let mutationPromise!: Promise<ProviderModelCatalog>;
    act(() => {
      mutationPromise = mutation.result.current.mutateAsync({
        providerId: 7,
        providerUuid: PROVIDER_UUIDS[7],
      });
    });
    await waitFor(() => expect(providerModelsRefresh).toHaveBeenCalledWith(7, PROVIDER_UUIDS[7]));

    await invalidateProviderModelCatalog(client, 7, PROVIDER_UUIDS[7]);
    client.setQueryData(key, postEditCatalog);
    pendingRefresh.resolve(catalog(7, "pre-edit"));
    await act(async () => {
      await mutationPromise;
    });

    expect(client.getQueryData(key)).toEqual(postEditCatalog);
  });

  it("can invalidate a catalog without dropping a legitimate in-flight refresh", async () => {
    const pendingRefresh = deferred<ProviderModelCatalog>();
    vi.mocked(providerModelsRefresh).mockReturnValueOnce(pendingRefresh.promise);
    const client = createTestQueryClient();
    const key = providerModelsKeys.catalog(7, PROVIDER_UUIDS[7]);
    const wrapper = createQueryWrapper(client);
    const mutation = renderHook(() => useProviderModelsRefreshMutation(), { wrapper });

    let mutationPromise!: Promise<ProviderModelCatalog>;
    act(() => {
      mutationPromise = mutation.result.current.mutateAsync({
        providerId: 7,
        providerUuid: PROVIDER_UUIDS[7],
      });
    });
    await waitFor(() => expect(providerModelsRefresh).toHaveBeenCalledWith(7, PROVIDER_UUIDS[7]));

    await invalidateProviderModelCatalog(client, 7, PROVIDER_UUIDS[7], {
      advanceGeneration: false,
    });
    const refreshed = catalog(7, "after-display-edit");
    pendingRefresh.resolve(refreshed);
    await act(async () => {
      await mutationPromise;
    });

    expect(client.getQueryData(key)).toEqual(refreshed);
  });

  it("keeps a late mutation for a deleted identity away from a reused provider ID", async () => {
    const replacementUuid = "77777777-7777-4777-8777-777777777777";
    const pendingRefresh = deferred<ProviderModelCatalog>();
    vi.mocked(providerModelsRefresh).mockReturnValueOnce(pendingRefresh.promise);
    const client = createTestQueryClient();
    const replacementCatalog = catalog(7, "replacement", replacementUuid);
    client.setQueryData(providerModelsKeys.catalog(7, replacementUuid), replacementCatalog);
    const wrapper = createQueryWrapper(client);
    const mutation = renderHook(() => useProviderModelsRefreshMutation(), { wrapper });

    let mutationPromise!: Promise<ProviderModelCatalog>;
    act(() => {
      mutationPromise = mutation.result.current.mutateAsync({
        providerId: 7,
        providerUuid: PROVIDER_UUIDS[7],
      });
    });
    await waitFor(() => expect(providerModelsRefresh).toHaveBeenCalledWith(7, PROVIDER_UUIDS[7]));
    pendingRefresh.resolve(catalog(7, "old-identity"));
    await act(async () => {
      await mutationPromise;
    });

    expect(client.getQueryData(providerModelsKeys.catalog(7, replacementUuid))).toEqual(
      replacementCatalog
    );
  });

  it("drops a late mutation result after a destructive cache epoch change", async () => {
    const pendingRefresh = deferred<ProviderModelCatalog>();
    vi.mocked(providerModelsRefresh).mockReturnValueOnce(pendingRefresh.promise);
    const client = createTestQueryClient();
    const importedCatalog = { ...catalog(7, "imported"), stale: true };
    const key = providerModelsKeys.catalog(7, PROVIDER_UUIDS[7]);
    const wrapper = createQueryWrapper(client);
    const mutation = renderHook(() => useProviderModelsRefreshMutation(), { wrapper });

    let mutationPromise!: Promise<ProviderModelCatalog>;
    act(() => {
      mutationPromise = mutation.result.current.mutateAsync({
        providerId: 7,
        providerUuid: PROVIDER_UUIDS[7],
      });
    });
    await waitFor(() => expect(providerModelsRefresh).toHaveBeenCalledWith(7, PROVIDER_UUIDS[7]));
    advanceProviderModelsGlobalGeneration(client);
    client.setQueryData(key, importedCatalog);
    pendingRefresh.resolve(catalog(7, "pre-import"));
    await act(async () => {
      await mutationPromise;
    });

    expect(client.getQueryData(key)).toEqual(importedCatalog);
  });

  it("removes only the deleted profile and preserves the backend file result", async () => {
    const profiles: CodexManagedProfile[] = [
      {
        profileUuid: "55555555-5555-4555-8555-555555555555",
        profileName: "first",
        modelUuid: "33333333-3333-4333-8333-333333333333",
        providerId: 7,
        providerUuid: PROVIDER_UUIDS[7],
        providerName: "P7",
        remoteModelId: "same-model",
        canonicalModel: "aio/33333333-3333-4333-8333-333333333333",
        fileStatus: "modified",
        createdAt: 1,
        updatedAt: 1,
      },
      {
        profileUuid: "66666666-6666-4666-8666-666666666666",
        profileName: "second",
        modelUuid: "44444444-4444-4444-8444-444444444444",
        providerId: 8,
        providerUuid: PROVIDER_UUIDS[8],
        providerName: "P8",
        remoteModelId: "same-model",
        canonicalModel: "aio/44444444-4444-4444-8444-444444444444",
        fileStatus: "managed",
        createdAt: 1,
        updatedAt: 1,
      },
    ];
    vi.mocked(codexManagedProfileDelete).mockResolvedValueOnce({
      deleted: true,
      externalFilePreserved: true,
    });
    const client = createTestQueryClient();
    client.setQueryData(codexManagedProfilesKeys.list(), profiles);
    const wrapper = createQueryWrapper(client);
    const mutation = renderHook(() => useCodexManagedProfileDeleteMutation(), { wrapper });

    let result: Awaited<ReturnType<typeof codexManagedProfileDelete>> | undefined;
    await act(async () => {
      result = await mutation.result.current.mutateAsync({
        profileUuid: profiles[0].profileUuid,
        providerId: 7,
        providerUuid: PROVIDER_UUIDS[7],
      });
    });

    expect(result).toEqual({ deleted: true, externalFilePreserved: true });
    expect(client.getQueryData(codexManagedProfilesKeys.list())).toEqual([profiles[1]]);
    expect(codexManagedProfileDelete).toHaveBeenCalledWith(profiles[0].profileUuid);
  });

  it("reconciles a late create after import without cancelling the import refetch", async () => {
    const importedProfiles: CodexManagedProfile[] = [];
    const finalProfiles: CodexManagedProfile[] = [
      {
        profileUuid: "77777777-7777-4777-8777-777777777777",
        profileName: "created-after-import",
        modelUuid: "33333333-3333-4333-8333-333333333333",
        providerId: 7,
        providerUuid: PROVIDER_UUIDS[7],
        providerName: "P7",
        remoteModelId: "same-model",
        canonicalModel: "aio/33333333-3333-4333-8333-333333333333",
        fileStatus: "managed",
        createdAt: 2,
        updatedAt: 2,
      },
    ];
    const importRefetch = deferred<CodexManagedProfile[]>();
    const postMutationRefetch = deferred<CodexManagedProfile[]>();
    const createResult = deferred<CodexManagedProfile>();
    vi.mocked(codexManagedProfilesList)
      .mockResolvedValueOnce([])
      .mockReturnValueOnce(importRefetch.promise)
      .mockReturnValueOnce(postMutationRefetch.promise);
    vi.mocked(codexManagedProfileCreate).mockReturnValueOnce(createResult.promise);

    const client = createTestQueryClient();
    const wrapper = createQueryWrapper(client);
    const profilesQuery = renderHook(() => useCodexManagedProfilesQuery(), { wrapper });
    await waitFor(() => expect(profilesQuery.result.current.isSuccess).toBe(true));
    const mutation = renderHook(() => useCodexManagedProfileCreateMutation(), { wrapper });
    let mutationPromise!: Promise<CodexManagedProfile>;
    act(() => {
      mutationPromise = mutation.result.current.mutateAsync({
        profileName: "created-after-import",
        modelUuid: finalProfiles[0].modelUuid,
        providerId: 7,
        providerUuid: PROVIDER_UUIDS[7],
      });
    });
    await waitFor(() =>
      expect(codexManagedProfileCreate).toHaveBeenCalledWith(
        "created-after-import",
        finalProfiles[0].modelUuid
      )
    );

    advanceProviderModelsGlobalGeneration(client);
    const key = codexManagedProfilesKeys.list();
    const importPromise = client.refetchQueries({ queryKey: key, exact: true, type: "all" });
    await waitFor(() => expect(codexManagedProfilesList).toHaveBeenCalledTimes(2));
    importRefetch.resolve(importedProfiles);
    await importPromise;

    createResult.resolve(finalProfiles[0]);
    await waitFor(() => expect(codexManagedProfilesList).toHaveBeenCalledTimes(3));
    postMutationRefetch.resolve(finalProfiles);
    await act(async () => {
      await mutationPromise;
    });

    expect(client.getQueryData(key)).toEqual(finalProfiles);
  });

  it("reconciles a late delete after import without restoring the deleted profile", async () => {
    const target: CodexManagedProfile = {
      profileUuid: "88888888-8888-4888-8888-888888888888",
      profileName: "delete-after-import",
      modelUuid: "33333333-3333-4333-8333-333333333333",
      providerId: 7,
      providerUuid: PROVIDER_UUIDS[7],
      providerName: "P7",
      remoteModelId: "same-model",
      canonicalModel: "aio/33333333-3333-4333-8333-333333333333",
      fileStatus: "managed",
      createdAt: 1,
      updatedAt: 1,
    };
    const importedProfiles: CodexManagedProfile[] = [];
    const finalProfiles: CodexManagedProfile[] = [];
    const importRefetch = deferred<CodexManagedProfile[]>();
    const postMutationRefetch = deferred<CodexManagedProfile[]>();
    const deleteResult = deferred<Awaited<ReturnType<typeof codexManagedProfileDelete>>>();
    vi.mocked(codexManagedProfilesList)
      .mockResolvedValueOnce([target])
      .mockReturnValueOnce(importRefetch.promise)
      .mockReturnValueOnce(postMutationRefetch.promise);
    vi.mocked(codexManagedProfileDelete).mockReturnValueOnce(deleteResult.promise);

    const client = createTestQueryClient();
    const wrapper = createQueryWrapper(client);
    const profilesQuery = renderHook(() => useCodexManagedProfilesQuery(), { wrapper });
    await waitFor(() => expect(profilesQuery.result.current.data).toEqual([target]));
    const mutation = renderHook(() => useCodexManagedProfileDeleteMutation(), { wrapper });
    let mutationPromise!: Promise<Awaited<ReturnType<typeof codexManagedProfileDelete>>>;
    act(() => {
      mutationPromise = mutation.result.current.mutateAsync({
        profileUuid: target.profileUuid,
        providerId: target.providerId,
        providerUuid: target.providerUuid,
      });
    });
    await waitFor(() => expect(codexManagedProfileDelete).toHaveBeenCalledWith(target.profileUuid));

    advanceProviderModelsGlobalGeneration(client);
    const key = codexManagedProfilesKeys.list();
    const importPromise = client.refetchQueries({ queryKey: key, exact: true, type: "all" });
    await waitFor(() => expect(codexManagedProfilesList).toHaveBeenCalledTimes(2));
    importRefetch.resolve(importedProfiles);
    await importPromise;

    deleteResult.resolve({ deleted: true, externalFilePreserved: false });
    await waitFor(() => expect(codexManagedProfilesList).toHaveBeenCalledTimes(3));
    postMutationRefetch.resolve(finalProfiles);
    await act(async () => {
      await mutationPromise;
    });

    expect(client.getQueryData(key)).toEqual(finalProfiles);
  });

  it("does not recreate a profile query removed by data reset", async () => {
    const createResult = deferred<CodexManagedProfile>();
    vi.mocked(codexManagedProfilesList).mockResolvedValueOnce([]);
    vi.mocked(codexManagedProfileCreate).mockReturnValueOnce(createResult.promise);
    const client = createTestQueryClient();
    const key = codexManagedProfilesKeys.list();
    await client.fetchQuery({ queryKey: key, queryFn: codexManagedProfilesList });
    const wrapper = createQueryWrapper(client);
    const mutation = renderHook(() => useCodexManagedProfileCreateMutation(), { wrapper });
    let mutationPromise!: Promise<CodexManagedProfile>;
    act(() => {
      mutationPromise = mutation.result.current.mutateAsync({
        profileName: "reset-race",
        modelUuid: "33333333-3333-4333-8333-333333333333",
        providerId: 7,
        providerUuid: PROVIDER_UUIDS[7],
      });
    });
    await waitFor(() => expect(codexManagedProfileCreate).toHaveBeenCalled());

    await resetAppDataQueryCaches(client);
    createResult.resolve({
      profileUuid: "99999999-9999-4999-8999-999999999999",
      profileName: "reset-race",
      modelUuid: "33333333-3333-4333-8333-333333333333",
      providerId: 7,
      providerUuid: PROVIDER_UUIDS[7],
      providerName: "P7",
      remoteModelId: "same-model",
      canonicalModel: "aio/33333333-3333-4333-8333-333333333333",
      fileStatus: "managed",
      createdAt: 1,
      updatedAt: 1,
    });
    await act(async () => {
      await mutationPromise;
    });

    expect(client.getQueryState(key)).toBeUndefined();
    expect(codexManagedProfilesList).toHaveBeenCalledTimes(1);
  });

  it("does not create a profile cache after its provider identity is retired", async () => {
    const createResult = deferred<CodexManagedProfile>();
    vi.mocked(codexManagedProfileCreate).mockReturnValueOnce(createResult.promise);
    const client = createTestQueryClient();
    const wrapper = createQueryWrapper(client);
    const mutation = renderHook(() => useCodexManagedProfileCreateMutation(), { wrapper });

    let mutationPromise!: Promise<CodexManagedProfile>;
    act(() => {
      mutationPromise = mutation.result.current.mutateAsync({
        profileName: "deleted-provider-race",
        modelUuid: "33333333-3333-4333-8333-333333333333",
        providerId: 7,
        providerUuid: PROVIDER_UUIDS[7],
      });
    });
    await waitFor(() => expect(codexManagedProfileCreate).toHaveBeenCalled());

    advanceProviderModelIdentityGenerationsForProviderId(client, 7);
    createResult.resolve({
      profileUuid: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
      profileName: "deleted-provider-race",
      modelUuid: "33333333-3333-4333-8333-333333333333",
      providerId: 7,
      providerUuid: PROVIDER_UUIDS[7],
      providerName: "P7",
      remoteModelId: "same-model",
      canonicalModel: "aio/33333333-3333-4333-8333-333333333333",
      fileStatus: "managed",
      createdAt: 1,
      updatedAt: 1,
    });
    await act(async () => {
      await mutationPromise;
    });

    expect(client.getQueryState(codexManagedProfilesKeys.list())).toBeUndefined();
  });
});
