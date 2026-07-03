import { createElement, useLayoutEffect } from "react";
import { render, renderHook } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { createTestAppSettings } from "../../../test/fixtures/settings";
import { useSettingsPersistenceReadState } from "../useSettingsPersistenceReadState";

const baseSettings = createTestAppSettings({
  preferred_port: 1420,
  auto_start: false,
  start_minimized: false,
  tray_enabled: true,
  log_retention_days: 30,
  show_home_heatmap: true,
  show_home_usage: true,
  home_usage_period: "last15",
});

describe("pages/settings/useSettingsPersistenceReadState", () => {
  it("applies a fresh settings snapshot and marks settings ready", () => {
    const hook = renderHook(() =>
      useSettingsPersistenceReadState({
        settingsQuery: {
          data: baseSettings as any,
          dataUpdatedAt: 100,
          error: null,
          isError: false,
          isLoading: false,
        },
      })
    );

    expect(hook.result.current.settingsReady).toBe(true);
    expect(hook.result.current.settingsReadErrorMessage).toBeNull();
    expect(hook.result.current.appliedSettings).toMatchObject({
      preferred_port: 1420,
      auto_start: false,
      start_minimized: false,
      tray_enabled: true,
    });
  });

  it("reports read protection and blocks writes when settings cannot be read", () => {
    const hook = renderHook(() =>
      useSettingsPersistenceReadState({
        settingsQuery: {
          data: null,
          dataUpdatedAt: 0,
          error: new Error("permission denied"),
          isError: true,
          isLoading: false,
        },
      })
    );

    expect(hook.result.current.settingsReady).toBe(true);
    expect(hook.result.current.settingsWriteBlocked).toBe(true);
    expect(hook.result.current.settingsReadErrorMessage).toBeTruthy();
    expect(hook.result.current.appliedSettings.preferred_port).not.toBe(1420);
  });

  it("commits read-protected state without an intermediate writable frame", () => {
    const committedStates: Array<{
      settingsReady: boolean;
      settingsWriteBlocked: boolean;
      settingsReadErrorMessage: string | null;
    }> = [];

    function Probe(props: { settingsQuery: any }) {
      const state = useSettingsPersistenceReadState({
        settingsQuery: props.settingsQuery,
      });

      useLayoutEffect(() => {
        committedStates.push({
          settingsReady: state.settingsReady,
          settingsWriteBlocked: state.settingsWriteBlocked,
          settingsReadErrorMessage: state.settingsReadErrorMessage,
        });
      });

      return null;
    }

    const view = render(
      createElement(Probe, {
        settingsQuery: {
          data: null,
          dataUpdatedAt: 0,
          error: null,
          isError: false,
          isLoading: true,
        },
      })
    );

    committedStates.length = 0;

    view.rerender(
      createElement(Probe, {
        settingsQuery: {
          data: null,
          dataUpdatedAt: 0,
          error: new Error("permission denied"),
          isError: true,
          isLoading: false,
        },
      })
    );

    expect(committedStates).toEqual([
      {
        settingsReady: true,
        settingsWriteBlocked: true,
        settingsReadErrorMessage: expect.any(String),
      },
    ]);
  });

  it("keeps local readonly protection until newer settings data arrives", () => {
    const settingsQuery = {
      data: baseSettings,
      dataUpdatedAt: 100,
      error: null,
      isError: false,
      isLoading: false,
    };

    const hook = renderHook(() =>
      useSettingsPersistenceReadState({
        settingsQuery,
      })
    );

    hook.result.current.setSettingsReadErrorMessage("blocked by write failure");
    hook.rerender();

    expect(hook.result.current.settingsWriteBlocked).toBe(true);

    hook.rerender();

    expect(hook.result.current.settingsWriteBlocked).toBe(true);
    expect(hook.result.current.settingsReadErrorMessage).toBe("blocked by write failure");
  });
});
