import { afterEach, describe, expect, it, vi } from "vitest";
import { commands } from "../../../generated/bindings";
import { createTestAppSettings } from "../../../test/fixtures/settings";

afterEach(() => {
  vi.restoreAllMocks();
});

describe("services/app/updateChannel", () => {
  it("normalizes unknown settings values to stable and labels Beta explicitly", async () => {
    const {
      BETA_UPDATE_CHANNEL,
      normalizeUpdateChannel,
      readUpdateChannelFromSettings,
      updateChannelLabel,
    } = await import("../updateChannel");

    expect(normalizeUpdateChannel("unknown")).toBe("stable");
    expect(readUpdateChannelFromSettings({ update_channel: "beta" })).toBe(BETA_UPDATE_CHANNEL);
    expect(readUpdateChannelFromSettings({ updateChannel: "beta" })).toBe("stable");
    expect(readUpdateChannelFromSettings({ channel: "beta" })).toBe("stable");
    expect(updateChannelLabel(BETA_UPDATE_CHANNEL)).toBe("Beta 更新");
  });

  it("uses the dedicated writer and confirms the exact Beta resource", async () => {
    const writer = vi
      .spyOn(commands, "settingsUpdateChannelSet")
      .mockImplementation(async (channel) => ({
        status: "ok",
        data: createTestAppSettings({ update_channel: channel }),
      }));

    const { settingsUpdateChannelSet } = await import("../updateChannel");
    await expect(settingsUpdateChannelSet("beta")).resolves.toBe("beta");

    expect(writer).toHaveBeenCalledTimes(1);
    const [channel, confirm] = writer.mock.calls[0];
    expect(channel).toBe("beta");
    expect(confirm).toEqual({
      confirm: expect.objectContaining({
        action: "settings_update_channel_set",
        resource: "update_channel:beta",
      }),
    });
  });
});
