import React from "react";
import ReactDOM from "react-dom/client";
import { getCurrentWindow } from "@tauri-apps/api/window";
import App from "./App";
import "./styles.css";

const RegistryEditor = React.lazy(() => import("./RegistryEditor"));

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    {getCurrentWindow().label === "registry-editor" ? (
      <React.Suspense fallback={<div className="registry-loading">Loading editor…</div>}>
        <RegistryEditor />
      </React.Suspense>
    ) : <App />}
  </React.StrictMode>,
);
