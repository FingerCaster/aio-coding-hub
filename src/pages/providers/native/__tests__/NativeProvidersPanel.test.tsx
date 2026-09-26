import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import * as service from "../../../../services/nativeCli";
import { nativeEdit, nativeList, nativeSummary } from "../../../../test/fixtures/native";
import { createQueryWrapper, createTestQueryClient } from "../../../../test/utils/reactQuery";
import { NativeProvidersPanel } from "../NativeProvidersPanel";

vi.mock("../../../../services/nativeCli", async (original) => ({
  ...(await original<typeof service>()),
  nativeCliProvidersList: vi.fn(),
  nativeCliProviderReadForEdit: vi.fn(),
  nativeCliProviderSave: vi.fn(),
  nativeCliProviderApply: vi.fn(),
  nativeCliProviderRemove: vi.fn(),
  nativeCliProviderDelete: vi.fn(),
}));
const result = {
  targetId: "pi:local:a",
  revision: "file-2",
  changed: true,
  backupPath: null,
  provider: null,
};
function mount() {
  const onImport = vi.fn();
  const client = createTestQueryClient();
  render(<NativeProvidersPanel client="pi" targetId="pi:local:a" onImport={onImport} />, {
    wrapper: createQueryWrapper(client),
  });
  return { onImport, client };
}
beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(service.nativeCliProvidersList).mockResolvedValue(nativeList());
  vi.mocked(service.nativeCliProviderReadForEdit).mockResolvedValue(nativeEdit());
  vi.mocked(service.nativeCliProviderSave).mockResolvedValue(result);
  vi.mocked(service.nativeCliProviderApply).mockResolvedValue(result);
  vi.mocked(service.nativeCliProviderRemove).mockResolvedValue(result);
  vi.mocked(service.nativeCliProviderDelete).mockResolvedValue(result);
});
describe("native provider CRUD", () => {
  it("never classifies unknown membership as an archived profile", async () => {
    vi.mocked(service.nativeCliProvidersList).mockResolvedValue(
      nativeList({
        providers: [
          nativeSummary({
            nativeKey: "unknown",
            displayName: "Unknown provider",
            state: "unknown",
          }),
          nativeSummary({
            nativeKey: "archived",
            displayName: "Archived provider",
            state: "archived",
          }),
        ],
      })
    );
    mount();
    await screen.findByText("Unknown provider");
    fireEvent.click(screen.getByRole("button", { name: "未加入(1)" }));
    expect(screen.getByText("Archived provider")).toBeInTheDocument();
    expect(screen.queryByText("Unknown provider")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "全部(2)" }));
    expect(screen.getByText("配置状态未知")).toBeInTheDocument();
  });

  it("invalidates membership filter counts when refreshing cached configuration fails", async () => {
    mount();
    await screen.findByText("Native Vendor");
    fireEvent.click(screen.getByRole("button", { name: "已加入(1)" }));
    vi.mocked(service.nativeCliProvidersList).mockRejectedValue(new Error("READ_FAILED"));
    fireEvent.click(screen.getByRole("button", { name: "刷新原生配置" }));
    await screen.findByText("配置状态未知");
    expect(screen.getByRole("button", { name: "已加入(—)" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "未加入(—)" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "全部(1)" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByText("Native Vendor")).toBeInTheDocument();
  });

  it("lists only safe summaries and imports only after explicit action", async () => {
    const { onImport } = mount();
    await screen.findByText("Native Vendor");
    expect(service.nativeCliProviderReadForEdit).not.toHaveBeenCalled();
    expect(screen.queryByText(/secret-host|read-secret|private/)).not.toBeInTheDocument();
    expect(onImport).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "添加到 AIO 网关" }));
    expect(onImport).toHaveBeenCalledWith("vendor");
  });
  it("removes and reapplies through revision-bound membership commands", async () => {
    mount();
    fireEvent.click(await screen.findByRole("button", { name: "从原生 CLI 移除" }));
    await waitFor(() =>
      expect(service.nativeCliProviderRemove).toHaveBeenCalledWith({
        targetId: "pi:local:a",
        nativeKey: "vendor",
        expectedRevision: "file-1",
        expectedNodeDigest: "node-1",
        expectedProfileRevision: "profile-1",
      })
    );
    expect(service.nativeCliProviderDelete).not.toHaveBeenCalled();
  });
  it("applies an archived profile without fabricating its raw node", async () => {
    vi.mocked(service.nativeCliProvidersList).mockResolvedValue(
      nativeList({ providers: [nativeSummary({ state: "archived", nodeDigest: null })] })
    );
    mount();
    fireEvent.click(await screen.findByRole("button", { name: "加入原生 CLI" }));
    await waitFor(() =>
      expect(service.nativeCliProviderApply).toHaveBeenCalledWith(
        expect.objectContaining({ expectedNodeDigest: null, expectedProfileRevision: "profile-1" })
      )
    );
    expect(service.nativeCliProviderSave).not.toHaveBeenCalled();
  });
  it("requires confirmation to delete a present node and its profile", async () => {
    mount();
    fireEvent.click(await screen.findByRole("button", { name: "删除档案" }));
    expect(service.nativeCliProviderDelete).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "确认删除" }));
    await waitFor(() =>
      expect(service.nativeCliProviderDelete).toHaveBeenCalledWith(
        expect.objectContaining({ removeFromNative: true, expectedRevision: "file-1" })
      )
    );
  });
  it("loads secrets only for editing and sends only changed fields", async () => {
    mount();
    fireEvent.click(await screen.findByRole("button", { name: "编辑" }));
    const dialog = await screen.findByRole("dialog");
    expect(service.nativeCliProviderReadForEdit).toHaveBeenCalledWith("pi:local:a", "vendor");
    fireEvent.change(within(dialog).getByLabelText("原生 Base URL"), {
      target: { value: "https://new.test" },
    });
    expect(within(dialog).getByRole("button", { name: "仅保存档案" })).toBeDisabled();
    fireEvent.click(within(dialog).getByRole("button", { name: "保存并加入 Pi" }));
    await waitFor(() =>
      expect(service.nativeCliProviderSave).toHaveBeenCalledWith(
        expect.objectContaining({
          node: null,
          patch: [{ path: ["baseUrl"], value: "https://new.test" }],
          apply: true,
          expectedRevision: "file-1",
        })
      )
    );
  });
  it("creates an explicit archive without applying it to the native file", async () => {
    mount();
    await screen.findByText("Native Vendor");
    fireEvent.click(screen.getByRole("button", { name: "新增原生供应商" }));
    const dialog = screen.getByRole("dialog");
    fireEvent.change(within(dialog).getByLabelText("原生标识"), {
      target: { value: "new-provider" },
    });
    fireEvent.change(within(dialog).getByLabelText("完整原生节点 JSON"), {
      target: { value: JSON.stringify(nativeEdit().node) },
    });
    fireEvent.click(within(dialog).getByRole("button", { name: "仅保存档案" }));
    await waitFor(() =>
      expect(service.nativeCliProviderSave).toHaveBeenCalledWith(
        expect.objectContaining({
          nativeKey: "new-provider",
          node: nativeEdit().node,
          patch: [],
          apply: false,
        })
      )
    );
  });
  it("never treats invalid or unreadable configuration as an empty writable file", async () => {
    vi.mocked(service.nativeCliProvidersList).mockResolvedValue(
      nativeList({ parseStatus: "invalid", revision: null })
    );
    mount();
    await screen.findByText("Native Vendor");
    expect(screen.getByRole("button", { name: "新增原生供应商" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "编辑" })).toBeDisabled();
    expect(screen.getByText("配置状态未知")).toBeInTheDocument();
  });
  it("reports conflicts without closing the editor or leaking backend details", async () => {
    vi.mocked(service.nativeCliProviderSave).mockRejectedValue(
      new Error("NATIVE_REVISION_CONFLICT apiKey=do-not-display")
    );
    mount();
    fireEvent.click(await screen.findByRole("button", { name: "编辑" }));
    fireEvent.click(await screen.findByRole("button", { name: "保存并加入 Pi" }));
    await screen.findByText(/配置或预览已变化/);
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    expect(screen.queryByText(/do-not-display/)).not.toBeInTheDocument();
  });
  it("filters providers by status pills and search query", async () => {
    vi.mocked(service.nativeCliProvidersList).mockResolvedValue(
      nativeList({
        providers: [
          nativeSummary({ nativeKey: "active-one", displayName: "Active Alpha", state: "present" }),
          nativeSummary({
            nativeKey: "archived-two",
            displayName: "Archived Beta",
            state: "archived",
          }),
        ],
      })
    );
    mount();
    await screen.findByText("Active Alpha");
    expect(screen.getByText("Archived Beta")).toBeInTheDocument();

    // Filter by archived
    fireEvent.click(screen.getByRole("button", { name: "未加入(1)" }));
    expect(screen.queryByText("Active Alpha")).not.toBeInTheDocument();
    expect(screen.getByText("Archived Beta")).toBeInTheDocument();

    // Filter by present
    fireEvent.click(screen.getByRole("button", { name: "已加入(1)" }));
    expect(screen.getByText("Active Alpha")).toBeInTheDocument();
    expect(screen.queryByText("Archived Beta")).not.toBeInTheDocument();

    // Reset to all
    fireEvent.click(screen.getByRole("button", { name: "全部(2)" }));
    expect(screen.getByText("Active Alpha")).toBeInTheDocument();
    expect(screen.getByText("Archived Beta")).toBeInTheDocument();

    // Search query
    fireEvent.change(screen.getByLabelText("搜索原生供应商"), { target: { value: "Alpha" } });
    expect(screen.getByText("Active Alpha")).toBeInTheDocument();
    expect(screen.queryByText("Archived Beta")).not.toBeInTheDocument();
  });
});
