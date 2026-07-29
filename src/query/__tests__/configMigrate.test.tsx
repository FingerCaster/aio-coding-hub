import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { configExport, configImport } from "../../services/app/configMigrate";
import { createQueryWrapper, createTestQueryClient } from "../../test/utils/reactQuery";
import {
  cliProxyKeys,
  codexManagedProfilesKeys,
  gatewayKeys,
  mcpKeys,
  promptsKeys,
  providerModelsKeys,
  providersKeys,
  settingsKeys,
  skillsKeys,
  sortModesKeys,
  workspacesKeys,
  wslKeys,
} from "../keys";
import { useConfigExportMutation, useConfigImportMutation } from "../configMigrate";

const PROVIDER_UUID = "11111111-1111-4111-8111-111111111111";

vi.mock("../../services/app/configMigrate", async () => {
  const actual = await vi.importActual<typeof import("../../services/app/configMigrate")>(
    "../../services/app/configMigrate"
  );
  return {
    ...actual,
    configExport: vi.fn(),
    configImport: vi.fn(),
  };
});

describe("query/configMigrate", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("useConfigExportMutation delegates file path to configExport", async () => {
    vi.mocked(configExport).mockResolvedValue(true);

    const client = createTestQueryClient();
    const wrapper = createQueryWrapper(client);
    const { result } = renderHook(() => useConfigExportMutation(), { wrapper });

    await act(async () => {
      await result.current.mutateAsync({ filePath: " /tmp/export.json " });
    });

    expect(configExport).toHaveBeenCalledWith("/tmp/export.json");
  });

  it("useConfigImportMutation invalidates imported config queries after success", async () => {
    vi.mocked(configImport).mockResolvedValue({
      providers_imported: 1,
      sort_modes_imported: 1,
      workspaces_imported: 1,
      prompts_imported: 1,
      mcp_servers_imported: 1,
      skill_repos_imported: 1,
      installed_skills_imported: 1,
      local_skills_imported: 1,
    });

    const client = createTestQueryClient();
    client.setQueryData(providerModelsKeys.catalog(7, PROVIDER_UUID), {
      providerId: 7,
      marker: "before-import",
    });
    client.setQueryData(codexManagedProfilesKeys.list(), [{ providerId: 7 }]);
    const cancelSpy = vi.spyOn(client, "cancelQueries");
    const invalidateSpy = vi.spyOn(client, "invalidateQueries");
    const wrapper = createQueryWrapper(client);
    const { result } = renderHook(() => useConfigImportMutation(), { wrapper });

    await act(async () => {
      await result.current.mutateAsync({ filePath: " /tmp/import.json " });
    });

    expect(configImport).toHaveBeenCalledWith("/tmp/import.json");
    expect(cancelSpy).toHaveBeenCalledWith({ queryKey: providerModelsKeys.all });
    expect(cancelSpy).toHaveBeenCalledWith({ queryKey: codexManagedProfilesKeys.all });
    expect(invalidateSpy).toHaveBeenCalledTimes(12);
    expect(invalidateSpy).toHaveBeenCalledWith({ queryKey: settingsKeys.all });
    expect(invalidateSpy).toHaveBeenCalledWith({ queryKey: gatewayKeys.all });
    expect(invalidateSpy).toHaveBeenCalledWith({ queryKey: providersKeys.all });
    expect(invalidateSpy).toHaveBeenCalledWith({ queryKey: providerModelsKeys.all });
    expect(invalidateSpy).toHaveBeenCalledWith({ queryKey: codexManagedProfilesKeys.all });
    expect(invalidateSpy).toHaveBeenCalledWith({ queryKey: sortModesKeys.all });
    expect(invalidateSpy).toHaveBeenCalledWith({ queryKey: workspacesKeys.all });
    expect(invalidateSpy).toHaveBeenCalledWith({ queryKey: promptsKeys.all });
    expect(invalidateSpy).toHaveBeenCalledWith({ queryKey: mcpKeys.all });
    expect(invalidateSpy).toHaveBeenCalledWith({ queryKey: skillsKeys.all });
    expect(invalidateSpy).toHaveBeenCalledWith({ queryKey: wslKeys.all });
    expect(invalidateSpy).toHaveBeenCalledWith({ queryKey: cliProxyKeys.all });
    expect(client.getQueryState(providerModelsKeys.catalog(7, PROVIDER_UUID))?.isInvalidated).toBe(
      true
    );
    expect(client.getQueryState(codexManagedProfilesKeys.list())?.isInvalidated).toBe(true);
  });

  it("prevents a pre-import model catalog read from committing after import", async () => {
    let resolveCatalog!: (value: unknown) => void;
    const pendingCatalog = new Promise<unknown>((resolve) => {
      resolveCatalog = resolve;
    });
    vi.mocked(configImport).mockResolvedValue({
      providers_imported: 1,
      sort_modes_imported: 0,
      workspaces_imported: 0,
      prompts_imported: 0,
      mcp_servers_imported: 0,
      skill_repos_imported: 0,
      installed_skills_imported: 0,
      local_skills_imported: 0,
    });
    const client = createTestQueryClient();
    const lateRead = client
      .fetchQuery({
        queryKey: providerModelsKeys.catalog(7, PROVIDER_UUID),
        queryFn: () => pendingCatalog,
      })
      .catch(() => undefined);
    const wrapper = createQueryWrapper(client);
    const { result } = renderHook(() => useConfigImportMutation(), { wrapper });

    await act(async () => {
      await result.current.mutateAsync({ filePath: "/tmp/import.json" });
    });
    resolveCatalog({ providerId: 7, marker: "pre-import" });
    await lateRead;

    expect(client.getQueryData(providerModelsKeys.catalog(7, PROVIDER_UUID))).toBeUndefined();
    expect(client.getQueryState(providerModelsKeys.catalog(7, PROVIDER_UUID))?.isInvalidated).toBe(
      true
    );
  });

  it("useConfigImportMutation skips invalidation when import returns null", async () => {
    vi.mocked(configImport).mockResolvedValue(null as never);

    const client = createTestQueryClient();
    const invalidateSpy = vi.spyOn(client, "invalidateQueries");
    const wrapper = createQueryWrapper(client);
    const { result } = renderHook(() => useConfigImportMutation(), { wrapper });

    await act(async () => {
      await result.current.mutateAsync({ filePath: "/tmp/import.json" });
    });

    expect(configImport).toHaveBeenCalledWith("/tmp/import.json");
    expect(invalidateSpy).not.toHaveBeenCalled();
  });

  it("rejects blank file paths before service calls", async () => {
    const client = createTestQueryClient();
    const wrapper = createQueryWrapper(client);

    const { result: exportResult } = renderHook(() => useConfigExportMutation(), { wrapper });
    await expect(exportResult.current.mutateAsync({ filePath: "   " })).rejects.toThrow(
      "SEC_INVALID_INPUT"
    );
    expect(configExport).not.toHaveBeenCalled();

    const { result: importResult } = renderHook(() => useConfigImportMutation(), { wrapper });
    await expect(importResult.current.mutateAsync({ filePath: "\n" })).rejects.toThrow(
      "SEC_INVALID_INPUT"
    );
    expect(configImport).not.toHaveBeenCalled();
  });
});
