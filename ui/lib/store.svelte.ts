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
  launching: false,
  /** Ids of addons whose missing requirements are being installed. */
  fixing: {} as Record<string, boolean>,
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

/** "A", "A and B", "A, B and C". */
export function joinNames(names: string[]) {
  return names.length < 2 ? (names[0] ?? "") : `${names.slice(0, -1).join(", ")} and ${names[names.length - 1]}`;
}

function installFailed(context: string, e: unknown) {
  const err = asError(e);
  api.log("warn", `${context}: [${err.code}] ${err.message}`);
  if (err.code === "distribution_blocked" && err.url) {
    toast(`${err.message} Download it there, then use Import .zip.`, {
      kind: "error",
      action: { label: "Open page", run: () => openUrl(err.url!) },
    });
  } else {
    toast(err.message, { kind: "error", ms: 7000 });
  }
}

export async function installAddon(id: string, _label: string) {
  try {
    const { addon: v, also_installed } = await api.install(id);
    await refreshInstalled();
    const extra = also_installed.length ? `, along with ${joinNames(also_installed)} which it needs` : "";
    toast(`${v.name} ${v.version ? v.version + " " : ""}installed${extra}`, { ms: extra ? 6000 : undefined });
    return v;
  } catch (e) {
    installFailed(`install ${id}`, e);
  } finally {
    delete app.progress[id];
  }
}

/** Installs the required addons one installed addon is missing. */
export async function installRequirements(a: AddonView) {
  if (app.fixing[a.id]) return;
  app.fixing[a.id] = true;
  try {
    const names = await api.installRequirements(a.id);
    await refreshInstalled();
    toast(names.length ? `Installed ${joinNames(names)} for ${a.name}` : `${a.name} has everything it needs`);
  } catch (e) {
    installFailed(`install requirements ${a.id}`, e);
    await refreshInstalled();
  } finally {
    delete app.fixing[a.id];
  }
}

/** On launch: fills in requirements for addons installed by older versions and
 *  installs any that are missing. Quiet unless it did something. */
export async function repairRequirements() {
  try {
    const r = await api.repairRequirements();
    if (r.installed.length || r.failed.length) await refreshInstalled();
    if (r.installed.length) {
      toast(`Installed ${joinNames(r.installed)}, which ${joinNames(r.needed_by)} needs to work`, { ms: 8000 });
    }
    for (const msg of r.failed) toast(msg, { kind: "error", ms: 8000 });
  } catch (e) {
    api.log("warn", `repair requirements: [${asError(e).code}] ${asError(e).message}`);
  }
}

export async function updateAll() {
  const ids = app.installed.filter((a) => a.new_version && !a.pinned).map((a) => [a.id, a.name] as const);
  for (const [id, name] of ids) {
    await installAddon(id, name);
  }
}

/** Starts the game, then watches for it to appear so the button can switch to "Playing". */
export async function play() {
  if (app.launching || app.state?.game?.running) return;
  app.launching = true;
  try {
    const via = await api.launchGame();
    toast(via === "battlenet" ? "Starting WoW Forever through Battle.net…" : "Starting WoW Forever…");
    for (let i = 0; i < 30 && !app.state?.game?.running; i++) {
      await new Promise((r) => setTimeout(r, 3000));
      await refreshState().catch(() => {});
    }
    if (!app.state?.game?.running) api.log("warn", "game didn't appear within 90s of launch");
  } catch (e) {
    fail(e, "launch game");
  } finally {
    app.launching = false;
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
