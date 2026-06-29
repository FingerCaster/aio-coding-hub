import { renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import {
  getAppSessionStartedAtMs,
  initializeAppSessionStartedAtMs,
  resetAppSessionStartedAtMsForTests,
  useInitializeAppSession,
  useAppSessionStartedAtMs,
} from "../appSession";

describe("app/appSession", () => {
  afterEach(() => {
    resetAppSessionStartedAtMsForTests();
  });

  it("initializes one stable app-open timestamp explicitly", () => {
    expect(getAppSessionStartedAtMs()).toBeNull();

    const first = initializeAppSessionStartedAtMs(() => 1_770_000_000_000);
    const second = initializeAppSessionStartedAtMs(() => 1_780_000_000_000);

    expect(first).toBe(1_770_000_000_000);
    expect(second).toBe(1_770_000_000_000);
    expect(getAppSessionStartedAtMs()).toBe(1_770_000_000_000);
  });

  it("exposes the initialized timestamp through hooks", () => {
    const { result } = renderHook(() => useInitializeAppSession());

    expect(result.current).toEqual(expect.any(Number));
    expect(result.current).toBe(getAppSessionStartedAtMs());

    const startedAt = renderHook(() => useAppSessionStartedAtMs());
    expect(startedAt.result.current).toBe(result.current);
  });
});
