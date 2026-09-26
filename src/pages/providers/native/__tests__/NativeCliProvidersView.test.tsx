import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { Link, MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";
import * as nativeCliService from "../../../../services/nativeCli";
import * as nativeGatewayService from "../../../../services/nativeGateway";
import { gatewayPreview, nativeList, nativeTarget } from "../../../../test/fixtures/native";
import { createQueryWrapper, createTestQueryClient } from "../../../../test/utils/reactQuery";
import { NativeCliProvidersView } from "../NativeCliProvidersView";

vi.mock("../../../../services/nativeCli", async (original) => ({
  ...(await original<typeof nativeCliService>()),
  nativeCliTargetsList: vi.fn(),
  nativeCliProvidersList: vi.fn(),
  nativeCliTargetValidate: vi.fn(),
  nativeCliTargetSelect: vi.fn(),
}));

vi.mock("../../../../services/nativeGateway", async (original) => ({
  ...(await original<typeof nativeGatewayService>()),
  nativeGatewayCatalogPreview: vi.fn(),
}));

vi.mock("../../../../query/requestLogs", async () => {
  const { useQuery } = await import("@tanstack/react-query");
  return {
    useRequestLogsListAllQuery: () =>
      useQuery({ queryKey: ["test-request-logs"], queryFn: async () => [], initialData: [] }),
  };
});

vi.mock("../../ProvidersView", () => ({
  ProvidersView: () => <div data-testid="mock-providers-view">AIO ProvidersView</div>,
}));

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(nativeCliService.nativeCliTargetsList).mockResolvedValue([
    nativeTarget({ client: "pi", targetId: "pi:default", selected: true }),
    nativeTarget({
      client: "pi",
      targetId: "pi:custom",
      selected: false,
      modelsPath: "C:/custom/models.json",
    }),
  ]);
  vi.mocked(nativeCliService.nativeCliProvidersList).mockResolvedValue(nativeList());
  vi.mocked(nativeGatewayService.nativeGatewayCatalogPreview).mockResolvedValue(gatewayPreview());
});

describe("NativeCliProvidersView integration", () => {
  it("resets the view and target when navigation removes the query parameters", async () => {
    render(
      <MemoryRouter initialEntries={["/providers?cli=pi&target=pi:custom&view=gateway"]}>
        <NativeCliProvidersView client="pi" setActiveCli={vi.fn()} />
        <Link to="/providers?cli=pi">恢复默认视图</Link>
      </MemoryRouter>,
      { wrapper: createQueryWrapper(createTestQueryClient()) }
    );
    await screen.findByTestId("mock-providers-view");
    await waitFor(() => expect(screen.getByLabelText("当前原生配置目标")).toHaveValue("pi:custom"));
    fireEvent.click(screen.getByRole("link", { name: "恢复默认视图" }));
    await waitFor(() =>
      expect(screen.getByLabelText("当前原生配置目标")).toHaveValue("pi:default")
    );
    expect(screen.getByRole("tab", { name: "原生配置" })).toHaveAttribute("aria-selected", "true");
    expect(screen.queryByTestId("mock-providers-view")).not.toBeInTheDocument();
  });

  it("renders CLI manager navigation link for the active client", async () => {
    const queryClient = createTestQueryClient();
    render(
      <MemoryRouter initialEntries={["/providers?cli=pi"]}>
        <NativeCliProvidersView client="pi" setActiveCli={vi.fn()} />
      </MemoryRouter>,
      { wrapper: createQueryWrapper(queryClient) }
    );

    const link = await screen.findByRole("link", { name: /CLI 管理/ });
    expect(link).toHaveAttribute("href", "/cli-manager?tab=pi");
  });

  it("reads view=gateway from search params and switches views", async () => {
    const queryClient = createTestQueryClient();
    render(
      <MemoryRouter initialEntries={["/providers?cli=pi&view=gateway"]}>
        <NativeCliProvidersView client="pi" setActiveCli={vi.fn()} />
      </MemoryRouter>,
      { wrapper: createQueryWrapper(queryClient) }
    );

    await screen.findByTestId("mock-providers-view");
    await screen.findByText("独立 AIO 入口");

    // Switch to native
    fireEvent.click(screen.getByRole("tab", { name: "原生配置" }));
    await screen.findByText("原生配置目标");
  });

  it("respects target parameter from search params", async () => {
    const queryClient = createTestQueryClient();
    render(
      <MemoryRouter initialEntries={["/providers?cli=pi&target=pi:custom"]}>
        <NativeCliProvidersView client="pi" setActiveCli={vi.fn()} />
      </MemoryRouter>,
      { wrapper: createQueryWrapper(queryClient) }
    );

    await screen.findByText("原生配置目标");
    const select = screen.getByLabelText("当前原生配置目标");
    expect(select).toHaveValue("pi:custom");
  });
});
