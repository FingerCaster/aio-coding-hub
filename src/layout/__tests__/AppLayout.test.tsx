import { render, screen } from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  resetAppStartupStatusStore,
  setAppStartupStatusSnapshot,
} from "../../app/startupStatusStore";
import { AppLayout } from "../AppLayout";

vi.mock("../../components/UpdateDialog", () => ({
  UpdateDialog: () => <div data-testid="update-dialog">update-dialog</div>,
}));

vi.mock("../../ui/Sidebar", () => ({
  Sidebar: () => <aside data-testid="sidebar">sidebar</aside>,
}));

describe("layout/AppLayout", () => {
  afterEach(() => {
    resetAppStartupStatusStore();
  });

  function renderAt(pathname: string) {
    render(
      <MemoryRouter initialEntries={[pathname]}>
        <Routes>
          <Route element={<AppLayout />}>
            <Route path="*" element={<div data-testid="outlet-content">outlet-content</div>} />
          </Route>
        </Routes>
      </MemoryRouter>
    );
  }

  it("renders sidebar, main content area (Outlet), and UpdateDialog", () => {
    renderAt("/");

    expect(screen.getByTestId("sidebar")).toBeInTheDocument();
    expect(screen.getByTestId("update-dialog")).toBeInTheDocument();
    expect(document.querySelector("[data-tauri-drag-region]")).toBeInTheDocument();
  });

  it("renders only the maintenance surface while reset recovery is blocked", () => {
    setAppStartupStatusSnapshot({
      running: false,
      maintenanceMode: true,
      currentStage: "failed",
      failedStage: "resetting_data",
      errorMessage: "reset pending",
      canRetry: true,
    });

    renderAt("/settings");

    expect(screen.getByRole("alert")).toHaveTextContent("数据重置维护未完成");
    expect(screen.queryByTestId("sidebar")).not.toBeInTheDocument();
    expect(screen.queryByTestId("update-dialog")).not.toBeInTheDocument();
  });

  it("allocates the Outlet the remaining height below the startup banner", () => {
    setAppStartupStatusSnapshot({
      running: false,
      maintenanceMode: false,
      currentStage: "failed",
      failedStage: "initializing_db",
      errorMessage: "startup pending",
      canRetry: true,
    });
    renderAt("/settings");

    const main = document.querySelector("#main-content");
    const outletContainer = screen.getByTestId("outlet-content").parentElement;

    expect(main).toHaveClass("flex", "flex-col", "min-h-0");
    expect(screen.getByRole("alert").parentElement).toBe(main);
    expect(outletContainer?.parentElement).toBe(main);
    expect(outletContainer).toHaveClass("min-h-0", "flex-1");
  });

  it.each([
    ["/", "theme-blue"],
    ["/providers/1", "theme-cyan"],
    ["/sessions/active", "theme-violet"],
    ["/workspaces/current", "theme-emerald"],
    ["/prompts/library", "theme-amber"],
    ["/mcp/servers", "theme-indigo"],
    ["/skills/local", "theme-pink"],
    ["/usage", "theme-orange"],
    ["/logs", "theme-slate"],
    ["/cli-manager", "theme-sky"],
    ["/console", "theme-rose"],
    ["/settings", "theme-slate"],
    ["/unknown", "theme-blue"],
  ])("applies route theme %s -> %s", (pathname, expectedClass) => {
    renderAt(pathname);

    expect(document.querySelector(".bg-grid-pattern")).toHaveClass(expectedClass);
  });
});
