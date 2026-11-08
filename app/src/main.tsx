import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import { listenToEvents, useStore } from "./store";
import "./styles.css";

// Set the theme before the first paint to avoid a flash.
document.documentElement.dataset.theme = window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";

async function start() {
  if (import.meta.env.DEV && !("__TAURI_INTERNALS__" in window && (window as any).__TAURI_INTERNALS__?.invoke)) {
    await (await import("./preview-mock")).installPreviewMock();
  }
  await listenToEvents();
  const s = useStore.getState();
  await s.refreshStatus();
  await Promise.all([s.refreshVoices(), s.refreshQueue()]);
}
start().catch((e) => useStore.getState().notify(String(e), "error"));

// Tick once a second so the running job's elapsed time updates.
setInterval(() => {
  const s = useStore.getState();
  if (Object.keys(s.live).length) s.set({ live: { ...s.live } });
}, 1000);

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
