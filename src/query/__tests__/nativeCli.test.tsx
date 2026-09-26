import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import * as native from "../../services/nativeCli";
import * as gateway from "../../services/nativeGateway";
import { deferred, nativeList, nativeTarget, nativeModel } from "../../test/fixtures/native";
import { createQueryWrapper, createTestQueryClient } from "../../test/utils/reactQuery";
import { invalidateNativeTarget, nativeCliKeys, useNativeCliProvidersQuery } from "../nativeCli";
import { useNativeGatewayModelsMutation } from "../nativeGateway";
vi.mock("../../services/nativeCli", async (original) => ({
  ...(await original<typeof native>()),
  nativeCliProvidersList: vi.fn(),
}));
vi.mock("../../services/nativeGateway", async (original) => ({
  ...(await original<typeof gateway>()),
  nativeGatewayModelsSet: vi.fn(),
}));
beforeEach(() => vi.clearAllMocks());
describe("native target and provider identity fences", () => {
  it("does not display previous target data and ignores reverse completion on target switches", async () => {
    const a = deferred<ReturnType<typeof nativeList>>();
    const b = deferred<ReturnType<typeof nativeList>>();
    vi.mocked(native.nativeCliProvidersList).mockImplementation((target) =>
      target === "a" ? a.promise : b.promise
    );
    const client = createTestQueryClient();
    const { result, rerender } = renderHook(({ target }) => useNativeCliProvidersQuery(target), {
      initialProps: { target: "a" },
      wrapper: createQueryWrapper(client),
    });
    rerender({ target: "b" });
    expect(result.current.data).toBeUndefined();
    await act(async () => b.resolve(nativeList({ target: nativeTarget({ targetId: "b" }) })));
    await waitFor(() => expect(result.current.data?.target.targetId).toBe("b"));
    await act(async () => a.resolve(nativeList({ target: nativeTarget({ targetId: "a" }) })));
    expect(result.current.data?.target.targetId).toBe("b");
  });
  it("cancels old reads before invalidating a mutated native target", async () => {
    const old = deferred<ReturnType<typeof nativeList>>();
    vi.mocked(native.nativeCliProvidersList)
      .mockReturnValueOnce(old.promise)
      .mockResolvedValue(nativeList({ revision: "file-2" }));
    const client = createTestQueryClient();
    const { result } = renderHook(() => useNativeCliProvidersQuery("pi:local:a"), {
      wrapper: createQueryWrapper(client),
    });
    await waitFor(() => expect(native.nativeCliProvidersList).toHaveBeenCalledOnce());
    await act(async () => invalidateNativeTarget(client, "pi:local:a"));
    await waitFor(() => expect(result.current.data?.revision).toBe("file-2"));
    await act(async () => old.resolve(nativeList({ revision: "file-1" })));
    expect(result.current.data?.revision).toBe("file-2");
    expect(client.getQueryData(nativeCliKeys.providers("pi:local:a"))).toEqual(
      nativeList({ revision: "file-2" })
    );
  });
  it("saves declarations with the immutable provider UUID and edited revision", async () => {
    vi.mocked(gateway.nativeGatewayModelsSet).mockResolvedValue({
      providerId: 7,
      providerUuid: "uuid-current",
      revision: "models-2",
      stale: false,
      models: [nativeModel()],
    });
    const client = createTestQueryClient();
    const { result } = renderHook(() => useNativeGatewayModelsMutation(7, "uuid-current"), {
      wrapper: createQueryWrapper(client),
    });
    await act(async () =>
      result.current.mutateAsync({ expectedRevision: "models-1", models: [nativeModel()] })
    );
    expect(gateway.nativeGatewayModelsSet).toHaveBeenCalledWith(7, "uuid-current", "models-1", [
      nativeModel(),
    ]);
  });
});
