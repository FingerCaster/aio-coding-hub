import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClientProvider } from "@tanstack/react-query";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { toast } from "sonner";
import { NativeCliUpdateCard } from "../NativeCliUpdateCard";
import {
  nativeCliCheckLatestVersion,
  nativeCliUpdate,
  type NativeCliVersionCheck,
} from "../../../services/cli/nativeCliUpdate";
import { confirmDesktopDialog } from "../../../services/desktop/confirm";
import { createTestQueryClient } from "../../../test/utils/reactQuery";

vi.mock("../../../services/cli/nativeCliUpdate", () => ({
  nativeCliCheckLatestVersion: vi.fn(),
  nativeCliUpdate: vi.fn(),
}));
vi.mock("../../../services/desktop/confirm", () => ({ confirmDesktopDialog: vi.fn() }));
vi.mock("sonner", () => ({ toast: { success: vi.fn(), error: vi.fn() } }));
const preview: NativeCliVersionCheck = {
  client: "pi",
  installed: true,
  installedVersion: "0.86.0",
  latestVersion: "0.87.1",
  updateAvailable: true,
  installMethod: "npm",
  installDirectory: "C:/Users/test/npm",
  executablePath: "C:/Users/test/npm/pi.cmd",
  planId: "a".repeat(32),
  blockedReason: null,
};
function setup(client: "pi" | "omp" = "pi") {
  const queryClient = createTestQueryClient();
  const view = render(
    <QueryClientProvider client={queryClient}>
      <NativeCliUpdateCard key={client} client={client} />
    </QueryClientProvider>
  );
  return { ...view, user: userEvent.setup(), queryClient };
}
beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(nativeCliCheckLatestVersion).mockResolvedValue(preview);
  vi.mocked(confirmDesktopDialog).mockResolvedValue(false);
  vi.mocked(nativeCliUpdate).mockResolvedValue({
    cliKey: "pi",
    success: true,
    output: "done",
    error: null,
  });
});
afterEach(() => vi.useRealTimers());

describe("native CLI explicit updates", () => {
  it("automatically checks and refreshes without installing", async () => {
    const { user } = setup();
    await screen.findByText("发现新版本");
    expect(nativeCliCheckLatestVersion).toHaveBeenCalledWith("pi");
    expect(nativeCliUpdate).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "检查更新" }));
    await waitFor(() => expect(nativeCliCheckLatestVersion).toHaveBeenCalledTimes(2));
    expect(nativeCliUpdate).not.toHaveBeenCalled();
    expect(confirmDesktopDialog).not.toHaveBeenCalled();
  });
  it("never updates after cancellation and confirms the pinned version and path", async () => {
    const { user } = setup();
    await user.click(await screen.findByRole("button", { name: "一键升级" }));
    expect(confirmDesktopDialog).toHaveBeenCalledWith(expect.stringContaining("v0.87.1"));
    expect(confirmDesktopDialog).toHaveBeenCalledWith(expect.stringContaining("C:/Users/test/npm"));
    expect(nativeCliUpdate).not.toHaveBeenCalled();
  });
  it("installs only after confirmation and refreshes both version and local info", async () => {
    vi.mocked(confirmDesktopDialog).mockResolvedValue(true);
    vi.mocked(nativeCliCheckLatestVersion)
      .mockResolvedValueOnce(preview)
      .mockResolvedValue({
        ...preview,
        installedVersion: preview.latestVersion,
        updateAvailable: false,
        planId: null,
      });
    const { user, queryClient } = setup();
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    await user.click(await screen.findByRole("button", { name: "一键升级" }));
    await screen.findByText("已是最新稳定版");
    expect(nativeCliUpdate).toHaveBeenCalledExactlyOnceWith("pi", preview.planId);
    expect(invalidate).toHaveBeenCalledWith({ queryKey: ["cli-manager", "native-info", "pi"] });
    expect(toast.success).toHaveBeenCalled();
  });
  it("offers first installation for OMP without running it automatically", async () => {
    vi.mocked(nativeCliCheckLatestVersion).mockResolvedValue({
      ...preview,
      client: "omp",
      installed: false,
      installedVersion: null,
      installMethod: "官方独立程序",
      updateAvailable: false,
    });
    const { user } = setup("omp");
    await user.click(await screen.findByRole("button", { name: "下载安装" }));
    expect(confirmDesktopDialog).toHaveBeenCalledWith(expect.stringContaining("下载安装 OMP"));
    expect(nativeCliUpdate).not.toHaveBeenCalled();
  });
  it("does not mark an unknown installed version as latest or offer replacement", async () => {
    vi.mocked(nativeCliCheckLatestVersion).mockResolvedValue({
      ...preview,
      installedVersion: null,
      updateAvailable: false,
      planId: null,
      blockedReason: "需要手动更新",
    });
    setup();
    await screen.findByText("当前版本未知，无法比较");
    expect(screen.queryByRole("button", { name: "一键升级" })).not.toBeInTheDocument();
    expect(screen.queryByText("已是最新稳定版")).not.toBeInTheDocument();
  });
  it("retains a useful error when a check fails and retries only the check", async () => {
    vi.mocked(nativeCliCheckLatestVersion).mockRejectedValueOnce(new Error("网络不可用"));
    const { user } = setup();
    await screen.findByRole("alert");
    await user.click(screen.getByRole("button", { name: "检查更新" }));
    await screen.findByText("发现新版本");
    expect(nativeCliUpdate).not.toHaveBeenCalled();
  });
  it("reports a failed install without claiming success or retrying installation", async () => {
    vi.mocked(confirmDesktopDialog).mockResolvedValue(true);
    vi.mocked(nativeCliUpdate).mockResolvedValue({
      cliKey: "pi",
      success: false,
      output: "",
      error: "权限不足",
    });
    const { user } = setup();
    await user.click(await screen.findByRole("button", { name: "一键升级" }));
    await screen.findByText("权限不足");
    expect(toast.success).not.toHaveBeenCalled();
    expect(nativeCliUpdate).toHaveBeenCalledTimes(1);
  });
  it("disables duplicate updates while installation is running", async () => {
    vi.mocked(confirmDesktopDialog).mockResolvedValue(true);
    let finish!: (value: Awaited<ReturnType<typeof nativeCliUpdate>>) => void;
    vi.mocked(nativeCliUpdate).mockImplementation(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        })
    );
    const { user } = setup();
    await user.click(await screen.findByRole("button", { name: "一键升级" }));
    expect(await screen.findByRole("button", { name: "正在下载并安装…" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "检查更新" })).toBeDisabled();
    await act(async () => finish({ cliKey: "pi", success: true, output: "", error: null }));
    expect(nativeCliUpdate).toHaveBeenCalledTimes(1);
  });
  it("does not install if the page unmounts while confirmation is pending", async () => {
    let accept!: (value: boolean) => void;
    vi.mocked(confirmDesktopDialog).mockImplementation(
      () =>
        new Promise((resolve) => {
          accept = resolve;
        })
    );
    const { user, unmount } = setup();
    await user.click(await screen.findByRole("button", { name: "一键升级" }));
    unmount();
    await act(async () => accept(true));
    expect(nativeCliUpdate).not.toHaveBeenCalled();
  });
  it("scheduled checks never call the installation command", async () => {
    vi.useFakeTimers();
    const { unmount } = setup();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(20);
    });
    expect(nativeCliCheckLatestVersion).toHaveBeenCalledTimes(1);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(60 * 60 * 1000);
    });
    expect(nativeCliCheckLatestVersion).toHaveBeenCalledTimes(2);
    expect(nativeCliUpdate).not.toHaveBeenCalled();
    expect(confirmDesktopDialog).not.toHaveBeenCalled();
    unmount();
  });
});
