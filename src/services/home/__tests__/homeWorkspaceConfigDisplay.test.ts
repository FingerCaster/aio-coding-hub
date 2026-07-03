import { afterEach, describe, expect, it, vi } from "vitest";
import {
  HOME_WORKSPACE_CONFIG_SHOW_ALL_STORAGE_KEY,
  readHomeWorkspaceConfigShowAllFromStorage,
  subscribeHomeWorkspaceConfigShowAll,
  writeHomeWorkspaceConfigShowAllToStorage,
} from "../homeWorkspaceConfigDisplay";

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
  window.localStorage.clear();
});

describe("services/home/homeWorkspaceConfigDisplay", () => {
  it("reads and writes the show-all flag with fallback values", () => {
    expect(readHomeWorkspaceConfigShowAllFromStorage()).toBe(false);

    writeHomeWorkspaceConfigShowAllToStorage(true);
    expect(window.localStorage.getItem(HOME_WORKSPACE_CONFIG_SHOW_ALL_STORAGE_KEY)).toBe("true");
    expect(readHomeWorkspaceConfigShowAllFromStorage()).toBe(true);

    writeHomeWorkspaceConfigShowAllToStorage(false);
    expect(readHomeWorkspaceConfigShowAllFromStorage()).toBe(false);

    vi.spyOn(window.localStorage.__proto__, "getItem").mockImplementation(() => {
      throw new Error("storage denied");
    });
    expect(readHomeWorkspaceConfigShowAllFromStorage()).toBe(false);
  });

  it("notifies subscribers on writes and matching storage events", () => {
    const first = vi.fn();
    const second = vi.fn();
    const unsubscribeFirst = subscribeHomeWorkspaceConfigShowAll(first);
    const unsubscribeSecond = subscribeHomeWorkspaceConfigShowAll(second);

    writeHomeWorkspaceConfigShowAllToStorage(true);
    expect(first).toHaveBeenCalledTimes(1);
    expect(second).toHaveBeenCalledTimes(1);

    window.dispatchEvent(
      new StorageEvent("storage", {
        key: "unrelated",
        storageArea: window.localStorage,
      })
    );
    expect(first).toHaveBeenCalledTimes(1);

    window.dispatchEvent(
      new StorageEvent("storage", {
        key: HOME_WORKSPACE_CONFIG_SHOW_ALL_STORAGE_KEY,
        storageArea: window.localStorage,
      })
    );
    expect(first).toHaveBeenCalledTimes(2);
    expect(second).toHaveBeenCalledTimes(2);

    window.dispatchEvent(new StorageEvent("storage", { key: null }));
    expect(first).toHaveBeenCalledTimes(3);
    expect(second).toHaveBeenCalledTimes(3);

    unsubscribeFirst();
    window.dispatchEvent(new StorageEvent("storage", { key: null }));
    expect(first).toHaveBeenCalledTimes(3);
    expect(second).toHaveBeenCalledTimes(4);

    unsubscribeSecond();
    window.dispatchEvent(new StorageEvent("storage", { key: null }));
    expect(second).toHaveBeenCalledTimes(4);
  });

  it("ignores unavailable browser storage APIs", () => {
    const originalWindow = window;
    vi.stubGlobal("window", undefined);

    expect(readHomeWorkspaceConfigShowAllFromStorage()).toBe(false);
    expect(() => writeHomeWorkspaceConfigShowAllToStorage(true)).not.toThrow();

    const listener = vi.fn();
    const unsubscribe = subscribeHomeWorkspaceConfigShowAll(listener);
    unsubscribe();

    vi.stubGlobal("window", originalWindow);
    vi.spyOn(window.localStorage.__proto__, "setItem").mockImplementation(() => {
      throw new Error("storage denied");
    });

    writeHomeWorkspaceConfigShowAllToStorage(true);
    expect(listener).not.toHaveBeenCalled();
  });
});
