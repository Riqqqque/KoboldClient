import { invoke } from "@tauri-apps/api/core";

export type Source = "curseforge" | "github" | "zip" | "wago" | "wowinterface";
export type SearchSource = "curseforge" | "wago" | "wowinterface" | "github";

export interface GameInfo {
  root: string;
  version: string | null;
  addons_dir: string;
  running: boolean;
}

export interface Settings {
  game_root: string | null;
  theme: string;
  check_updates_on_launch: boolean;
  auto_update_addons: boolean;
  verbose_logging: boolean;
  backup_before_update: boolean;
  update_before_play: boolean;
  lua_errors_seen: string | null;
}

export interface StateView {
  game: GameInfo | null;
  settings: Settings;
  has_cf_key: boolean;
  has_gh_token: boolean;
  has_wago_key: boolean;
  app_version: string;
  log_dir: string;
}

export interface AddonView {
  id: string;
  name: string;
  author: string;
  summary: string;
  source: Source | null;
  version: string | null;
  new_version: string | null;
  icon_url: string | null;
  project_url: string | null;
  enabled: boolean;
  pinned: boolean;
  folders: string[];
  built_for_forever: boolean;
  missing: boolean;
  /** Required addons that aren't installed, so this one won't work yet. */
  missing_deps: Requirement[];
}

export interface Requirement {
  id: string;
  name: string;
}

export interface InstallOutcome {
  addon: AddonView;
  also_installed: string[];
}

export interface RepairOutcome {
  installed: string[];
  needed_by: string[];
  failed: string[];
}

export interface RemoteAddon {
  id: string;
  source: Source;
  name: string;
  author: string;
  summary: string;
  downloads: number;
  updated: string | null;
  icon_url: string | null;
  project_url: string;
  downloadable: boolean;
  has_forever_build: boolean;
  note: string | null;
}

export interface Progress {
  id: string;
  stage: "download" | "verify" | "backup" | "install" | "done" | "error";
  pct: number;
}

export interface DeletePreview {
  name: string;
  folders: string[];
  kept: [string, string][];
  saved_variables: number;
  needed_by: string[];
}

export interface Removed {
  token: string;
  name: string;
}

export interface ErrorGroup {
  message: string;
  count: number;
  first: string;
  last: string;
  stack: string[];
  blame: "code" | "taint";
}

export interface AddonErrors {
  id: string | null;
  name: string;
  folder: string;
  source: Source | null;
  enabled: boolean;
  new_version: string | null;
  project_url: string | null;
  count: number;
  groups: ErrorGroup[];
}

export interface ErrorReport {
  session: string;
  started: string | null;
  seen: boolean;
  total: number;
  addons: AddonErrors[];
  other: ErrorGroup[];
  /** Taint errors since the interface last loaded; /reload clears them. */
  taint: number;
  /** Addons the game named in those taint errors. */
  taint_by: string[];
  /** When the interface last reloaded mid-session, if errors came before it. */
  reloaded: string | null;
  before_reload: number;
}

export interface Backup {
  name: string;
  created: number;
  kind: "auto" | "manual" | "restore";
  bytes: number;
}

export interface ListEntry {
  id: string;
  name: string;
  source: Source | null;
  version: string | null;
  project_url: string | null;
  pinned: boolean;
  enabled: boolean;
}

export interface ImportPreview {
  to_install: ListEntry[];
  present: ListEntry[];
  manual: ListEntry[];
}

export interface AppError {
  code: string;
  message: string;
  url: string | null;
}

/** Every backend error arrives as `{code, message, url}`; anything else is wrapped. */
export function asError(e: unknown): AppError {
  if (e && typeof e === "object" && "message" in e) {
    const o = e as Partial<AppError>;
    return { code: o.code ?? "other", message: String(o.message), url: o.url ?? null };
  }
  return { code: "other", message: String(e), url: null };
}

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(cmd, args);
  } catch (e) {
    throw asError(e);
  }
}

export const api = {
  state: () => call<StateView>("get_state"),
  detectGame: () => call<GameInfo[]>("detect_game"),
  setGameRoot: (path: string) => call<GameInfo>("set_game_root", { path }),
  updateSettings: (settings: Settings) => call<Settings>("update_settings", { settings }),
  saveCfKey: (key: string) => call<void>("save_cf_key", { key }),
  clearCfKey: () => call<void>("clear_cf_key"),
  saveGhToken: (token: string) => call<number>("save_gh_token", { token }),
  clearGhToken: () => call<void>("clear_gh_token"),
  saveWagoKey: (key: string) => call<void>("save_wago_key", { key }),
  clearWagoKey: () => call<void>("clear_wago_key"),
  luaErrors: () => call<ErrorReport>("lua_errors"),
  dismissLuaErrors: (session: string) => call<void>("dismiss_lua_errors", { session }),
  listBackups: () => call<Backup[]>("list_backups"),
  backupNow: () => call<Backup>("backup_now"),
  restoreBackup: (name: string) => call<void>("restore_backup", { name }),
  deleteBackup: (name: string) => call<void>("delete_backup", { name }),
  linkHandInstalled: () => call<{ linked: string[] }>("link_hand_installed"),
  exportAddonList: (path: string) => call<number>("export_addon_list", { path }),
  previewAddonList: (path: string) => call<ImportPreview>("preview_addon_list", { path }),
  installed: () => call<AddonView[]>("list_installed"),
  search: (source: SearchSource, query: string, sort: string) =>
    call<RemoteAddon[]>("search", { source, query, sort }),
  install: (id: string) => call<InstallOutcome>("install", { id }),
  installRequirements: (id: string) => call<string[]>("install_requirements", { id }),
  repairRequirements: () => call<RepairOutcome>("repair_requirements"),
  importZip: (path: string) => call<AddonView>("import_zip", { path }),
  checkUpdates: () => call<AddonView[]>("check_updates"),
  deletePreview: (id: string) => call<DeletePreview>("delete_preview", { id }),
  remove: (id: string, deleteSettings: boolean) => call<Removed>("remove_addon", { id, deleteSettings }),
  undoRemove: (token: string) => call<void>("undo_remove", { token }),
  setEnabled: (id: string, enabled: boolean) => call<void>("set_enabled", { id, enabled }),
  setPinned: (id: string, pinned: boolean) => call<void>("set_pinned", { id, pinned }),
  launchGame: () => call<"battlenet" | "direct">("launch_game"),
  openFolder: (which: "logs" | "addons" | "backups") => call<void>("open_folder", { which }),
  exportDiagnostics: () => call<string>("export_diagnostics"),
  log: (level: "info" | "warn" | "error", message: string) =>
    invoke("log_from_ui", { level, message }).catch(() => {}),
};
