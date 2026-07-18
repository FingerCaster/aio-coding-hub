import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ProviderShareImportPreview } from "../../../services/providers/providerShare";
import { ProviderImportDialog } from "../ProviderImportDialog";

const previewFromContent = vi.hoisted(() => vi.fn());
const previewFromFile = vi.hoisted(() => vi.fn());
const discardPreview = vi.hoisted(() => vi.fn());
const importMutateAsync = vi.hoisted(() => vi.fn());

vi.mock("../../../services/providers/providerShare", () => ({
  providerShareImportPreviewFromContent: previewFromContent,
  providerShareImportPreviewFromFile: previewFromFile,
  providerShareImportPreviewDiscard: discardPreview,
}));

vi.mock("../../../query/providerShare", () => ({
  useProviderShareImportMutation: () => ({
    mutateAsync: importMutateAsync,
    isPending: false,
  }),
}));

vi.mock("sonner", () => ({
  toast: { success: vi.fn(), error: vi.fn() },
}));

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((resolvePromise) => {
    resolve = resolvePromise;
  });
  return { promise, resolve };
}

function preview(tokenCharacter: string): ProviderShareImportPreview {
  return {
    previewToken: tokenCharacter.repeat(64),
    cliKey: "claude",
    sourceName: "Source",
    finalName: "Source 副本",
    sourceEnabled: true,
    importEnabled: false,
    authMode: "api_key",
    credentialStatus: "configured",
    extensionCount: 0,
    extensions: [],
    canImport: true,
  };
}

function renderDialog(open = true) {
  const onOpenChange = vi.fn();
  const onImported = vi.fn();
  const result = render(
    <ProviderImportDialog open={open} onOpenChange={onOpenChange} onImported={onImported} />
  );
  return { ...result, onOpenChange, onImported };
}

function startContentPreview(content = "initial content") {
  fireEvent.click(screen.getByRole("button", { name: "内容" }));
  fireEvent.change(screen.getByRole("textbox", { name: "供应商分享 JSON 内容" }), {
    target: { value: content },
  });
  fireEvent.click(screen.getByRole("button", { name: "校验并预览" }));
}

describe("pages/providers/ProviderImportDialog", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    discardPreview.mockResolvedValue(true);
  });

  it("discards a deferred preview that arrives after pasted content changes", async () => {
    const request = deferred<ProviderShareImportPreview>();
    previewFromContent.mockReturnValueOnce(request.promise);
    renderDialog();

    startContentPreview();
    expect(screen.getByRole("textbox", { name: "供应商分享 JSON 内容" })).toBeEnabled();
    fireEvent.change(screen.getByRole("textbox", { name: "供应商分享 JSON 内容" }), {
      target: { value: "changed while previewing" },
    });

    await act(async () => request.resolve(preview("a")));

    await waitFor(() => expect(discardPreview).toHaveBeenCalledWith("a".repeat(64)));
    expect(screen.queryByText("Source 副本")).not.toBeInTheDocument();
  });

  it("discards a deferred preview after the parent closes the dialog", async () => {
    const request = deferred<ProviderShareImportPreview>();
    previewFromContent.mockReturnValueOnce(request.promise);
    const { rerender, onOpenChange, onImported } = renderDialog();

    startContentPreview();
    rerender(
      <ProviderImportDialog open={false} onOpenChange={onOpenChange} onImported={onImported} />
    );
    await act(async () => request.resolve(preview("b")));

    await waitFor(() => expect(discardPreview).toHaveBeenCalledWith("b".repeat(64)));
  });

  it("discards a deferred preview after unmount", async () => {
    const request = deferred<ProviderShareImportPreview>();
    previewFromContent.mockReturnValueOnce(request.promise);
    const { unmount } = renderDialog();

    startContentPreview();
    unmount();
    await act(async () => request.resolve(preview("c")));

    await waitFor(() => expect(discardPreview).toHaveBeenCalledWith("c".repeat(64)));
  });

  it("renders a credential-free preview and imports the selected token", async () => {
    const nextPreview = preview("d");
    previewFromContent.mockResolvedValueOnce(nextPreview);
    importMutateAsync.mockResolvedValueOnce({
      id: 9,
      cli_key: "claude",
      name: "Source 副本",
      enabled: false,
    });
    const { onImported } = renderDialog();

    startContentPreview();
    expect(await screen.findByText("Source 副本")).toBeInTheDocument();
    expect(screen.getByText("API Key 已配置")).toBeInTheDocument();
    expect(screen.queryByText(/synthetic-key/i)).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "确认导入" }));
    await waitFor(() =>
      expect(importMutateAsync).toHaveBeenCalledWith({ previewToken: "d".repeat(64) })
    );
    expect(onImported).toHaveBeenCalledWith(expect.objectContaining({ id: 9, enabled: false }));
  });
});
