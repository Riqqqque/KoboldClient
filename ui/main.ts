import { mount } from "svelte";
import "./styles/app.css";
import App from "./App.svelte";
import { api } from "./lib/api";

window.addEventListener("error", (e) => api.log("error", `ui error: ${e.message} at ${e.filename}:${e.lineno}`));
window.addEventListener("unhandledrejection", (e) => api.log("error", `ui unhandled rejection: ${String(e.reason?.message ?? e.reason)}`));

if (import.meta.env.DEV && !("__TAURI_INTERNALS__" in window)) {
  await import("./dev/mock");
}

export default mount(App, { target: document.getElementById("app")! });
