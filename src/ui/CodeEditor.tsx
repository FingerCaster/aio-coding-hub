import { useEffect, useRef } from "react";
import { cn } from "../utils/cn";
import { BRAND } from "../constants/colors";

export type CodeEditorLanguage = "toml" | "text";

export type CodeEditorProps = {
  value: string;
  onChange?: (value: string) => void;
  language?: CodeEditorLanguage;
  placeholder?: string;
  readOnly?: boolean;
  minHeight?: string;
  height?: string | number;
  className?: string;
};

type EditorViewInstance = import("@codemirror/view").EditorView;

type CodeMirrorBundle = {
  EditorView: typeof import("codemirror").EditorView;
  basicSetup: typeof import("codemirror").basicSetup;
  EditorState: typeof import("@codemirror/state").EditorState;
  placeholderExt: typeof import("@codemirror/view").placeholder;
  StreamLanguage: typeof import("@codemirror/language").StreamLanguage;
  tomlMode: typeof import("@codemirror/legacy-modes/mode/toml").toml;
};

let codeMirrorBundlePromise: Promise<CodeMirrorBundle> | null = null;

function loadCodeMirrorBundle() {
  codeMirrorBundlePromise ??= Promise.all([
    import("codemirror"),
    import("@codemirror/state"),
    import("@codemirror/view"),
    import("@codemirror/language"),
    import("@codemirror/legacy-modes/mode/toml"),
  ]).then(([codemirror, state, view, language, toml]) => ({
    EditorView: codemirror.EditorView,
    basicSetup: codemirror.basicSetup,
    EditorState: state.EditorState,
    placeholderExt: view.placeholder,
    StreamLanguage: language.StreamLanguage,
    tomlMode: toml.toml,
  }));

  return codeMirrorBundlePromise;
}

export function CodeEditor({
  value,
  onChange,
  language = "text",
  placeholder,
  readOnly = false,
  minHeight = "280px",
  height,
  className,
}: CodeEditorProps) {
  const editorRef = useRef<HTMLDivElement>(null);
  const viewRef = useRef<EditorViewInstance | null>(null);
  const valueRef = useRef(value);
  valueRef.current = value;

  const onChangeRef = useRef(onChange);
  onChangeRef.current = onChange;

  useEffect(() => {
    const parent = editorRef.current;
    if (!parent) return;
    viewRef.current = null;
    let cancelled = false;
    let view: EditorViewInstance | null = null;

    void loadCodeMirrorBundle().then(
      ({ EditorView, basicSetup, EditorState, placeholderExt, StreamLanguage, tomlMode }) => {
        if (cancelled || !editorRef.current) return;

        const heightValue = height
          ? typeof height === "number"
            ? `${height}px`
            : height
          : undefined;

        const baseTheme = EditorView.baseTheme({
          ".cm-editor": {
            border: "1px solid rgb(226 232 240)",
            borderRadius: "0.5rem",
            background: "transparent",
          },
          ".cm-editor.cm-focused": {
            outline: "none",
            borderColor: BRAND.accent,
          },
          ".cm-scroller": {
            background: "transparent",
          },
          ".cm-gutters": {
            background: "transparent",
            borderRight: "1px solid rgb(226 232 240)",
            color: "rgb(100 116 139)",
          },
          ".cm-selectionBackground, .cm-content ::selection": {
            background: "rgba(0, 82, 255, 0.18)",
          },
          ".cm-activeLine": {
            background: "rgba(0, 82, 255, 0.06)",
          },
          ".cm-activeLineGutter": {
            background: "rgba(0, 82, 255, 0.06)",
          },
        });

        const sizingTheme = EditorView.theme({
          "&": heightValue ? { height: heightValue } : { minHeight },
          ".cm-scroller": { overflow: "auto" },
          ".cm-content": {
            fontFamily:
              "ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, 'Liberation Mono', 'Courier New', monospace",
            fontSize: "13px",
          },
        });

        const languageExtension = language === "toml" ? StreamLanguage.define(tomlMode) : [];

        const extensions = [
          basicSetup,
          baseTheme,
          sizingTheme,
          languageExtension,
          EditorState.readOnly.of(readOnly),
          placeholder && !readOnly ? placeholderExt(placeholder) : [],
          !readOnly
            ? EditorView.updateListener.of((update) => {
                if (!update.docChanged) return;
                onChangeRef.current?.(update.state.doc.toString());
              })
            : [],
          readOnly
            ? EditorView.theme({
                ".cm-cursor, .cm-dropCursor": { border: "none" },
                ".cm-activeLine": { background: "transparent !important" },
                ".cm-activeLineGutter": { background: "transparent !important" },
              })
            : [],
        ];

        const state = EditorState.create({
          doc: valueRef.current,
          extensions,
        });

        view = new EditorView({
          state,
          parent,
        });

        viewRef.current = view;
      }
    );

    return () => {
      cancelled = true;
      view?.destroy();
    };
  }, [language, readOnly, minHeight, height, placeholder]);

  useEffect(() => {
    const view = viewRef.current;
    if (!view) return;
    if (view.state.doc.toString() === value) return;
    view.dispatch({
      changes: {
        from: 0,
        to: view.state.doc.length,
        insert: value,
      },
    });
  }, [value]);

  return <div ref={editorRef} className={cn("w-full", className)} />;
}
