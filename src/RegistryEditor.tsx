import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { json } from "@codemirror/lang-json";
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
    void invoke<unknown>("get_custom_registry").then((services) => {
      if (cancelled) return;
      const contents = JSON.stringify(services, null, 2);
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
        json(),
        oneDark,
        EditorView.contentAttributes.of({ "aria-label": "Custom registry JSON" }),
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
    let services: unknown;
    try {
      services = JSON.parse(contents);
      if (!Array.isArray(services)) {
        throw new Error("The registry must be a JSON array.");
      }
    } catch (reason) {
      setError(`Could not save custom registry: ${String(reason)}`);
      return;
    }

    savingRef.current = true;
    setSaving(true);
    try {
      await invoke("set_custom_registry", { services });
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
          <p>Edit the JSON array of services to add or override.</p>
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
        Each service needs id, name, category, feed_url, status_url, and type (rss or json).
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
