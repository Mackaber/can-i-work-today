import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { yaml } from "@codemirror/lang-yaml";
import { oneDark } from "@codemirror/theme-one-dark";
import { basicSetup, EditorView } from "codemirror";

function RegistryEditor() {
  const editorElement = useRef<HTMLDivElement>(null);
  const editor = useRef<EditorView | null>(null);
  const savingRef = useRef(false);
  const [initialDocument, setInitialDocument] = useState<string | null>(null);
  const [document, setDocument] = useState("");
  const [savedDocument, setSavedDocument] = useState("");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");
  const [loadAttempt, setLoadAttempt] = useState(0);

  useEffect(() => {
    let cancelled = false;
    void invoke<string>("get_custom_registry").then((contents) => {
      if (cancelled) return;
      setInitialDocument(contents);
      setDocument(contents);
      setSavedDocument(contents);
      setError("");
    }).catch((reason) => {
      if (!cancelled) setError(`Could not load custom registry: ${String(reason)}`);
    });
    return () => {
      cancelled = true;
    };
  }, [loadAttempt]);

  useEffect(() => {
    if (initialDocument === null || !editorElement.current) return;
    const view = new EditorView({
      doc: initialDocument,
      parent: editorElement.current,
      extensions: [
        basicSetup,
        yaml(),
        oneDark,
        EditorView.contentAttributes.of({ "aria-label": "Custom registry YAML" }),
        EditorView.updateListener.of((update) => {
          if (update.docChanged) setDocument(update.state.doc.toString());
        }),
      ],
    });
    editor.current = view;
    return () => {
      editor.current = null;
      view.destroy();
    };
  }, [initialDocument]);

  async function save() {
    if (!editor.current || savingRef.current) return;
    const contents = editor.current.state.doc.toString();
    savingRef.current = true;
    setSaving(true);
    try {
      await invoke("set_custom_registry", { contents });
      setSavedDocument(contents);
      setError("");
    } catch (reason) {
      setError(`Could not save custom registry: ${String(reason)}`);
    } finally {
      savingRef.current = false;
      setSaving(false);
    }
  }

  return (
    <main
      className="registry-window"
      onKeyDown={(event) => {
        if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "s") {
          event.preventDefault();
          void save();
        }
      }}
    >
      <header className="registry-header">
        <div>
          <p className="eyebrow">CAN I WORK TODAY</p>
          <h1>Custom registry</h1>
          <p>Edit the YAML list of services to add or override.</p>
        </div>
        <button
          type="button"
          className="registry-save"
          disabled={initialDocument === null || saving}
          onClick={() => void save()}
        >
          {saving ? "Saving…" : "Save"}
        </button>
      </header>
      <p className="registry-help">
        Each service needs id, name, category, feed_url, status_url, type (rss or json),
        and parsing_rules with items_path, status_path, and degraded_regex.
        Matching IDs replace built-in entries; new IDs are added.
      </p>
      {initialDocument === null ? (
        <div className="registry-loading">
          {error ? (
            <button type="button" className="settings-save" onClick={() => setLoadAttempt((attempt) => attempt + 1)}>
              Retry loading
            </button>
          ) : "Loading registry…"}
        </div>
      ) : (
        <div className="registry-code" ref={editorElement} />
      )}
      {error && <div className="error-banner" role="alert">{error}</div>}
      <footer className="registry-footer" aria-live="polite">
        <span>{initialDocument === null ? "" : document === savedDocument ? "Saved" : "Unsaved changes"}</span>
        <span>Save with ⌘S / Ctrl+S</span>
      </footer>
    </main>
  );
}

export default RegistryEditor;
