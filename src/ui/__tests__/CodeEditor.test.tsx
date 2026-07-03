import { cleanup, render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

/* ------------------------------------------------------------------ */
/*  Mock setup - all codemirror modules                               */
/*  vi.hoisted ensures these are available when vi.mock factories run */
/* ------------------------------------------------------------------ */

const {
  destroyMock,
  dispatchMock,
  MockEditorView,
  MockEditorState,
  mockPlaceholder,
  mockStreamLanguageDefine,
  mockTomlMode,
} = vi.hoisted(() => {
  const destroyMock = vi.fn();
  const dispatchMock = vi.fn();

  class MockEditorView {
    state: any;
    constructor(config: any) {
      this.state = config.state;
    }
    destroy = destroyMock;
    dispatch = dispatchMock;
    static baseTheme = vi.fn(() => "baseTheme");
    static theme = vi.fn(() => "theme");
    static updateListener = { of: vi.fn(() => "updateListener") };
  }

  const MockEditorState = {
    create: vi.fn((config: any) => ({
      doc: { toString: () => config.doc, length: config.doc.length },
    })),
    readOnly: { of: vi.fn(() => "readOnly") },
  };

  const mockPlaceholder = vi.fn(() => "placeholderExt");
  const mockStreamLanguageDefine = vi.fn(() => "tomlLanguage");
  const mockTomlMode = { name: "toml" };

  return {
    destroyMock,
    dispatchMock,
    MockEditorView,
    MockEditorState,
    mockPlaceholder,
    mockStreamLanguageDefine,
    mockTomlMode,
  };
});

vi.mock("codemirror", () => ({
  EditorView: MockEditorView,
  basicSetup: "basicSetup",
}));

vi.mock("@codemirror/state", () => ({
  EditorState: MockEditorState,
}));

vi.mock("@codemirror/view", () => ({
  placeholder: mockPlaceholder,
}));

vi.mock("@codemirror/language", () => ({
  StreamLanguage: { define: mockStreamLanguageDefine },
}));

vi.mock("@codemirror/legacy-modes/mode/toml", () => ({
  toml: mockTomlMode,
}));

/* ------------------------------------------------------------------ */
/*  Import the component under test AFTER mocks are declared          */
/* ------------------------------------------------------------------ */

import { CodeEditor } from "../CodeEditor";

/* ------------------------------------------------------------------ */
/*  Tests                                                             */
/* ------------------------------------------------------------------ */

async function waitForEditorCreated(times = 1) {
  await waitFor(() => expect(MockEditorState.create).toHaveBeenCalledTimes(times));
}

describe("ui/CodeEditor", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(() => {
    cleanup();
  });

  /* --- Rendering -------------------------------------------------- */

  it("renders a container div with correct className", () => {
    const { container } = render(<CodeEditor value="hello" />);
    const wrapper = container.firstChild as HTMLElement;
    expect(wrapper).toBeInstanceOf(HTMLDivElement);
    expect(wrapper.className).toContain("w-full");
  });

  it("applies custom className", () => {
    const { container } = render(<CodeEditor value="" className="my-custom" />);
    const wrapper = container.firstChild as HTMLElement;
    expect(wrapper.className).toContain("w-full");
    expect(wrapper.className).toContain("my-custom");
  });

  /* --- EditorView lifecycle --------------------------------------- */

  it("creates EditorView on mount and destroys on unmount", async () => {
    const { unmount } = render(<CodeEditor value="test" />);

    await waitForEditorCreated();
    expect(MockEditorState.create).toHaveBeenCalledTimes(1);
    expect(MockEditorState.create).toHaveBeenCalledWith(expect.objectContaining({ doc: "test" }));

    expect(destroyMock).not.toHaveBeenCalled();
    unmount();
    expect(destroyMock).toHaveBeenCalledTimes(1);
  });

  /* --- Language extensions ---------------------------------------- */

  it("uses toml language extension when language='toml'", async () => {
    render(<CodeEditor value="" language="toml" />);
    await waitForEditorCreated();
    expect(mockStreamLanguageDefine).toHaveBeenCalledWith(mockTomlMode);
  });

  it("uses empty array (no language) when language='text' (default)", async () => {
    render(<CodeEditor value="" />);
    await waitForEditorCreated();
    expect(mockStreamLanguageDefine).not.toHaveBeenCalled();
  });

  /* --- Placeholder ------------------------------------------------ */

  it("applies placeholder extension when placeholder is provided and not readOnly", async () => {
    render(<CodeEditor value="" placeholder="Type here..." />);
    await waitForEditorCreated();
    expect(mockPlaceholder).toHaveBeenCalledWith("Type here...");
  });

  it("does NOT apply placeholder when readOnly", async () => {
    render(<CodeEditor value="" placeholder="Type here..." readOnly />);
    await waitForEditorCreated();
    expect(mockPlaceholder).not.toHaveBeenCalled();
  });

  /* --- ReadOnly behaviour ----------------------------------------- */

  it("applies readOnly cursor theme when readOnly=true", async () => {
    render(<CodeEditor value="" readOnly />);
    await waitForEditorCreated();

    // readOnly=true triggers EditorView.theme for cursor hiding.
    // EditorView.theme is called for sizing theme + readOnly theme = at least 2 calls.
    const themeCalls = MockEditorView.theme.mock.calls;
    const hasReadOnlyCursorTheme = themeCalls.some(
      (call: any) => call[0][".cm-cursor, .cm-dropCursor"] !== undefined
    );
    expect(hasReadOnlyCursorTheme).toBe(true);
  });

  it("registers updateListener when not readOnly", async () => {
    render(<CodeEditor value="" />);
    await waitForEditorCreated();
    expect(MockEditorView.updateListener.of).toHaveBeenCalledTimes(1);
    expect(MockEditorView.updateListener.of).toHaveBeenCalledWith(expect.any(Function));
  });

  it("does NOT register updateListener when readOnly", async () => {
    render(<CodeEditor value="" readOnly />);
    await waitForEditorCreated();
    expect(MockEditorView.updateListener.of).not.toHaveBeenCalled();
  });

  /* --- readOnly.of ------------------------------------------------ */

  it("calls EditorState.readOnly.of with the readOnly prop value", async () => {
    const { unmount } = render(<CodeEditor value="" readOnly />);
    await waitForEditorCreated();
    expect(MockEditorState.readOnly.of).toHaveBeenCalledWith(true);
    unmount();

    vi.clearAllMocks();
    render(<CodeEditor value="" />);
    await waitForEditorCreated();
    expect(MockEditorState.readOnly.of).toHaveBeenCalledWith(false);
  });

  /* --- Value sync (second useEffect) ------------------------------ */

  it("dispatches value changes when value prop updates", async () => {
    const { rerender } = render(<CodeEditor value="initial" />);
    await waitForEditorCreated();

    // After first render, dispatch should not have been called
    // (value matches what was passed to EditorState.create)
    expect(dispatchMock).not.toHaveBeenCalled();

    // Rerender with new value
    rerender(<CodeEditor value="updated" />);

    await waitFor(() => expect(dispatchMock).toHaveBeenCalledTimes(1));
    expect(dispatchMock).toHaveBeenCalledWith({
      changes: {
        from: 0,
        to: "initial".length,
        insert: "updated",
      },
    });
  });

  it("skips dispatch when value matches current doc", async () => {
    const { rerender } = render(<CodeEditor value="same" />);
    await waitForEditorCreated();
    expect(dispatchMock).not.toHaveBeenCalled();

    // Rerender with the same value
    rerender(<CodeEditor value="same" />);
    expect(dispatchMock).not.toHaveBeenCalled();
  });

  /* --- Height prop ------------------------------------------------ */

  it("converts numeric height to px string", async () => {
    render(<CodeEditor value="" height={500} />);
    await waitForEditorCreated();
    const themeCalls = MockEditorView.theme.mock.calls;
    const sizingCall = themeCalls.find((call: any) => call[0]["&"]?.height === "500px");
    expect(sizingCall).toBeDefined();
  });

  it("uses string height as-is", async () => {
    render(<CodeEditor value="" height="80vh" />);
    await waitForEditorCreated();
    const themeCalls = MockEditorView.theme.mock.calls;
    const sizingCall = themeCalls.find((call: any) => call[0]["&"]?.height === "80vh");
    expect(sizingCall).toBeDefined();
  });

  it("falls back to minHeight when height is undefined", async () => {
    render(<CodeEditor value="" minHeight="400px" />);
    await waitForEditorCreated();
    const themeCalls = MockEditorView.theme.mock.calls;
    const sizingCall = themeCalls.find((call: any) => call[0]["&"]?.minHeight === "400px");
    expect(sizingCall).toBeDefined();
  });

  it("uses default minHeight of 280px when neither height nor minHeight specified", async () => {
    render(<CodeEditor value="" />);
    await waitForEditorCreated();
    const themeCalls = MockEditorView.theme.mock.calls;
    const sizingCall = themeCalls.find((call: any) => call[0]["&"]?.minHeight === "280px");
    expect(sizingCall).toBeDefined();
  });

  /* --- onChange callback via updateListener ----------------------- */

  it("updateListener callback invokes onChange when doc changes", async () => {
    const onChange = vi.fn();
    render(<CodeEditor value="" onChange={onChange} />);
    await waitForEditorCreated();

    // Get the listener callback passed to updateListener.of
    const calls = (MockEditorView.updateListener.of as any).mock.calls;
    const listenerCallback = calls[0][0] as (update: any) => void;

    // Simulate a doc-changed update
    listenerCallback({
      docChanged: true,
      state: { doc: { toString: () => "new content" } },
    });
    expect(onChange).toHaveBeenCalledWith("new content");
  });

  it("updateListener callback does NOT invoke onChange when doc did not change", async () => {
    const onChange = vi.fn();
    render(<CodeEditor value="" onChange={onChange} />);
    await waitForEditorCreated();

    const calls = (MockEditorView.updateListener.of as any).mock.calls;
    const listenerCallback = calls[0][0] as (update: any) => void;

    listenerCallback({
      docChanged: false,
      state: { doc: { toString: () => "" } },
    });
    expect(onChange).not.toHaveBeenCalled();
  });

  /* --- Editor recreation on dependency changes -------------------- */

  it("recreates EditorView when language prop changes", async () => {
    const { rerender } = render(<CodeEditor value="" language="text" />);
    await waitForEditorCreated();
    expect(MockEditorState.create).toHaveBeenCalledTimes(1);
    expect(destroyMock).not.toHaveBeenCalled();

    rerender(<CodeEditor value="" language="toml" />);
    // old view destroyed, new one created
    expect(destroyMock).toHaveBeenCalledTimes(1);
    await waitForEditorCreated(2);
    expect(MockEditorState.create).toHaveBeenCalledTimes(2);
  });

  it("recreates EditorView when readOnly prop changes", async () => {
    const { rerender } = render(<CodeEditor value="" />);
    await waitForEditorCreated();
    expect(MockEditorState.create).toHaveBeenCalledTimes(1);

    rerender(<CodeEditor value="" readOnly />);
    expect(destroyMock).toHaveBeenCalledTimes(1);
    await waitForEditorCreated(2);
    expect(MockEditorState.create).toHaveBeenCalledTimes(2);
  });
});
