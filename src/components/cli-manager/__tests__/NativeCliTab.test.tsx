import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClientProvider } from "@tanstack/react-query";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { toast } from "sonner";
import * as cli from "../../../services/cli/cliManager";
import * as native from "../../../services/nativeCli";
import { copyText } from "../../../services/clipboard";
import { openDesktopPath, openDesktopUrl } from "../../../services/desktop/opener";
import { createTestQueryClient } from "../../../test/utils/reactQuery";
import { nativeTarget } from "../../../test/fixtures/native";
import { NativeCliTab } from "../tabs/NativeCliTab";
vi.mock("../../../services/cli/cliManager", async (original) => ({
  ...(await original<typeof cli>()),
  cliManagerNativeInfoGet: vi.fn(),
}));
vi.mock("../../../services/nativeCli", async (original) => ({
  ...(await original<typeof native>()),
  nativeCliTargetsList: vi.fn(),
}));
vi.mock("../../../services/clipboard", () => ({ copyText: vi.fn() }));
vi.mock("../../../services/desktop/opener", () => ({
  openDesktopPath: vi.fn(),
  openDesktopUrl: vi.fn(),
}));
vi.mock("sonner", () => ({ toast: { success: vi.fn(), error: vi.fn() } }));
vi.mock("../NativeCliUpdateCard", () => ({
  NativeCliUpdateCard: ({ client }: { client: string }) => <div>{client} 版本管理</div>,
}));
vi.mock("../omp/OmpSettingsPanel", () => ({ OmpSettingsPanel: () => <div>OMP 原生设置</div> }));
function setup(client: "pi" | "omp" = "pi") {
  const user = userEvent.setup();
  render(
    <QueryClientProvider client={createTestQueryClient()}>
      <MemoryRouter>
        <NativeCliTab client={client} />
      </MemoryRouter>
    </QueryClientProvider>
  );
  return user;
}
beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(cli.cliManagerNativeInfoGet).mockResolvedValue({
    found: true,
    executable_path: "C:/bin/pi.cmd",
    version: null,
    error: null,
    shell: "powershell",
    resolved_via: "PATH",
  });
  vi.mocked(native.nativeCliTargetsList).mockResolvedValue([nativeTarget()]);
  vi.mocked(copyText).mockResolvedValue(undefined);
  vi.mocked(openDesktopPath).mockResolvedValue(true);
  vi.mocked(openDesktopUrl).mockResolvedValue(true);
});
describe("native CLI manager", () => {
  it.each([
    ["pi", "0.87.1"],
    ["omp", "18.3.2"],
  ] as const)("displays the detected %s version", async (client, version) => {
    vi.mocked(cli.cliManagerNativeInfoGet).mockResolvedValue({
      found: true,
      executable_path: "C:/bin/" + client,
      version,
      error: null,
      shell: null,
      resolved_via: "path_scan",
    });
    setup(client);
    await screen.findByText("已安装 " + version);
    expect(screen.queryByText(/版本未知/)).not.toBeInTheDocument();
    expect(cli.cliManagerNativeInfoGet).toHaveBeenCalledWith(client);
  });

  it.each(["pi", "omp"] as const)(
    "probes %s without inventing a version and offers both management destinations",
    async (client) => {
      setup(client);
      await screen.findByText("已安装 · 版本未知");
      expect(cli.cliManagerNativeInfoGet).toHaveBeenCalledWith(client);
      expect(screen.getByRole("link", { name: "管理原生供应商" }).getAttribute("href")).toContain(
        "/providers?cli=" + client + "&view=native"
      );
      expect(screen.getByRole("link", { name: "管理 AIO 网关" }).getAttribute("href")).toContain(
        "/providers?cli=" + client + "&view=gateway"
      );
      expect(screen.getByText(/未能读取当前安装的版本/)).toBeInTheDocument();
    }
  );
  it("copies paths and opens the exact selected target directory", async () => {
    const user = setup();
    await screen.findByText("已安装 · 版本未知");
    await user.click(screen.getByRole("button", { name: "复制可执行路径" }));
    expect(copyText).toHaveBeenLastCalledWith("C:/bin/pi.cmd");
    await user.click(screen.getByRole("button", { name: "复制模型配置路径" }));
    expect(copyText).toHaveBeenLastCalledWith("C:/agent/models.json");
    await user.click(screen.getByRole("button", { name: "打开配置目录" }));
    expect(openDesktopPath).toHaveBeenCalledWith("C:/agent");
  });
  it("refreshes both installation and target state", async () => {
    const user = setup();
    await screen.findByText("已安装 · 版本未知");
    await user.click(screen.getByRole("button", { name: "刷新" }));
    await waitFor(() => expect(cli.cliManagerNativeInfoGet).toHaveBeenCalledTimes(2));
    expect(native.nativeCliTargetsList).toHaveBeenCalledTimes(2);
  });
  it("uses the same target picker for OMP profiles and follows the chosen target", async () => {
    vi.mocked(native.nativeCliTargetsList).mockResolvedValue([
      nativeTarget({ client: "omp", targetId: "omp:a" }),
      nativeTarget({
        client: "omp",
        targetId: "omp:work",
        selected: false,
        profile: "work",
        agentDir: "C:/omp/work",
        modelsPath: "C:/omp/work/models.yml",
        format: "yaml",
      }),
    ]);
    const user = setup("omp");
    await screen.findByText("已安装 · 版本未知");
    await user.click(screen.getByRole("button", { name: "切换目录 / Profile" }));
    await user.selectOptions(screen.getByLabelText("当前原生配置目标"), "omp:work");
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(screen.getByText("Profile work")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "打开配置目录" }));
    expect(openDesktopPath).toHaveBeenCalledWith("C:/omp/work");
    expect(screen.getByRole("link", { name: "管理原生供应商" }).getAttribute("href")).toContain(
      "target=omp%3Awork"
    );
  });
  it("does not claim installation state when the probe reports an error", async () => {
    vi.mocked(cli.cliManagerNativeInfoGet).mockResolvedValue({
      found: true,
      executable_path: "C:/stale/pi",
      version: "0.1",
      error: "SCAN_FAILED",
      shell: null,
      resolved_via: "unknown",
    });
    setup();
    await screen.findByText("无法确认安装状态，请重试。");
    expect(screen.queryByText(/已安装/)).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "复制可执行路径" })).toBeDisabled();
    expect(screen.queryByText("C:/stale/pi")).not.toBeInTheDocument();
  });
  it("disables target actions when a cached directory can no longer be confirmed", async () => {
    const user = setup();
    await screen.findByText("已安装 · 版本未知");
    vi.mocked(native.nativeCliTargetsList).mockRejectedValue(new Error("READ_FAILED"));
    await user.click(screen.getByRole("button", { name: "刷新" }));
    await screen.findByText("无法确认原生目录，请刷新重试。");
    expect(screen.getByRole("button", { name: "打开配置目录" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "复制模型配置路径" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "切换目录" })).toBeDisabled();
  });
  it("expands installation help when not installed and only copies commands", async () => {
    vi.mocked(cli.cliManagerNativeInfoGet).mockResolvedValue({
      found: false,
      executable_path: null,
      version: null,
      error: null,
      shell: null,
      resolved_via: "unknown",
    });
    const user = setup();
    await screen.findByText("未检测到");
    expect(screen.getByText("安装与更新").closest("details")).toHaveAttribute("open");
    expect(screen.getByText(/自动检查只提示，安装和升级须由你主动确认/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "复制npm · 安装或更新命令" }));
    expect(copyText).toHaveBeenCalledWith("npm install -g @earendil-works/pi-coding-agent@latest");
    expect(openDesktopPath).not.toHaveBeenCalled();
  });
  it("uses the correct official documentation and release links for OMP", async () => {
    const user = setup("omp");
    await screen.findByText("已安装 · 版本未知");
    await user.click(screen.getByRole("button", { name: "官方文档" }));
    expect(openDesktopUrl).toHaveBeenLastCalledWith("https://github.com/can1357/oh-my-pi");
    await user.click(screen.getByText("安装与更新"));
    await user.click(screen.getByRole("button", { name: "查看官方发布版本" }));
    expect(openDesktopUrl).toHaveBeenLastCalledWith("https://github.com/can1357/oh-my-pi/releases");
  });
  it("reports failed desktop and clipboard actions", async () => {
    vi.mocked(openDesktopPath).mockResolvedValue(false);
    vi.mocked(copyText).mockRejectedValue(new Error("CLIPBOARD_FAILED"));
    const user = setup();
    await screen.findByText("已安装 · 版本未知");
    await user.click(screen.getByRole("button", { name: "打开配置目录" }));
    expect(toast.error).toHaveBeenCalledWith("无法打开目录，请确认目录已存在");
    await user.click(screen.getByRole("button", { name: "复制可执行路径" }));
    expect(toast.error).toHaveBeenCalledWith("复制失败，请手动复制");
  });
});
