import { act, renderHook, waitFor } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { toast } from "sonner";
import { tauriInvoke } from "../../test/mocks/tauri";
import { clearTauriRuntime, setTauriRuntime } from "../../test/utils/tauriRuntime";
import { createDeferred } from "../../test/utils/deferred";
import { settingsKeys, updaterKeys } from "../../query/keys";

vi.mock("sonner", () => ({
  toast: Object.assign(vi.fn(), { success: vi.fn(), error: vi.fn() }),
}));
vi.mock("../../services/consoleLog", () => ({ logToConsole: vi.fn() }));

describe("hooks/useUpdateMeta", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    localStorage.removeItem("devPreview.enabled");
    localStorage.removeItem("updater.betaParticipationConfirmed");
  });

  it("uses the shared dev preview toggle to return a mock update and block install", async () => {
    vi.resetModules();
    clearTauriRuntime();
    localStorage.setItem("devPreview.enabled", "1");

    const { queryClient } = await import("../../query/queryClient");
    queryClient.clear();

    const mod = await import("../useUpdateMeta");
    const { updateCheckNow, updateDownloadAndInstall, updateDialogSetOpen, useUpdateMeta } = mod;

    const update = await updateCheckNow({ silent: true, openDialogIfUpdate: true });
    expect(update?.rid).toBe(9_999_001);
    expect(update?.body).toContain("Dev 预览更新日志");

    const wrapper = ({ children }: any) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );

    const { result } = renderHook(() => useUpdateMeta(), { wrapper });
    await act(async () => {
      await Promise.resolve();
    });

    expect(result.current.dialogOpen).toBe(true);
    expect(result.current.updateCandidate?.rid).toBe(9_999_001);
    expect(result.current.updateCandidate?.body).toContain("不会参与真实安装");

    queryClient.setQueryData(updaterKeys.check(), update);
    updateDialogSetOpen(true);
    await expect(updateDownloadAndInstall()).resolves.toBe(false);
    expect(toast).toHaveBeenCalledWith("Dev 预览更新仅用于展示，不能安装");

    localStorage.removeItem("devPreview.enabled");
  });

  it("covers update check, dialog state, and download/install flows", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-02-01T00:00:00Z"));

    vi.resetModules();
    clearTauriRuntime();

    const { queryClient } = await import("../../query/queryClient");
    queryClient.clear();

    const mod = await import("../useUpdateMeta");
    const { updateCheckNow, updateDownloadAndInstall, updateDialogSetOpen, useUpdateMeta } = mod;

    // no runtime -> null and no toast
    expect(await updateCheckNow({ silent: false, openDialogIfUpdate: true })).toBeNull();

    setTauriRuntime();

    const checkResults: any[] = [
      false,
      {
        rid: 1,
        channel: "stable",
        isPrerelease: false,
        version: "1.0.1",
        currentVersion: "1.0.0",
        releaseUrl:
          "https://github.com/FingerCaster/aio-coding-hub/releases/tag/aio-coding-hub-v1.0.1",
        date: null,
        body: null,
      },
    ];

    const installDeferred = createDeferred<unknown>();

    vi.mocked(tauriInvoke).mockImplementation(async (cmd: string, args?: any) => {
      if (cmd === "desktop_updater_check") {
        return checkResults.shift() ?? false;
      }
      if (cmd === "desktop_updater_download_and_install") {
        args?.onEvent?.__emit?.({ event: "started", data: { contentLength: 100 } });
        args?.onEvent?.__emit?.({ event: "progress", data: { chunkLength: 10 } });
        args?.onEvent?.__emit?.({ event: "progress", data: { chunkLength: 5 } });
        args?.onEvent?.__emit?.({ event: "finished", data: { ok: true } });
        return installDeferred.promise as any;
      }
      return null as any;
    });

    // latest -> toast
    expect(await updateCheckNow({ silent: false, openDialogIfUpdate: false })).toBeNull();
    expect(toast).toHaveBeenCalledWith("已是最新版本");

    // update available -> open dialog
    const update = await updateCheckNow({ silent: true, openDialogIfUpdate: true });
    expect(update?.rid).toBe(1);

    const wrapper = ({ children }: any) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );

    const { result } = renderHook(() => useUpdateMeta(), { wrapper });
    await act(async () => {
      await Promise.resolve();
    });
    expect(result.current.dialogOpen).toBe(true);

    // prepare candidate in cache (updateDownloadAndInstall reads queryClient)
    queryClient.setQueryData(updaterKeys.check(), update);

    // cannot close while installing
    const installingPromise = updateDownloadAndInstall();
    await act(async () => {
      await Promise.resolve();
    });
    updateDialogSetOpen(false);
    expect(result.current.dialogOpen).toBe(true);

    installDeferred.resolve(true);
    expect(await installingPromise).toBe(true);

    await act(async () => {
      await Promise.resolve();
    });

    expect(result.current.installTotalBytes).toBe(100);
    expect(result.current.installDownloadedBytes).toBe(15);

    // close resets install progress
    updateDialogSetOpen(false);
    await act(async () => {
      await Promise.resolve();
    });
    expect(result.current.dialogOpen).toBe(false);
    expect(result.current.installDownloadedBytes).toBe(0);
    expect(result.current.installTotalBytes).toBeNull();

    vi.clearAllTimers();
    vi.useRealTimers();
  });

  it("sets installError when download/install throws", async () => {
    vi.useFakeTimers();
    vi.resetModules();
    setTauriRuntime();

    const { queryClient } = await import("../../query/queryClient");
    queryClient.clear();

    const mod = await import("../useUpdateMeta");
    const { updateDownloadAndInstall, updateDialogSetOpen, useUpdateMeta } = mod;

    await mod.syncCanonicalUpdateChannel("stable");
    const stableContext = mod.getUpdateChannelSnapshot();
    queryClient.setQueryData(updaterKeys.check("stable"), {
      rid: 2,
      channel: "stable",
      generation: stableContext.generation,
    } as any);

    vi.mocked(tauriInvoke).mockImplementation(async (cmd: string) => {
      if (cmd === "desktop_updater_download_and_install") throw new Error("boom");
      return null as any;
    });

    const wrapper = ({ children }: any) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );
    const { result } = renderHook(() => useUpdateMeta(), { wrapper });

    updateDialogSetOpen(true);
    await act(async () => {
      await Promise.resolve();
    });

    expect(await updateDownloadAndInstall()).toBe(false);
    await act(async () => {
      await Promise.resolve();
    });

    expect(result.current.installError).toBe("Error: boom");
    expect(toast).toHaveBeenCalledWith("安装更新失败：请稍后重试");

    vi.clearAllTimers();
    vi.useRealTimers();
  });

  it("refreshes the one-shot updater resource before retrying a failed install", async () => {
    vi.resetModules();
    setTauriRuntime();

    const { queryClient } = await import("../../query/queryClient");
    queryClient.clear();

    const mod = await import("../useUpdateMeta");
    await mod.syncCanonicalUpdateChannel("stable");
    const { generation } = mod.getUpdateChannelSnapshot();
    queryClient.setQueryData(updaterKeys.check("stable"), {
      rid: 21,
      channel: "stable",
      isPrerelease: false,
      version: "1.0.1",
      currentVersion: "1.0.0",
      releaseUrl:
        "https://github.com/FingerCaster/aio-coding-hub/releases/tag/aio-coding-hub-v1.0.1",
      date: null,
      body: null,
      generation,
    });

    const installedRids: number[] = [];
    vi.mocked(tauriInvoke).mockImplementation(async (cmd: string, args?: any) => {
      if (cmd === "desktop_updater_download_and_install") {
        installedRids.push(args.rid);
        if (args.rid === 21) throw new Error("temporary download failure");
        return true as any;
      }
      if (cmd === "desktop_updater_check") {
        return {
          rid: 22,
          channel: "stable",
          isPrerelease: false,
          version: "1.0.1",
          currentVersion: "1.0.0",
          releaseUrl:
            "https://github.com/FingerCaster/aio-coding-hub/releases/tag/aio-coding-hub-v1.0.1",
          date: null,
          body: null,
        } as any;
      }
      return null as any;
    });

    await expect(mod.updateDownloadAndInstall()).resolves.toBe(false);
    expect(queryClient.getQueryData(updaterKeys.check("stable"))).toMatchObject({
      rid: 22,
      generation,
    });

    await expect(mod.updateDownloadAndInstall()).resolves.toBe(true);
    expect(installedRids).toEqual([21, 22]);
  });

  it("keeps UI state subscribers isolated when one listener throws", async () => {
    vi.resetModules();
    clearTauriRuntime();

    const mod = await import("../useUpdateMeta");
    const { logToConsole } = await import("../../services/consoleLog");
    const failingListener = vi.fn(() => {
      throw new Error("listener boom");
    });
    const healthyListener = vi.fn();

    const unsubscribeFailing = mod.subscribeUpdateMeta(failingListener);
    const unsubscribeHealthy = mod.subscribeUpdateMeta(healthyListener);

    expect(() => mod.updateDialogSetOpen(true)).not.toThrow();
    expect(failingListener).toHaveBeenCalledTimes(1);
    expect(healthyListener).toHaveBeenCalledTimes(1);
    expect(logToConsole).toHaveBeenCalledWith("warn", "更新状态订阅处理失败", {
      error: "Error: listener boom",
    });

    unsubscribeFailing();
    unsubscribeHealthy();
    mod.updateDialogSetOpen(false);
  });

  it("logs and toasts when update check fails even if localStorage write also throws", async () => {
    vi.resetModules();
    setTauriRuntime();

    const setItemSpy = vi.spyOn(Storage.prototype, "setItem");
    setItemSpy.mockImplementation(() => {
      throw new Error("quota");
    });

    const { queryClient } = await import("../../query/queryClient");
    queryClient.clear();

    const { updateCheckNow } = await import("../useUpdateMeta");

    vi.mocked(tauriInvoke).mockImplementation(async (cmd: string) => {
      if (cmd === "desktop_updater_check") throw new Error("check boom");
      return null as any;
    });

    expect(await updateCheckNow({ silent: false, openDialogIfUpdate: false })).toBeNull();
    expect(toast).toHaveBeenCalledWith("检查更新失败：Error: check boom");

    setItemSpy.mockRestore();
  });

  it("returns null without candidate and reuses the in-flight install promise", async () => {
    vi.resetModules();
    setTauriRuntime();

    const { queryClient } = await import("../../query/queryClient");
    queryClient.clear();

    const mod = await import("../useUpdateMeta");
    const { updateDownloadAndInstall, useUpdateMeta } = mod;

    const wrapper = ({ children }: any) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );
    const { result } = renderHook(() => useUpdateMeta(), { wrapper });

    expect(await updateDownloadAndInstall()).toBeNull();

    const installDeferred = createDeferred<boolean>();
    let downloadCalls = 0;
    await mod.syncCanonicalUpdateChannel("stable");
    const stableContext = mod.getUpdateChannelSnapshot();
    queryClient.setQueryData(updaterKeys.check("stable"), {
      rid: 3,
      channel: "stable",
      generation: stableContext.generation,
    } as any);

    vi.mocked(tauriInvoke).mockImplementation(async (cmd: string, args?: any) => {
      if (cmd === "desktop_updater_download_and_install") {
        downloadCalls += 1;
        args?.onEvent?.__emit?.({ event: "started", data: {} });
        args?.onEvent?.__emit?.({ event: "progress", data: { chunkLength: 0 } });
        return installDeferred.promise as any;
      }
      return null as any;
    });

    const firstPromise = updateDownloadAndInstall();
    await act(async () => {
      await Promise.resolve();
    });
    expect(result.current.installTotalBytes).toBeNull();

    const secondPromise = updateDownloadAndInstall();
    expect(downloadCalls).toBe(1);

    installDeferred.resolve(true);
    await expect(firstPromise).resolves.toBe(true);
    await expect(secondPromise).resolves.toBe(true);
  });

  it("rejects cached candidates without an exact channel and generation", async () => {
    vi.resetModules();
    setTauriRuntime();

    const { queryClient } = await import("../../query/queryClient");
    queryClient.clear();
    const mod = await import("../useUpdateMeta");
    await mod.syncCanonicalUpdateChannel("stable");
    const { generation } = mod.getUpdateChannelSnapshot();

    queryClient.setQueryData(updaterKeys.check("stable"), {
      rid: 10,
      channel: "stable",
    } as any);
    await expect(mod.updateDownloadAndInstall()).resolves.toBeNull();

    queryClient.setQueryData(updaterKeys.check("stable"), {
      rid: 11,
      channel: "stable",
      generation: generation - 1,
    } as any);
    await expect(mod.updateDownloadAndInstall()).resolves.toBeNull();

    queryClient.setQueryData(updaterKeys.check("stable"), {
      rid: 12,
      channel: "beta",
      generation,
    } as any);
    await expect(mod.updateDownloadAndInstall()).resolves.toBeNull();
    expect(tauriInvoke).not.toHaveBeenCalledWith(
      "desktop_updater_download_and_install",
      expect.anything()
    );
  });

  it("discards a late stable resource after switching to Beta", async () => {
    vi.resetModules();
    setTauriRuntime();

    const checkDeferred = createDeferred<unknown>();
    vi.mocked(tauriInvoke).mockImplementation(async (cmd: string) => {
      if (cmd === "desktop_updater_check") return checkDeferred.promise as any;
      return null as any;
    });

    const { commands } = await import("../../generated/bindings");
    const discard = vi.fn(async () => ({ status: "ok", data: true }));
    (commands as any).desktopUpdaterDiscard = discard;

    const { queryClient } = await import("../../query/queryClient");
    queryClient.clear();
    const mod = await import("../useUpdateMeta");
    await mod.syncCanonicalUpdateChannel("stable");

    const pendingCheck = mod.updateCheckNow({ silent: true, openDialogIfUpdate: true });
    await Promise.resolve();
    const stableGeneration = mod.getUpdateChannelSnapshot().generation;
    await mod.syncCanonicalUpdateChannel("beta");

    checkDeferred.resolve({
      rid: 20,
      channel: "stable",
      isPrerelease: false,
      version: "1.0.0",
      currentVersion: "0.9.0",
      releaseUrl:
        "https://github.com/FingerCaster/aio-coding-hub/releases/tag/aio-coding-hub-v1.0.0",
      date: null,
      body: null,
    });

    await expect(pendingCheck).resolves.toBeNull();
    await waitFor(() => expect(discard).toHaveBeenCalledWith(20));
    expect(mod.getUpdateChannelSnapshot()).toMatchObject({
      channel: "beta",
      generation: stableGeneration + 1,
    });
    expect(queryClient.getQueryData(updaterKeys.check("stable"))).toBeUndefined();
    expect(queryClient.getQueryData(updaterKeys.check("beta"))).toBeUndefined();
    delete (commands as any).desktopUpdaterDiscard;
  });

  it("removes and discards the old candidate when a fresh check fails", async () => {
    vi.resetModules();
    setTauriRuntime();

    vi.mocked(tauriInvoke).mockImplementation(async (cmd: string) => {
      if (cmd === "desktop_updater_check") throw new Error("fresh check failed");
      return null as any;
    });
    const { commands } = await import("../../generated/bindings");
    const discard = vi.fn(async () => ({ status: "ok", data: true }));
    (commands as any).desktopUpdaterDiscard = discard;
    const { queryClient } = await import("../../query/queryClient");
    queryClient.clear();
    const mod = await import("../useUpdateMeta");
    await mod.syncCanonicalUpdateChannel("stable");
    const generation = mod.getUpdateChannelSnapshot().generation;
    queryClient.setQueryData(updaterKeys.check("stable"), {
      rid: 25,
      channel: "stable",
      version: "1.0.0",
      currentVersion: "0.9.0",
      releaseUrl: "https://github.com/FingerCaster/aio-coding-hub/releases/tag/v1.0.0",
      date: null,
      body: null,
      generation,
    });

    const wrapper = ({ children }: any) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );
    const { result } = renderHook(() => mod.useUpdateMeta(), { wrapper });
    mod.updateDialogSetOpen(true);
    await act(async () => Promise.resolve());
    expect(result.current.updateCandidate?.rid).toBe(25);
    expect(result.current.dialogOpen).toBe(true);

    let checked: unknown;
    await act(async () => {
      checked = await mod.updateCheckNow({ silent: true, openDialogIfUpdate: false });
    });
    expect(checked).toBeNull();

    await waitFor(() => expect(discard).toHaveBeenCalledWith(25));
    expect(queryClient.getQueryData(updaterKeys.check("stable"))).toBeUndefined();
    expect(result.current.updateCandidate).toBeNull();
    expect(result.current.dialogOpen).toBe(false);
    delete (commands as any).desktopUpdaterDiscard;
  });

  it("does not synthesize a stable dev-preview candidate while Beta is active", async () => {
    vi.resetModules();
    clearTauriRuntime();
    localStorage.setItem("devPreview.enabled", "1");

    const { queryClient } = await import("../../query/queryClient");
    queryClient.clear();
    const mod = await import("../useUpdateMeta");
    await mod.syncCanonicalUpdateChannel("beta");

    await expect(
      mod.updateCheckNow({ silent: true, openDialogIfUpdate: true })
    ).resolves.toBeNull();

    const wrapper = ({ children }: any) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );
    const { result } = renderHook(() => mod.useUpdateMeta(), { wrapper });
    await act(async () => Promise.resolve());

    expect(result.current.updateChannel).toBe("beta");
    expect(result.current.updateCandidate).toBeNull();
    expect(result.current.dialogOpen).toBe(false);
    expect(tauriInvoke).not.toHaveBeenCalledWith("desktop_updater_check", expect.anything());
  });

  it("normalizes a successful import to stable and clears the Beta resource", async () => {
    vi.resetModules();
    setTauriRuntime();

    let settingsReads = 0;
    vi.mocked(tauriInvoke).mockImplementation(async (cmd: string) => {
      if (cmd === "settings_get") {
        settingsReads += 1;
        return { update_channel: settingsReads === 1 ? "beta" : "stable" } as any;
      }
      return null as any;
    });

    const { commands } = await import("../../generated/bindings");
    const discard = vi.fn(async () => ({ status: "ok", data: true }));
    (commands as any).desktopUpdaterDiscard = discard;
    const { queryClient } = await import("../../query/queryClient");
    queryClient.clear();
    const mod = await import("../useUpdateMeta");

    const wrapper = ({ children }: any) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );
    const { result } = renderHook(() => mod.useUpdateMeta(), { wrapper });
    await waitFor(() => {
      expect(result.current.updateChannel).toBe("beta");
      expect(result.current.checkingUpdate).toBe(false);
    });
    const betaGeneration = mod.getUpdateChannelSnapshot().generation;
    queryClient.setQueryData(updaterKeys.check("beta"), {
      rid: 30,
      channel: "beta",
      generation: betaGeneration,
    } as any);
    mod.updateDialogSetOpen(true);

    await act(async () => {
      const { notifyUpdateChannelImportSucceeded } =
        await import("../../services/app/updateChannel");
      await notifyUpdateChannelImportSucceeded();
    });

    await waitFor(() => expect(settingsReads).toBeGreaterThanOrEqual(2));
    await waitFor(() => expect(discard).toHaveBeenCalledWith(30));
    expect(result.current.updateChannel).toBe("stable");
    expect(result.current.updateGeneration).toBe(betaGeneration + 1);
    expect(result.current.updateCandidate).toBeNull();
    expect(result.current.dialogOpen).toBe(false);
    delete (commands as any).desktopUpdaterDiscard;
  });

  it("fails closed to stable when the canonical settings reread after import fails", async () => {
    vi.resetModules();
    setTauriRuntime();

    vi.mocked(tauriInvoke).mockImplementation(async (cmd: string) => {
      if (cmd === "settings_get") throw new Error("import settings reread failed");
      if (cmd === "desktop_updater_discard") return true as any;
      return null as any;
    });

    const { queryClient } = await import("../../query/queryClient");
    queryClient.clear();
    queryClient.setQueryData(settingsKeys.get(), { update_channel: "beta" });

    const mod = await import("../useUpdateMeta");
    await mod.syncCanonicalUpdateChannel("beta");
    const betaGeneration = mod.getUpdateChannelSnapshot().generation;
    queryClient.setQueryData(updaterKeys.check("beta"), {
      rid: 31,
      channel: "beta",
      generation: betaGeneration,
    } as any);

    const wrapper = ({ children }: any) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );
    const { result } = renderHook(() => mod.useUpdateMeta(), { wrapper });
    await act(async () => Promise.resolve());
    mod.updateDialogSetOpen(true);

    await act(async () => {
      const { notifyUpdateChannelImportSucceeded } =
        await import("../../services/app/updateChannel");
      await notifyUpdateChannelImportSucceeded();
    });

    expect(result.current.updateChannel).toBe("stable");
    expect(result.current.updateCandidate).toBeNull();
    expect(result.current.dialogOpen).toBe(false);
    expect(result.current.updateChannelError).toContain("import settings reread failed");
    expect(queryClient.getQueryData(updaterKeys.check("beta"))).toBeUndefined();
    expect(tauriInvoke).not.toHaveBeenCalledWith("desktop_updater_check", expect.anything());
  });

  it("recovers canonical Beta after a transient settings read failure", async () => {
    vi.resetModules();
    setTauriRuntime();

    let settingsReadFails = false;
    vi.mocked(tauriInvoke).mockImplementation(async (cmd: string) => {
      if (cmd === "settings_get") {
        if (settingsReadFails) throw new Error("temporary settings failure");
        return { update_channel: "beta" } as any;
      }
      return null as any;
    });

    const { queryClient } = await import("../../query/queryClient");
    const { settingsGet } = await import("../../services/settings/settings");
    queryClient.clear();
    const mod = await import("../useUpdateMeta");

    const wrapper = ({ children }: any) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );
    const { result } = renderHook(() => mod.useUpdateMeta(), { wrapper });
    await waitFor(() => {
      expect(result.current.updateChannel).toBe("beta");
      expect(result.current.updateChannelReady).toBe(true);
    });

    settingsReadFails = true;
    await act(async () => {
      await expect(
        queryClient.fetchQuery({
          queryKey: settingsKeys.get(),
          queryFn: settingsGet,
          staleTime: 0,
          retry: false,
        })
      ).rejects.toThrow("temporary settings failure");
    });
    await waitFor(() => {
      expect(result.current.updateChannel).toBe("stable");
      expect(result.current.updateChannelReady).toBe(false);
    });

    settingsReadFails = false;
    await act(async () => {
      await queryClient.fetchQuery({
        queryKey: settingsKeys.get(),
        queryFn: settingsGet,
        staleTime: 0,
        retry: false,
      });
    });
    await waitFor(() => {
      expect(result.current.updateChannel).toBe("beta");
      expect(result.current.updateChannelReady).toBe(true);
    });
  });

  it("rolls back to the canonical stable channel when the dedicated Beta save fails", async () => {
    vi.resetModules();
    setTauriRuntime();

    const { commands } = await import("../../generated/bindings");
    const writer = vi.fn(async () => {
      throw new Error("save failed");
    });
    (commands as any).settingsUpdateChannelSet = writer;
    const { queryClient } = await import("../../query/queryClient");
    queryClient.clear();
    const mod = await import("../useUpdateMeta");
    await mod.syncCanonicalUpdateChannel("stable");
    const stableGeneration = mod.getUpdateChannelSnapshot().generation;

    await expect(mod.setUpdateChannel("beta")).resolves.toBe(false);
    expect(mod.getUpdateChannelSnapshot()).toMatchObject({
      channel: "stable",
      generation: stableGeneration,
      saving: false,
    });
    expect(tauriInvoke).not.toHaveBeenCalledWith("desktop_updater_check", expect.anything());
    delete (commands as any).settingsUpdateChannelSet;
  });

  it("clears the Beta resource and immediately checks stable after opting out", async () => {
    vi.resetModules();
    setTauriRuntime();

    const { commands } = await import("../../generated/bindings");
    const writer = vi.fn(async (channel: "stable" | "beta") => ({
      status: "ok",
      data: { update_channel: channel },
    }));
    const discard = vi.fn(async () => ({ status: "ok", data: true }));
    (commands as any).settingsUpdateChannelSet = writer;
    (commands as any).desktopUpdaterDiscard = discard;
    vi.mocked(tauriInvoke).mockImplementation(async (cmd: string) => {
      if (cmd === "desktop_updater_check") {
        return {
          rid: 41,
          channel: "stable",
          isPrerelease: false,
          version: "1.0.0",
          currentVersion: "1.0.0-beta.2",
          releaseUrl:
            "https://github.com/FingerCaster/aio-coding-hub/releases/tag/aio-coding-hub-v1.0.0",
          date: null,
          body: null,
        } as any;
      }
      return null as any;
    });

    const { queryClient } = await import("../../query/queryClient");
    queryClient.clear();
    const mod = await import("../useUpdateMeta");
    await mod.syncCanonicalUpdateChannel("beta");
    const betaGeneration = mod.getUpdateChannelSnapshot().generation;
    queryClient.setQueryData(updaterKeys.check("beta"), {
      rid: 40,
      channel: "beta",
      version: "1.0.0-beta.2",
      currentVersion: "0.9.0",
      releaseUrl:
        "https://github.com/FingerCaster/aio-coding-hub/releases/tag/aio-coding-hub-v1.0.0-beta.2",
      date: null,
      body: null,
      generation: betaGeneration,
    });

    await expect(mod.setUpdateChannel("stable")).resolves.toBe(true);
    await waitFor(() => expect(discard).toHaveBeenCalledWith(40));
    expect(writer).toHaveBeenCalledWith("stable", null);
    expect(mod.getUpdateChannelSnapshot()).toMatchObject({
      channel: "stable",
      generation: betaGeneration + 1,
    });
    expect(queryClient.getQueryData(updaterKeys.check("beta"))).toBeUndefined();
    expect(queryClient.getQueryData(updaterKeys.check("stable"))).toMatchObject({
      rid: 41,
      channel: "stable",
      generation: betaGeneration + 1,
    });
    delete (commands as any).settingsUpdateChannelSet;
    delete (commands as any).desktopUpdaterDiscard;
  });
});
