import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { SettingsAboutCard } from "../SettingsAboutCard";

describe("pages/settings/SettingsAboutCard", () => {
  it("renders placeholder when about is null", () => {
    render(<SettingsAboutCard about={null} checkingUpdate={false} checkUpdate={vi.fn()} />);
    expect(screen.getByText("关于应用")).toBeInTheDocument();
    expect(screen.getByText("加载中…")).toBeInTheDocument();
  });

  it("renders about information when available", () => {
    const checkUpdate = vi.fn().mockResolvedValue(undefined);

    render(
      <SettingsAboutCard
        about={{
          os: "mac",
          arch: "arm64",
          profile: "dev",
          app_version: "0.0.0",
          bundle_type: null,
          run_mode: "desktop",
        }}
        checkingUpdate={false}
        checkUpdate={checkUpdate}
      />
    );

    expect(screen.getByText("版本")).toBeInTheDocument();
    expect(screen.getByText("0.0.0")).toBeInTheDocument();
    expect(screen.getByText("平台")).toBeInTheDocument();
    expect(screen.getByText("mac/arm64")).toBeInTheDocument();
    expect(screen.queryByText("Bundle")).not.toBeInTheDocument();
    expect(screen.getByText("运行模式")).toBeInTheDocument();
    expect(screen.getByText("desktop")).toBeInTheDocument();
    expect(screen.getByText("检查更新")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "检查" }));
    expect(checkUpdate).toHaveBeenCalledTimes(1);
  });

  it("hides unknown run_mode and shows bundle when known", () => {
    render(
      <SettingsAboutCard
        about={{
          os: "mac",
          arch: "arm64",
          profile: "release",
          app_version: "0.0.0",
          bundle_type: "app",
          run_mode: "unknown",
        }}
        checkingUpdate={false}
        checkUpdate={vi.fn()}
      />
    );

    expect(screen.getByText("Bundle")).toBeInTheDocument();
    expect(screen.getByText("app")).toBeInTheDocument();
    expect(screen.queryByText("运行模式")).not.toBeInTheDocument();
    expect(screen.queryByText("unknown")).not.toBeInTheDocument();
    expect(screen.getByText("检查更新")).toBeInTheDocument();
  });

  it("renders portable action and checking state", () => {
    const checkUpdate = vi.fn().mockResolvedValue(undefined);
    const view = render(
      <SettingsAboutCard
        about={{
          os: "mac",
          arch: "arm64",
          profile: "dev",
          app_version: "0.0.0",
          bundle_type: null,
          run_mode: "portable",
        }}
        checkingUpdate={false}
        checkUpdate={checkUpdate}
      />
    );

    expect(screen.getByText("获取新版本")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "打开" }));
    expect(checkUpdate).toHaveBeenCalledTimes(1);

    view.rerender(
      <SettingsAboutCard
        about={{
          os: "mac",
          arch: "arm64",
          profile: "dev",
          app_version: "0.0.0",
          bundle_type: null,
          run_mode: "desktop",
        }}
        checkingUpdate
        checkUpdate={checkUpdate}
      />
    );

    expect(screen.getByRole("button", { name: "检查中…" })).toBeDisabled();
  });

  it("keeps stable selected when first-time Beta confirmation is cancelled", () => {
    const setUpdateChannel = vi.fn().mockResolvedValue(true);
    render(
      <SettingsAboutCard
        about={{
          os: "windows",
          arch: "x86_64",
          profile: "release",
          app_version: "1.0.0",
          bundle_type: "msi",
          run_mode: "desktop",
        }}
        checkingUpdate={false}
        checkUpdate={vi.fn()}
        setUpdateChannel={setUpdateChannel}
      />
    );

    const participation = screen.getByRole("switch", { name: "参与 Beta 测试" });
    expect(participation).toHaveAttribute("data-state", "unchecked");
    fireEvent.click(participation);

    expect(screen.getByRole("dialog", { name: "参与 Beta 测试" })).toBeInTheDocument();
    expect(setUpdateChannel).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "取消" }));

    expect(screen.queryByRole("dialog", { name: "参与 Beta 测试" })).not.toBeInTheDocument();
    expect(participation).toHaveAttribute("data-state", "unchecked");
  });

  it("changes channel only after confirmation and a successful backend save", async () => {
    const setUpdateChannel = vi.fn().mockResolvedValue(true);
    render(
      <SettingsAboutCard
        about={{
          os: "windows",
          arch: "x86_64",
          profile: "release",
          app_version: "1.0.0",
          bundle_type: "msi",
          run_mode: "desktop",
        }}
        checkingUpdate={false}
        checkUpdate={vi.fn()}
        setUpdateChannel={setUpdateChannel}
      />
    );

    const participation = screen.getByRole("switch", { name: "参与 Beta 测试" });
    fireEvent.click(participation);
    fireEvent.click(screen.getByRole("button", { name: "确认参与 Beta 测试" }));

    await waitFor(() => expect(setUpdateChannel).toHaveBeenCalledWith("beta"));
    await waitFor(() =>
      expect(screen.queryByRole("dialog", { name: "参与 Beta 测试" })).not.toBeInTheDocument()
    );
    expect(participation).toHaveAttribute("data-state", "unchecked");
  });

  it("keeps stable and exposes an error when the Beta save fails", async () => {
    const setUpdateChannel = vi.fn().mockResolvedValue(false);
    render(
      <SettingsAboutCard
        about={{
          os: "windows",
          arch: "x86_64",
          profile: "release",
          app_version: "1.0.0",
          bundle_type: "msi",
          run_mode: "desktop",
        }}
        checkingUpdate={false}
        checkUpdate={vi.fn()}
        setUpdateChannel={setUpdateChannel}
      />
    );

    const participation = screen.getByRole("switch", { name: "参与 Beta 测试" });
    fireEvent.click(participation);
    fireEvent.click(screen.getByRole("button", { name: "确认参与 Beta 测试" }));

    await waitFor(() => expect(screen.getAllByRole("alert").length).toBeGreaterThan(0));
    expect(screen.getAllByText("未能保存更新频道，当前频道未改变。").length).toBeGreaterThan(0);
    expect(participation).toHaveAttribute("data-state", "unchecked");
    expect(screen.getByRole("dialog", { name: "参与 Beta 测试" })).toBeInTheDocument();
  });

  it("renders the exact Beta candidate version and exits to stable without confirmation", async () => {
    const setUpdateChannel = vi.fn().mockResolvedValue(true);
    render(
      <SettingsAboutCard
        about={{
          os: "windows",
          arch: "x86_64",
          profile: "release",
          app_version: "1.0.0",
          bundle_type: "msi",
          run_mode: "desktop",
        }}
        checkingUpdate={false}
        checkUpdate={vi.fn()}
        updateChannel="beta"
        betaParticipationConfirmed
        updateCandidate={{
          rid: 4,
          channel: "beta",
          isPrerelease: true,
          version: "1.1.0-beta.3",
          currentVersion: "1.0.0",
          date: null,
          body: null,
          releaseUrl:
            "https://github.com/FingerCaster/aio-coding-hub/releases/tag/aio-coding-hub-v1.1.0-beta.3",
          generation: 2,
        }}
        setUpdateChannel={setUpdateChannel}
      />
    );

    expect(screen.getByLabelText("Beta 更新 1.1.0-beta.3")).toBeInTheDocument();
    expect(screen.getByText("1.1.0-beta.3")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("switch", { name: "参与 Beta 测试" }));

    await waitFor(() => expect(setUpdateChannel).toHaveBeenCalledWith("stable"));
    expect(screen.queryByRole("dialog", { name: "参与 Beta 测试" })).not.toBeInTheDocument();
  });

  it("labels a final release from the Beta subscription as a normal update", () => {
    render(
      <SettingsAboutCard
        about={{
          os: "windows",
          arch: "x86_64",
          profile: "release",
          app_version: "1.1.0-beta.3",
          bundle_type: "msi",
          run_mode: "desktop",
        }}
        checkingUpdate={false}
        checkUpdate={vi.fn()}
        updateChannel="beta"
        betaParticipationConfirmed
        updateCandidate={{
          rid: 5,
          channel: "beta",
          isPrerelease: false,
          version: "1.1.0",
          currentVersion: "1.1.0-beta.3",
          date: null,
          body: null,
          releaseUrl:
            "https://github.com/FingerCaster/aio-coding-hub/releases/tag/aio-coding-hub-v1.1.0",
          generation: 2,
        }}
      />
    );

    expect(screen.getByLabelText("可用更新 1.1.0")).toBeInTheDocument();
    expect(screen.queryByLabelText("Beta 更新 1.1.0")).not.toBeInTheDocument();
  });

  it("does not repeat the risk dialog after participation was confirmed", async () => {
    const setUpdateChannel = vi.fn().mockResolvedValue(true);
    render(
      <SettingsAboutCard
        about={{
          os: "windows",
          arch: "x86_64",
          profile: "release",
          app_version: "1.0.0",
          bundle_type: "msi",
          run_mode: "desktop",
        }}
        checkingUpdate={false}
        checkUpdate={vi.fn()}
        betaParticipationConfirmed
        setUpdateChannel={setUpdateChannel}
      />
    );

    fireEvent.click(screen.getByRole("switch", { name: "参与 Beta 测试" }));
    await waitFor(() => expect(setUpdateChannel).toHaveBeenCalledWith("beta"));
    expect(screen.queryByRole("dialog", { name: "参与 Beta 测试" })).not.toBeInTheDocument();
  });
});
