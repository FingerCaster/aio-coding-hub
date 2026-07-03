import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  resetAppStartupStatusStore,
  setAppStartupStatusSnapshot,
} from "../../../app/startupStatusStore";
import { appStartupRetry } from "../../../services/app/startupStatus";
import { logToConsole } from "../../../services/consoleLog";
import { AppStartupStatusBanner } from "../AppStartupStatusBanner";

const navigate = vi.fn();

vi.mock("react-router-dom", () => ({
  useNavigate: () => navigate,
}));

vi.mock("sonner", () => ({ toast: vi.fn() }));
vi.mock("../../../services/consoleLog", () => ({ logToConsole: vi.fn() }));
vi.mock("../../../services/app/startupStatus", async () => {
  const actual = await vi.importActual<typeof import("../../../services/app/startupStatus")>(
    "../../../services/app/startupStatus"
  );
  return {
    ...actual,
    appStartupRetry: vi.fn(),
  };
});

function setFailedStatus(partial: Record<string, unknown> = {}) {
  setAppStartupStatusSnapshot({
    running: false,
    currentStage: "failed",
    failedStage: "starting_gateway",
    errorMessage: "gateway boom",
    canRetry: true,
    ...partial,
  } as any);
}

describe("components/app/AppStartupStatusBanner", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    resetAppStartupStatusStore();
  });

  afterEach(() => {
    resetAppStartupStatusStore();
  });

  it("stays hidden unless startup has failed", () => {
    const { rerender } = render(<AppStartupStatusBanner />);
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();

    setAppStartupStatusSnapshot({
      running: true,
      currentStage: "starting_gateway",
      failedStage: null,
      errorMessage: null,
      canRetry: false,
    } as any);
    rerender(<AppStartupStatusBanner />);

    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("renders failed stage details and navigates to settings", () => {
    setFailedStatus();

    render(<AppStartupStatusBanner />);

    expect(screen.getByRole("alert")).toHaveTextContent("网关启动失败：gateway boom");

    fireEvent.click(screen.getByRole("button", { name: "打开设置" }));
    expect(navigate).toHaveBeenCalledWith("/settings");
  });

  it("uses fallback stage labels and disables retry when retry is not allowed", () => {
    setFailedStatus({
      failedStage: null,
      errorMessage: null,
      canRetry: false,
    });

    render(<AppStartupStatusBanner />);

    expect(screen.getByRole("alert")).toHaveTextContent("应用启动失败：应用启动失败");
    expect(screen.getByRole("button", { name: "重试启动" })).toBeDisabled();
  });

  it("updates the startup snapshot after retry succeeds", async () => {
    setFailedStatus({ failedStage: "reading_settings" });
    vi.mocked(appStartupRetry).mockResolvedValue({
      running: false,
      currentStage: "ready",
      failedStage: null,
      errorMessage: null,
      canRetry: false,
    } as any);

    render(<AppStartupStatusBanner />);

    fireEvent.click(screen.getByRole("button", { name: "重试启动" }));
    expect(screen.getByRole("button", { name: "重试中..." })).toBeDisabled();

    await waitFor(() => expect(appStartupRetry).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(screen.queryByRole("alert")).not.toBeInTheDocument());
  });

  it("logs and restores retry state when retry fails", async () => {
    setFailedStatus({ failedStage: "finalizing_wsl", errorMessage: null });
    vi.mocked(appStartupRetry).mockRejectedValue(new Error("retry boom"));

    render(<AppStartupStatusBanner />);

    fireEvent.click(screen.getByRole("button", { name: "重试启动" }));

    await waitFor(() =>
      expect(logToConsole).toHaveBeenCalledWith(
        "error",
        "重试启动任务失败",
        expect.objectContaining({ failed_stage: "finalizing_wsl", error: "Error: retry boom" })
      )
    );
    expect(screen.getByRole("button", { name: "重试启动" })).toBeEnabled();
    expect(screen.getByRole("alert")).toHaveTextContent("WSL 启动收尾失败：WSL 启动收尾失败");
  });
});
