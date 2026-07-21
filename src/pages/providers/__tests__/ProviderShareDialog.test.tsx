import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ProviderSummary } from "../../../services/providers/providers";
import { ProviderShareDialog } from "../ProviderShareDialog";

const copyShare = vi.hoisted(() => vi.fn());
const saveShare = vi.hoisted(() => vi.fn());
const toastSuccess = vi.hoisted(() => vi.fn());
const toastError = vi.hoisted(() => vi.fn());

vi.mock("../../../services/providers/providerShare", () => ({
  providerShareCopyToClipboard: copyShare,
  providerShareSaveToFile: saveShare,
}));

vi.mock("sonner", () => ({
  toast: { success: toastSuccess, error: toastError },
}));

const provider = {
  id: 7,
  provider_uuid: "11111111-1111-4111-8111-111111111111",
  cli_key: "claude",
  name: "Share Me",
  source_provider_id: null,
} as ProviderSummary;

describe("pages/providers/ProviderShareDialog", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("warns about credentials and copies through the backend-owned action", async () => {
    copyShare.mockResolvedValueOnce(true);
    const onOpenChange = vi.fn();
    render(<ProviderShareDialog open provider={provider} onOpenChange={onOpenChange} />);

    expect(screen.getByText(/包含完整 API Key、OAuth Token 和 Client Secret/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "复制内容" }));

    await waitFor(() => expect(copyShare).toHaveBeenCalledWith(7));
    expect(toastSuccess).toHaveBeenCalledWith(
      "供应商分享内容已复制，60 秒后仅在内容未变化时自动清空"
    );
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("keeps the warning dialog open when the native save dialog is cancelled", async () => {
    saveShare.mockResolvedValueOnce(false);
    const onOpenChange = vi.fn();
    render(<ProviderShareDialog open provider={provider} onOpenChange={onOpenChange} />);

    fireEvent.click(screen.getByRole("button", { name: "保存到本地" }));

    await waitFor(() => expect(saveShare).toHaveBeenCalledWith(7));
    expect(onOpenChange).not.toHaveBeenCalledWith(false);
    expect(toastSuccess).not.toHaveBeenCalled();
  });
});
