import { render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { describe, expect, it, vi } from "vitest";
import { AppRoutes } from "../AppRoutes";

vi.mock("../../layout/AppLayout", async () => {
  const { Outlet } = await vi.importActual<typeof import("react-router-dom")>("react-router-dom");
  return {
    AppLayout: () => (
      <div>
        <span>layout-shell</span>
        <Outlet />
      </div>
    ),
  };
});

vi.mock("../../ui/Spinner", () => ({
  Spinner: () => <div role="status">loading-route</div>,
}));

vi.mock("../../pages/HomePage", () => ({
  HomePage: () => <h1>home-route</h1>,
}));
vi.mock("../../pages/ProvidersPage", () => ({
  ProvidersPage: () => <h1>providers-route</h1>,
}));
vi.mock("../../pages/SessionsPage", () => ({
  SessionsPage: () => <h1>sessions-route</h1>,
}));
vi.mock("../../pages/SessionsProjectPage", () => ({
  SessionsProjectPage: () => <h1>sessions-project-route</h1>,
}));
vi.mock("../../pages/SessionsMessagesPage", () => ({
  SessionsMessagesPage: () => <h1>sessions-messages-route</h1>,
}));
vi.mock("../../pages/WorkspacesPage", () => ({
  WorkspacesPage: () => <h1>workspaces-route</h1>,
}));
vi.mock("../../pages/PromptsPage", () => ({
  PromptsPage: () => <h1>prompts-route</h1>,
}));
vi.mock("../../pages/McpPage", () => ({
  McpPage: () => <h1>mcp-route</h1>,
}));
vi.mock("../../pages/PluginsPage", () => ({
  PluginsPage: () => <h1>plugins-route</h1>,
}));
vi.mock("../../pages/LogsPage", () => ({
  LogsPage: () => <h1>logs-route</h1>,
}));
vi.mock("../../pages/ConsolePage", () => ({
  ConsolePage: () => <h1>console-route</h1>,
}));
vi.mock("../../pages/UsagePage", () => ({
  UsagePage: () => <h1>usage-route</h1>,
}));
vi.mock("../../pages/SettingsPage", () => ({
  SettingsPage: () => <h1>settings-route</h1>,
}));
vi.mock("../../pages/CliManagerPage", () => ({
  CliManagerPage: () => <h1>cli-manager-route</h1>,
}));
vi.mock("../../pages/SkillsPage", () => ({
  SkillsPage: () => <h1>skills-route</h1>,
}));
vi.mock("../../pages/SkillsMarketPage", () => ({
  SkillsMarketPage: () => <h1>skills-market-route</h1>,
}));

function renderRoute(path: string) {
  return render(
    <MemoryRouter initialEntries={[path]}>
      <AppRoutes />
    </MemoryRouter>
  );
}

describe("app/AppRoutes", () => {
  it.each([
    ["/", "home-route"],
    ["/providers", "providers-route"],
    ["/sessions", "sessions-route"],
    ["/sessions/claude/project-1", "sessions-project-route"],
    ["/sessions/claude/project-1/session/session-1", "sessions-messages-route"],
    ["/workspaces", "workspaces-route"],
    ["/prompts", "prompts-route"],
    ["/mcp", "mcp-route"],
    ["/plugins", "plugins-route"],
    ["/logs", "logs-route"],
    ["/console", "console-route"],
    ["/usage", "usage-route"],
    ["/settings/general", "settings-route"],
    ["/cli-manager", "cli-manager-route"],
    ["/skills", "skills-route"],
    ["/skills/market", "skills-market-route"],
  ])("renders %s", async (path, heading) => {
    renderRoute(path);

    expect(await screen.findByRole("heading", { name: heading })).toBeInTheDocument();
    expect(screen.getByText("layout-shell")).toBeInTheDocument();
  });

  it("redirects unknown paths to home", async () => {
    renderRoute("/missing");

    await waitFor(() => {
      expect(screen.getByRole("heading", { name: "home-route" })).toBeInTheDocument();
    });
  });
});
