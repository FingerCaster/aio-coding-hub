import { fireEvent, render, screen } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import type { ReactElement } from "react";
import { MemoryRouter, useLocation, useNavigate } from "react-router-dom";
import { describe, expect, it, vi } from "vitest";
import { ProvidersPage } from "../ProvidersPage";
import { createTestQueryClient } from "../../test/utils/reactQuery";
import { useProvidersListQuery } from "../../query/providers";
import { useSettingsQuery } from "../../query/settings";
import { createTestAppSettings } from "../../test/fixtures/settings";

vi.mock("../providers/ProvidersView", () => ({
  ProvidersView: ({ activeCli }: any) => (
    <div data-testid="providers-view">providers:{activeCli}</div>
  ),
}));
vi.mock("../providers/native/NativeCliProvidersView", () => ({
  NativeCliProvidersView: ({ client }: { client: string }) => <div>native:{client}</div>,
}));

vi.mock("../../query/providers", async () => {
  const actual =
    await vi.importActual<typeof import("../../query/providers")>("../../query/providers");
  return { ...actual, useProvidersListQuery: vi.fn() };
});

vi.mock("../../query/settings", async () => {
  const actual =
    await vi.importActual<typeof import("../../query/settings")>("../../query/settings");
  return { ...actual, useSettingsQuery: vi.fn() };
});

function LocationProbe() {
  const location = useLocation();
  const navigate = useNavigate();
  return (
    <>
      <output data-testid="location">{location.search}</output>
      <button onClick={() => navigate(-1)}>返回上一页</button>
    </>
  );
}

function renderWithProviders(element: ReactElement, initialEntry = "/providers") {
  const client = createTestQueryClient();
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={[initialEntry]}>
        {element}
        <LocationProbe />
      </MemoryRouter>
    </QueryClientProvider>
  );
}

describe("pages/ProvidersPage", () => {
  it("clears another client target on channel switch and follows browser history", () => {
    vi.mocked(useSettingsQuery).mockReturnValue({
      data: createTestAppSettings(),
    } as any);
    renderWithProviders(<ProvidersPage />, "/providers?cli=pi&view=gateway&target=pi%3Alocal%3Aa");
    expect(screen.getByText("native:pi")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("tab", { name: "OMP" }));
    expect(screen.getByText("native:omp")).toBeInTheDocument();
    expect(screen.getByTestId("location").textContent).toBe("?cli=omp&view=gateway");
    fireEvent.click(screen.getByRole("button", { name: "返回上一页" }));
    expect(screen.getByText("native:pi")).toBeInTheDocument();
    expect(screen.getByTestId("location").textContent).toContain("target=pi%3Alocal%3Aa");
  });

  it("uses top tabs to switch CLI providers view", () => {
    vi.mocked(useSettingsQuery).mockReturnValue({
      data: createTestAppSettings({ cli_priority_order: ["codex", "claude", "gemini"] }),
    } as any);
    vi.mocked(useProvidersListQuery).mockReturnValue({
      data: [],
      isFetching: false,
    } as any);

    renderWithProviders(<ProvidersPage />);

    expect(screen.getByRole("heading", { level: 1, name: "供应商" })).toBeInTheDocument();
    expect(screen.getByTestId("providers-view")).toBeInTheDocument();
    expect(screen.getByText("providers:codex")).toBeInTheDocument();

    expect(screen.getAllByRole("tab").map((tab) => tab.textContent)).toEqual([
      "Codex",
      "Claude",
      "Gemini",
      "Grok",
      "Pi",
      "OMP",
    ]);

    fireEvent.click(screen.getByRole("tab", { name: "Claude" }));

    expect(screen.getByRole("heading", { level: 1, name: "供应商" })).toBeInTheDocument();
    expect(screen.getByText("providers:claude")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("tab", { name: "Pi" }));
    expect(screen.getByText("native:pi")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("tab", { name: "OMP" }));
    expect(screen.getByText("native:omp")).toBeInTheDocument();
  });
});
