import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import type { Update } from "@tauri-apps/plugin-updater";
import { api, asError, type AddonView, type Progress, type StateView } from "./api";

export type View = "installed" | "browse" | "settings";
export type ToastAction = { label: string; run: () => void };
export type Toast = { id: number; message: string; kind: "info" | "error"; action?: ToastAction };

export const THEMES = [
  { id: "ember", name: "Ember", blurb: "Warm charcoal and candle-gold", swatch: ["#14100d", "#eba340", "#f1e7da"] },
  { id: "deepstone", name: "Deepstone", blurb: "Cool slate with a calm blue", swatch: ["#0e1115", "#6ea8ff", "#e6ebf1"] },
  { id: "parchment", name: "Parchment", blurb: "Light cream and rust ink", swatch: ["#f3ede2", "#b4552b", "#2a2118"] },
  { id: "fel", name: "Fel", blurb: "Near-black with sharp green", swatch: ["#0c0f0b", "#93d651", "#e4ecdc"] },
] as const;

export const app = $state({
  state: null as StateView | null,
  installed: [] as AddonView[],
  progress: {} as Record<string, Progress>,
  view: "installed" as View,
  checking: false,
  appUpdate: null as Update | null,
  toasts: [] as Toast[],
  bannerDismissed: false,
});

let toastSeq = 0;
export function toast(message: string, opts: { kind?: "info" | "error"; action?: ToastAction; ms?: number } = {}) {
  const t: Toast = { id: ++toastSeq, message, kind: opts.kind ?? "info", action: opts.action };
  app.toasts = [...app.toasts.slice(-2), t];
  setTimeout(() => dismissToast(t.id), opts.ms ?? (opts.action ? 10000 : 3200));
}
export function dismissToast(id: number) {
  app.toasts = app.toasts.filter((t) => t.id !== id);
}

/** Shows an error toast and writes it to the log. */
export function fail(e: unknown, context: string) {
  const err = asError(e);
  api.log("warn", `${context}: [${err.code}] ${err.message}`);
  toast(err.message, { kind: "error", ms: 6000 });
  return err;
}

export function applyTheme(theme: string) {
  document.documentElement.dataset.theme = theme;
  try {
    localStorage.setItem("kobold-theme", theme);
  } catch {
    /* storage can be unavailable; the setting is also saved on disk */
  }
}

export async function refreshState() {
  app.state = await api.state();
}

export async function refreshInstalled() {
  try {
    app.installed = await api.installed();
  } catch (e) {
    fail(e, "list installed");
  }
}

export async function checkUpdates(silent = false) {
  if (app.checking) return;
  app.checking = true;
  try {
    const found = await api.checkUpdates();
    await refreshInstalled();
    if (!silent) toast(found.length ? `${found.length} update${found.length > 1 ? "s" : ""} available` : "Everything is up to date");
  } catch (e) {
    if (!silent) fail(e, "check updates");
  } finally {
    app.checking = false;
  }
}

export async function installAddon(id: string, label: string) {
  try {
    const v = await api.install(id);
    await refreshInstalled();
    toast(`${v.name} ${v.version ? v.version + " " : ""}installed`);
    return v;
  } catch (e) {
    const err = asError(e);
    api.log("warn", `install ${id}: [${err.code}] ${err.message}`);
    if (err.code === "distribution_blocked" && err.url) {
      toast(`${label}'s author only allows downloads from CurseForge. Download it there, then use Import .zip.`, {
        kind: "error",
        action: { label: "Open page", run: () => openUrl(err.url!) },
      });
    } else {
      toast(err.message, { kind: "error", ms: 6000 });
    }
  } finally {
    delete app.progress[id];
  }
}

export async function updateAll() {
  const ids = app.installed.filter((a) => a.new_version && !a.pinned).map((a) => [a.id, a.name] as const);
  for (const [id, name] of ids) {
    await installAddon(id, name);
  }
}

export async function startListening() {
  await listen<Progress>("install-progress", (e) => {
    const p = e.payload;
    if (p.stage === "done" || p.stage === "error") delete app.progress[p.id];
    else app.progress[p.id] = p;
  });
}

export function formatCount(n: number) {
  if (n >= 1e6) return (n / 1e6).toFixed(n >= 1e7 ? 0 : 1) + "M";
  if (n >= 1e3) return (n / 1e3).toFixed(n >= 1e4 ? 0 : 1) + "K";
  return String(n);
}

export function timeAgo(iso: string | null) {
  if (!iso) return "";
  const s = (Date.now() - new Date(iso).getTime()) / 1000;
  if (!isFinite(s)) return "";
  if (s < 3600) return "just now";
  if (s < 86400) return "today";
  const d = Math.floor(s / 86400);
  if (d === 1) return "yesterday";
  if (d < 30) return `${d} days ago`;
  const m = Math.floor(d / 30);
  if (m < 12) return `${m} month${m > 1 ? "s" : ""} ago`;
  const y = Math.floor(m / 12);
  return `${y} year${y > 1 ? "s" : ""} ago`;
}
