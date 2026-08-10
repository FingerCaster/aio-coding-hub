import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AIO_REPO_URL } from "../../../constants/urls";
import { tauriInvoke } from "../../../test/mocks/tauri";
import { setTauriRuntime } from "../../../test/utils/tauriRuntime";

const AIO_REPO_PATH = new URL(AIO_REPO_URL).pathname.split("/").filter(Boolean);
const AIO_RELEASE_TAG_URL = `${AIO_REPO_URL}/releases/tag/aio-coding-hub-v0.60.0`;

function createUpdaterMetadata(
  overrides: Partial<{
    rid: number;
    channel: "stable" | "beta";
    isPrerelease: boolean;
    version: string;
    currentVersion: string;
    date: string | null;
    body: string | null;
    releaseUrl: string;
  }> = {}
) {
  const version = overrides.version ?? "0.60.0";
  return {
    rid: 1,
    channel: "stable" as const,
    isPrerelease: overrides.isPrerelease ?? version.includes("-beta."),
    version,
    currentVersion: "0.59.0",
    date: null,
    body: null,
    releaseUrl: `${AIO_REPO_URL}/releases/tag/aio-coding-hub-v${version}`,
    ...overrides,
  };
}

describe("services/app/updater", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("parseUpdaterCheckResult requires exact channel-bound metadata", async () => {
    const { parseUpdaterCheckResult } = await import("../updater");

    expect(parseUpdaterCheckResult(null)).toBeNull();
    expect(parseUpdaterCheckResult(false)).toBeNull();
    expect(parseUpdaterCheckResult("x")).toBeNull();
    expect(parseUpdaterCheckResult({})).toBeNull();
    expect(parseUpdaterCheckResult({ rid: "1" })).toBeNull();
    expect(parseUpdaterCheckResult({ rid: -1 })).toBeNull();
    expect(parseUpdaterCheckResult({ rid: 1.5 })).toBeNull();
    expect(parseUpdaterCheckResult({ rid: Number.NaN })).toBeNull();

    const valid = createUpdaterMetadata({
      rid: 1,
      date: "2026-02-01",
      body: "notes",
    });
    expect(parseUpdaterCheckResult(valid)).toEqual(valid);

    for (const key of [
      "channel",
      "isPrerelease",
      "currentVersion",
      "version",
      "releaseUrl",
      "date",
      "body",
    ] as const) {
      const malformed = { ...valid } as Record<string, unknown>;
      delete malformed[key];
      expect(parseUpdaterCheckResult(malformed)).toBeNull();
    }

    expect(parseUpdaterCheckResult({ ...valid, channel: "nightly" })).toBeNull();
    expect(parseUpdaterCheckResult({ ...valid, currentVersion: "" })).toBeNull();
    expect(
      parseUpdaterCheckResult(
        createUpdaterMetadata({ channel: "stable", version: "0.60.0-beta.1" })
      )
    ).toBeNull();
    expect(
      parseUpdaterCheckResult(
        createUpdaterMetadata({ channel: "beta", version: "0.60.0-beta.1" }),
        "beta"
      )
    ).toEqual(createUpdaterMetadata({ channel: "beta", version: "0.60.0-beta.1" }));
    expect(
      parseUpdaterCheckResult(createUpdaterMetadata({ channel: "beta", version: "0.60.0-beta.1" }))
    ).toBeNull();
    expect(
      parseUpdaterCheckResult(createUpdaterMetadata({ channel: "beta", version: "0.60.0-rc.1" }))
    ).toBeNull();
    expect(parseUpdaterCheckResult(createUpdaterMetadata({ version: "01.60.0" }))).toBeNull();
    expect(
      parseUpdaterCheckResult(createUpdaterMetadata({ releaseUrl: "https://example.com/release" }))
    ).toBeNull();
    expect(parseUpdaterCheckResult({ ...valid, date: 123 })).toBeNull();
    expect(parseUpdaterCheckResult({ ...valid, body: { text: "notes" } })).toBeNull();
  });

  it("accepts only the exact AIO GitHub release URL", async () => {
    const { isExactAioReleaseUrl } = await import("../updater");

    expect(isExactAioReleaseUrl(AIO_RELEASE_TAG_URL)).toBe(true);
    expect(isExactAioReleaseUrl(`${AIO_REPO_URL}/releases/tag/aio-coding-hub-v0.60.0/extra`)).toBe(
      false
    );
    expect(isExactAioReleaseUrl(`${AIO_REPO_URL}/releases/tag/aio-coding-hub-v0.60.0?x=1`)).toBe(
      false
    );
    expect(
      isExactAioReleaseUrl(
        "https://github.com.evil.example/FingerCaster/aio-coding-hub/releases/tag/v1"
      )
    ).toBe(false);
    expect(
      isExactAioReleaseUrl("https://user@github.com/FingerCaster/aio-coding-hub/releases/tag/v1")
    ).toBe(false);
  });

  it("updaterCheck parses tauri result", async () => {
    const { updaterCheck } = await import("../updater");

    setTauriRuntime();

    vi.mocked(tauriInvoke).mockResolvedValueOnce(false as any);
    expect(await updaterCheck()).toBeNull();
    expect(tauriInvoke).toHaveBeenLastCalledWith("desktop_updater_check", {
      expectedChannel: "stable",
      timeout: null,
    });

    vi.mocked(tauriInvoke).mockResolvedValueOnce(createUpdaterMetadata({ rid: 2 }) as any);
    expect(await updaterCheck()).toEqual({
      rid: 2,
      channel: "stable",
      isPrerelease: false,
      version: "0.60.0",
      currentVersion: "0.59.0",
      date: null,
      body: null,
      releaseUrl: AIO_RELEASE_TAG_URL,
    });

    vi.mocked(tauriInvoke).mockResolvedValueOnce(
      createUpdaterMetadata({ channel: "beta", version: "0.60.0-beta.1" }) as any
    );
    expect(await updaterCheck("beta")).toMatchObject({
      channel: "beta",
      version: "0.60.0-beta.1",
    });
    expect(tauriInvoke).toHaveBeenLastCalledWith("desktop_updater_check", {
      expectedChannel: "beta",
      timeout: null,
    });
  });

  it("calls the generated updater command with the explicit channel and timeout", async () => {
    const { commands } = await import("../../../generated/bindings");
    const original = commands.desktopUpdaterCheck;
    const generatedCheck = vi.fn(async () => ({
      status: "ok",
      data: {
        rid: 5,
        channel: "beta",
        isPrerelease: true,
        version: "0.61.0-beta.1",
        currentVersion: "0.60.0",
        releaseUrl:
          "https://github.com/FingerCaster/aio-coding-hub/releases/tag/aio-coding-hub-v0.61.0-beta.1",
        date: null,
        body: null,
      },
    }));
    (commands as any).desktopUpdaterCheck = generatedCheck;

    try {
      const { updaterCheck } = await import("../updater");
      await expect(updaterCheck("beta")).resolves.toMatchObject({
        rid: 5,
        channel: "beta",
        version: "0.61.0-beta.1",
      });
      expect(generatedCheck).toHaveBeenCalledWith("beta", null);
    } finally {
      commands.desktopUpdaterCheck = original;
    }
  });

  it("updaterCheck replaces GitHub release fallback notes with release body", async () => {
    const { updaterCheck } = await import("../updater");

    setTauriRuntime();

    vi.mocked(tauriInvoke).mockResolvedValueOnce(
      createUpdaterMetadata({
        rid: 3,
        date: "2026-06-14T15:58:48Z",
        body: `See release: ${AIO_RELEASE_TAG_URL}`,
      }) as any
    );

    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({
        body: "## 0.60.0\n\n- 具体更新内容",
      }),
    });
    vi.stubGlobal("fetch", fetchMock);

    await expect(updaterCheck()).resolves.toEqual({
      rid: 3,
      channel: "stable",
      isPrerelease: false,
      version: "0.60.0",
      currentVersion: "0.59.0",
      releaseUrl: AIO_RELEASE_TAG_URL,
      date: "2026-06-14T15:58:48Z",
      body: "## 0.60.0\n\n- 具体更新内容",
    });

    expect(fetchMock).toHaveBeenCalledWith(
      `https://api.github.com/repos/${AIO_REPO_PATH[0]}/${AIO_REPO_PATH[1]}/releases/tags/aio-coding-hub-v0.60.0`,
      expect.objectContaining({
        headers: expect.objectContaining({ accept: "application/vnd.github+json" }),
      })
    );
  });

  it("updaterCheck keeps fallback notes when GitHub release body cannot be loaded", async () => {
    const { updaterCheck } = await import("../updater");

    setTauriRuntime();

    const fallbackBody = `See release: ${AIO_RELEASE_TAG_URL}`;
    vi.mocked(tauriInvoke).mockResolvedValueOnce(
      createUpdaterMetadata({ rid: 4, body: fallbackBody }) as any
    );

    const fetchMock = vi.fn().mockResolvedValue({ ok: false });
    vi.stubGlobal("fetch", fetchMock);

    await expect(updaterCheck()).resolves.toEqual({
      rid: 4,
      channel: "stable",
      isPrerelease: false,
      version: "0.60.0",
      currentVersion: "0.59.0",
      date: null,
      body: fallbackBody,
      releaseUrl: AIO_RELEASE_TAG_URL,
    });
  });

  it("updaterDiscard uses the generated one-shot resource command", async () => {
    const { updaterDiscard } = await import("../updater");

    setTauriRuntime();
    vi.mocked(tauriInvoke).mockResolvedValueOnce(true as any);

    await expect(updaterDiscard(7)).resolves.toBe(true);
    expect(tauriInvoke).toHaveBeenCalledWith("desktop_updater_discard", { rid: 7 });
  });

  it("updaterDownloadAndInstall maps events and supports timeout option", async () => {
    const { updaterDownloadAndInstall } = await import("../updater");

    setTauriRuntime();

    const events: any[] = [];
    vi.mocked(tauriInvoke).mockImplementation(async (cmd: string, args?: any) => {
      if (cmd !== "desktop_updater_download_and_install") return null as any;

      const ch = args?.onEvent;
      ch?.__emit?.({ foo: 1 }); // ignored
      ch?.__emit?.({ event: "started", data: { contentLength: 123 } });
      ch?.__emit?.({ event: "progress", data: { chunkLength: 10 } });
      ch?.__emit?.({ event: "progress", data: { chunkLength: "bad" } }); // ignored chunkLength
      ch?.__emit?.({ event: "finished", data: { ok: true } });
      return true as any;
    });

    const ok = await updaterDownloadAndInstall({
      rid: 99,
      timeoutMs: 1234,
      onEvent: (e) => events.push(e),
    });

    expect(ok).toBe(true);
    expect(tauriInvoke).toHaveBeenCalledWith(
      "desktop_updater_download_and_install",
      expect.objectContaining({
        rid: 99,
        timeout: 1234,
        onEvent: expect.anything(),
        confirm: expect.objectContaining({
          confirm: expect.objectContaining({
            action: "desktop_updater_download_and_install",
            resource: "updater:99",
            nonce: expect.any(String),
          }),
        }),
      })
    );

    expect(events).toEqual([
      { event: "started", data: { contentLength: 123 } },
      { event: "progress", data: { chunkLength: 10 } },
      { event: "progress", data: { chunkLength: undefined } },
      { event: "finished", data: { ok: true } },
    ]);
  });

  it("updaterDownloadAndInstall rejects invalid rid and timeout before handwritten IPC", async () => {
    const { updaterDiscard, updaterDownloadAndInstall } = await import("../updater");
    const { desktopUpdaterCheck } = await import("../../desktop/updater");

    setTauriRuntime();

    await expect(updaterDownloadAndInstall({ rid: -1 })).rejects.toThrow("SEC_INVALID_INPUT");
    await expect(updaterDownloadAndInstall({ rid: 1.5 })).rejects.toThrow("SEC_INVALID_INPUT");
    await expect(updaterDownloadAndInstall({ rid: 1, timeoutMs: 0 })).rejects.toThrow(
      "SEC_INVALID_INPUT"
    );
    await expect(desktopUpdaterCheck({ channel: "stable", timeoutMs: Number.NaN })).rejects.toThrow(
      "SEC_INVALID_INPUT"
    );
    await expect(desktopUpdaterCheck({ channel: "nightly" as any })).rejects.toThrow(
      "SEC_INVALID_INPUT"
    );
    await expect(updaterDiscard(-1)).rejects.toThrow("SEC_INVALID_INPUT");

    expect(tauriInvoke).not.toHaveBeenCalled();
  });

  it("updaterDownloadAndInstall tolerates missing callback and default timeout branches", async () => {
    const { updaterDownloadAndInstall } = await import("../updater");

    setTauriRuntime();

    vi.mocked(tauriInvoke).mockImplementation(async (cmd: string, args?: any) => {
      if (cmd !== "desktop_updater_download_and_install") return null as any;

      const ch = args?.onEvent;
      ch?.__emit?.({ event: "started", data: "invalid" });
      ch?.__emit?.({ event: "progress", data: null });
      ch?.__emit?.({ event: "finished" });
      return true as any;
    });

    const ok = await updaterDownloadAndInstall({
      rid: 7,
    });

    expect(ok).toBe(true);
    expect(tauriInvoke).toHaveBeenCalledWith(
      "desktop_updater_download_and_install",
      expect.objectContaining({
        rid: 7,
        timeout: null,
        onEvent: expect.anything(),
      })
    );
  });
});
