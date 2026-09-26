import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import * as gateway from "../../../../services/nativeGateway";
import { NATIVE_GATEWAY_PROTOCOLS } from "../../../../constants/nativeGateway";
import { gatewayPreview, nativeModel } from "../../../../test/fixtures/native";
import { createQueryWrapper, createTestQueryClient } from "../../../../test/utils/reactQuery";
import { nativeCliKeys } from "../../../../query/nativeCli";
import { NativeGatewayImportDialog } from "../NativeGatewayImportDialog";
import { NativeGatewayEntries } from "../NativeGatewayEntries";

vi.mock("../../../../services/nativeGateway", async (original) => ({
  ...(await original<typeof gateway>()),
  nativeGatewayImportPreview: vi.fn(),
  nativeGatewayImportConfirm: vi.fn(),
  nativeGatewayCatalogPreview: vi.fn(),
  nativeGatewayApply: vi.fn(),
  nativeGatewayRemove: vi.fn(),
}));
vi.mock("../../../../query/requestLogs", async () => {
  const { useQuery } = await import("@tanstack/react-query");
  return {
    useRequestLogsListAllQuery: () =>
      useQuery({ queryKey: ["test-request-logs"], queryFn: async () => [], initialData: [] }),
  };
});
const imports = () => ({
  targetId: "pi:local:a",
  nativeKey: "vendor",
  revision: "file-1",
  canImport: true,
  issues: [],
  groups: NATIVE_GATEWAY_PROTOCOLS.map(({ key }, index) => ({
    groupId: `group-${index}`,
    protocol: key,
    baseUrl: `https://upstream-${index}.test`,
    models: [nativeModel({ requestModelId: `model-${index}` })],
  })),
});
beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(gateway.nativeGatewayImportPreview).mockResolvedValue(imports());
  vi.mocked(gateway.nativeGatewayImportConfirm).mockResolvedValue([]);
  vi.mocked(gateway.nativeGatewayCatalogPreview).mockResolvedValue(gatewayPreview());
  vi.mocked(gateway.nativeGatewayApply).mockResolvedValue({
    targetId: "pi:local:a",
    revision: "file-2",
    changed: true,
    backupPath: null,
    manifests: [],
  });
  vi.mocked(gateway.nativeGatewayRemove).mockResolvedValue({
    targetId: "pi:local:a",
    revision: "file-2",
    changed: true,
    backupPath: null,
    manifests: [],
  });
});
function importDialog() {
  const onImported = vi.fn();
  render(
    <NativeGatewayImportDialog
      targetId="pi:local:a"
      nativeKey="vendor"
      onClose={vi.fn()}
      onImported={onImported}
    />,
    { wrapper: createQueryWrapper(createTestQueryClient()) }
  );
  return onImported;
}
async function fillCredentials() {
  await screen.findByText("anthropic-messages");
  for (const { key } of NATIVE_GATEWAY_PROTOCOLS)
    fireEvent.change(screen.getByLabelText(`AIO API Key · ${key}`), {
      target: { value: `literal-${key}` },
    });
}
describe("explicit native gateway import", () => {
  it("previews all four protocol groups and requires credentials plus confirmation", async () => {
    const onImported = importDialog();
    await screen.findByText("anthropic-messages");
    expect(gateway.nativeGatewayImportConfirm).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "确认导入为停用上游" })).toBeDisabled();
    await fillCredentials();
    fireEvent.click(screen.getByRole("button", { name: "确认导入为停用上游" }));
    await waitFor(() => expect(onImported).toHaveBeenCalledOnce());
    expect(gateway.nativeGatewayImportConfirm).toHaveBeenCalledWith({
      targetId: "pi:local:a",
      nativeKey: "vendor",
      expectedRevision: "file-1",
      credentials: NATIVE_GATEWAY_PROTOCOLS.map(({ key }, index) => ({
        groupId: `group-${index}`,
        apiKey: `literal-${key}`,
      })),
    });
    expect(gateway.nativeGatewayApply).not.toHaveBeenCalled();
  });
  it("blocks unsupported capabilities instead of importing a lossy subset", async () => {
    vi.mocked(gateway.nativeGatewayImportPreview).mockResolvedValue({
      ...imports(),
      canImport: false,
      issues: [{ modelId: "model-0", code: "UNSUPPORTED_HEADERS", blocking: true }],
    });
    importDialog();
    await fillCredentials();
    expect(screen.getByRole("button", { name: "确认导入为停用上游" })).toBeDisabled();
    expect(screen.getByText(/无法安全导入/)).toBeInTheDocument();
  });
  it("clears credentials and requires a fresh preview after a conflict", async () => {
    vi.mocked(gateway.nativeGatewayImportConfirm).mockRejectedValue(
      new Error("NATIVE_REVISION_CONFLICT")
    );
    const onImported = importDialog();
    await fillCredentials();
    fireEvent.click(screen.getByRole("button", { name: "确认导入为停用上游" }));
    await screen.findByText(/配置或预览已变化/);
    expect(onImported).not.toHaveBeenCalled();
    expect(screen.queryByLabelText(/AIO API Key/)).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "确认导入为停用上游" })).toBeDisabled();
  });
});
function entries() {
  const client = createTestQueryClient();
  render(<NativeGatewayEntries client="pi" targetId="pi:local:a" />, {
    wrapper: createQueryWrapper(client),
  });
  return client;
}
describe("independent AIO entries", () => {
  it("marks cached manifest status unknown after a refresh failure", async () => {
    vi.mocked(gateway.nativeGatewayCatalogPreview).mockResolvedValue(
      gatewayPreview({
        manifests: [
          {
            protocol: "openai-responses",
            nativeKey: "aio-responses",
            generation: 1,
            state: "applied",
            stale: false,
            modified: false,
          },
        ],
      })
    );
    entries();
    await screen.findByText(/aio-responses.*已配置/);
    vi.mocked(gateway.nativeGatewayCatalogPreview).mockRejectedValue(
      new Error("NATIVE_READ_FAILED")
    );
    fireEvent.click(screen.getByRole("button", { name: "刷新状态" }));
    await screen.findByText(/配置状态未知/);
    expect(screen.queryByText(/aio-responses.*已配置/)).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "预览入口变更" })).toBeDisabled();
  });
  it("shows listener, configuration and observed traffic separately and applies only after confirmation", async () => {
    entries();
    await screen.findByText("入口未配置");
    expect(screen.getByText(/最近 100 条记录中未观测/)).toBeInTheDocument();
    expect(gateway.nativeGatewayApply).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "预览入口变更" }));
    await screen.findByRole("dialog");
    expect(gateway.nativeGatewayApply).not.toHaveBeenCalled();
    expect(screen.getByText(/aio-responses/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "确认应用" }));
    await waitFor(() =>
      expect(gateway.nativeGatewayApply).toHaveBeenCalledWith({
        targetId: "pi:local:a",
        expectedRevision: "file-1",
        catalogRevision: "catalog-1",
      })
    );
    await screen.findByText(/请在原生 CLI 的模型选择器中选择 AIO/);
  });
  it("blocks a stale confirmation after the catalog changes", async () => {
    const client = entries();
    await screen.findByText("入口未配置");
    fireEvent.click(screen.getByRole("button", { name: "预览入口变更" }));
    await screen.findByRole("dialog");
    await act(async () =>
      client.setQueryData(
        nativeCliKeys.gateway("pi:local:a"),
        gatewayPreview({ catalogRevision: "catalog-2" })
      )
    );
    await waitFor(() => expect(screen.getByRole("button", { name: "确认应用" })).toBeDisabled());
    expect(gateway.nativeGatewayApply).not.toHaveBeenCalled();
  });
  it("shows external modification and never claims failed removal succeeded", async () => {
    vi.mocked(gateway.nativeGatewayCatalogPreview).mockResolvedValue(
      gatewayPreview({
        manifests: [
          {
            protocol: "openai-responses",
            nativeKey: "aio-responses",
            generation: 1,
            state: "applied",
            stale: false,
            modified: true,
          },
        ],
      })
    );
    vi.mocked(gateway.nativeGatewayRemove).mockRejectedValue(new Error("OWNERSHIP_CONFLICT"));
    entries();
    await screen.findByText(/配置被外部修改/);
    expect(screen.getByRole("button", { name: "预览入口变更" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "预览移除入口" }));
    fireEvent.click(await screen.findByRole("button", { name: "确认移除" }));
    await screen.findByText(/配置或预览已变化/);
    expect(screen.queryByText(/已移除仍由 AIO 拥有的入口/)).not.toBeInTheDocument();
  });
});
