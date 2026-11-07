// Development-only stand-in for the Rust backend, so the UI can be worked on in a
// plain browser with `npm run dev`. Never included in release builds.
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import type { AddonView, Backup, ErrorReport, RemoteAddon, StateView } from "../lib/api";

mockWindows("main");

const gh = (login: string) => `https://avatars.githubusercontent.com/${login}?s=128`;

const state: StateView = {
  game: { root: "D:\\games\\World of Warcraft\\_forever_", version: "1.60.1.70790", addons_dir: "", running: !new URLSearchParams(location.search).has("idle") },
  settings: { game_root: "D:\\games\\World of Warcraft\\_forever_", theme: localStorage.getItem("kobold-theme") ?? "ember", check_updates_on_launch: true, auto_update_addons: false, verbose_logging: false, backup_before_update: true, update_before_play: true, lua_errors_seen: null },
  has_cf_key: new URLSearchParams(location.search).has("cf"),
  has_gh_token: false,
  has_wago_key: new URLSearchParams(location.search).has("wago"),
  app_version: "0.2.0",
  log_dir: "C:\\Users\\you\\AppData\\Local\\com.rique.koboldclient\\logs",
};

const base = { summary: "", project_url: null, pinned: false, missing: false, missing_deps: [] as AddonView["missing_deps"], built_for_forever: true, new_version: null, enabled: true, icon_url: null } as const;
let installed: AddonView[] = [
  { ...base, id: "gh:sample/bossmod", name: "Sample Boss Mod", author: "Sample Author", source: "github", version: "427.8", new_version: "428.0", icon_url: gh("github"), folders: ["SampleBoss", "SampleBoss_Core"] },
  { ...base, id: "cf:1", name: "Quest Helper", author: "Sample Author", source: "curseforge", version: "10.4.2", folders: ["QuestHelper"], missing_deps: [{ id: "cf:90", name: "Quest Helper Data" }] },
  { ...base, id: "cf:2", name: "Tweaks Plus", author: "Sample Author", source: "curseforge", version: "1.60.08", enabled: false, folders: ["TweaksPlus"] },
  { ...base, id: "gh:sample/waypoints", name: "Waypoints", author: "Sample Author", source: "github", version: "4.1.0", pinned: true, icon_url: gh("octocat"), folders: ["Waypoints"] },
  { ...base, id: "local:MyBars", name: "MyBars", author: "", source: null, version: null, folders: ["MyBars", "MyBars_Options"] },
  { ...base, id: "local:OldMapPins", name: "OldMapPins", author: "Sample Author", source: null, version: "2.3.1", built_for_forever: false, folders: ["OldMapPins"] },
];

const results: RemoteAddon[] = [
  { note: null, id: "cf:10", source: "curseforge", name: "Sample Bags", author: "Sample Author", summary: "Clean bag and bank view with sorting, junk detection and cross-character search.", downloads: 9_400_000, updated: new Date(Date.now() - 864e5 * 2).toISOString(), icon_url: gh("octocat"), project_url: "https://example.com", downloadable: true, has_forever_build: true },
  { note: null, id: "cf:1", source: "curseforge", name: "Quest Helper", author: "Sample Author", summary: "The ultimate questing helper.", downloads: 48_200_000, updated: new Date().toISOString(), icon_url: null, project_url: "https://example.com", downloadable: true, has_forever_build: true },
  { note: null, id: "cf:12", source: "curseforge", name: "Sample Inventory", author: "Sample Author", summary: "Bag replacement with rule-based sorting and layouts for every container.", downloads: 30_100_000, updated: new Date(Date.now() - 864e5 * 9).toISOString(), icon_url: null, project_url: "https://example.com", downloadable: false, has_forever_build: true },
];

const WOWI_NOTE = "WoWInterface doesn't say which addons support Forever. Kobold checks after installing.";
const wowiResults: RemoteAddon[] = [
  { note: WOWI_NOTE, id: "wowi:27236", source: "wowinterface", name: "Sample Forever UI", author: "Sample Author", summary: "Folders: SampleUI", downloads: 4_100, updated: new Date(Date.now() - 864e5 * 4).toISOString(), icon_url: null, project_url: "https://example.com", downloadable: true, has_forever_build: true },
  { note: WOWI_NOTE, id: "wowi:3826", source: "wowinterface", name: "Sample Mod Collection", author: "Sample Author", summary: "Folders: SM_Core, SM_Bars", downloads: 598_625, updated: new Date(Date.now() - 864e5 * 400).toISOString(), icon_url: null, project_url: "https://example.com", downloadable: true, has_forever_build: true },
];
const wagoResults: RemoteAddon[] = [
  { note: null, id: "wago:abc123", source: "wago", name: "Sample Raid Timers", author: "Sample Author", summary: "Timers and warnings for raid encounters.", downloads: 2_300_000, updated: new Date(Date.now() - 864e5).toISOString(), icon_url: null, project_url: "https://example.com", downloadable: true, has_forever_build: true },
];

const SESSION = "11/4 22:03:50|14";
const errorReport = (): ErrorReport => ({
  session: SESSION,
  started: "11/4 22:03:50",
  seen: state.settings.lua_errors_seen === SESSION,
  total: 14,
  addons: [
    {
      id: "cf:1", name: "Quest Helper", folder: "QuestHelper", source: "curseforge", enabled: installed.find((i) => i.id === "cf:1")?.enabled ?? true, new_version: null, project_url: "https://example.com", count: 13,
      groups: [
        { message: "Interface/AddOns/QuestHelper/Database/Zones.lua:27: attempt to index global 'QuestHelperDB' (a nil value)", count: 12, first: "11/4 22:03:54", last: "11/4 22:03:55", blame: "code", stack: ["[Interface/AddOns/QuestHelper/Database/Zones.lua]:27: in main chunk"] },
        { message: "Interface/AddOns/QuestHelper/Modules/Init.lua:425: attempt to call a nil value", count: 1, first: "11/4 22:03:55", last: "11/4 22:03:55", blame: "code", stack: ["[Interface/AddOns/QuestHelper/Libs/AceAddon-3.0.lua]:494: in function 'InitializeAddon'", "[C]: in function 'xpcall'"] },
      ],
    },
    {
      id: "gh:sample/waypoints", name: "Waypoints", folder: "Waypoints", source: "github", enabled: true, new_version: null, project_url: "https://example.com", count: 1,
      groups: [{ message: "...ddOns/Blizzard_UnitFrame/Shared/CompactUnitFrame.lua:698: attempt to compare local 'oldR' (a secret number value, while execution tainted by 'Waypoints')", count: 1, first: "11/4 22:23:59", last: "11/4 22:23:59", blame: "taint", stack: ["[Interface/AddOns/Blizzard_UnitFrame/Shared/CompactUnitFrame.lua]:698: in function 'CompactUnitFrame_UpdateHealthColor'", "[C]: in function 'Show'"] }],
    },
  ],
  other: [
    { message: "...ce/AddOns/Blizzard_ActionBar/Shared/ActionButton.lua:891: bad argument #1 to 'SetCooldown' (Usage: self:SetCooldown(start, duration [, modRate]). Secret values are only allowed during untainted execution for this argument.)", count: 1228, first: "11/4 22:21:50", last: "11/4 22:28:15", blame: "taint", stack: ["[Interface/AddOns/Blizzard_ActionBar/Shared/ActionButton.lua]:891: in function <...ce/AddOns/Blizzard_ActionBar/Shared/ActionButton.lua:889>", "[Interface/AddOns/Blizzard_GamepadActionBars/ActionBarButton.lua]:1010: in function 'UpdateCooldown'"] },
    { message: '[string "/run x()"]:1: attempt to call a nil value', count: 1, first: "11/4 22:30:00", last: "11/4 22:30:00", blame: "code", stack: [] },
  ],
  // `?taint` in the URL simulates a tainted session; `&reloaded` one after a /reload.
  ...(new URLSearchParams(location.search).has("taint")
    ? new URLSearchParams(location.search).has("reloaded")
      ? { taint: 0, taint_by: [], reloaded: "11/4 22:31:02", before_reload: 1243 }
      : { taint: 1229, taint_by: ["Waypoints"], reloaded: null, before_reload: 0 }
    : { taint: 0, taint_by: [], reloaded: null, before_reload: 0 }),
});

const now = () => Math.floor(Date.now() / 1000);
let backups: Backup[] = [
  { name: "wtf-1762290000-auto.zip", created: now() - 3600 * 5, kind: "auto", bytes: 3_400_000 },
  { name: "wtf-1762200000-manual.zip", created: now() - 86400 * 3, kind: "manual", bytes: 3_100_000 },
];

mockIPC((cmd, args) => {
  const a = (args ?? {}) as Record<string, unknown>;
  switch (cmd) {
    case "get_state":
      return state;
    case "launch_game":
      setTimeout(() => (state.game!.running = true), 4000);
      return "battlenet";
    case "list_installed":
      return installed;
    case "check_updates":
      return installed.filter((x) => x.new_version);
    case "search": {
      const pool = a.source === "wowinterface" ? wowiResults : a.source === "wago" ? wagoResults : results;
      return pool.filter((r) => !a.query || r.name.toLowerCase().includes(String(a.query).toLowerCase()));
    }
    case "lua_errors":
      return errorReport();
    case "dismiss_lua_errors":
      state.settings.lua_errors_seen = String(a.session);
      return null;
    case "list_backups":
      return backups;
    case "backup_now": {
      const b: Backup = { name: `wtf-${now()}-manual.zip`, created: now(), kind: "manual", bytes: 3_450_000 };
      backups = [b, ...backups];
      return new Promise((res) => setTimeout(() => res(b), 700));
    }
    case "restore_backup": {
      if (state.game?.running) return Promise.reject({ code: "other", message: "Close WoW Forever first.", url: null });
      backups = [{ name: `wtf-${now()}-restore.zip`, created: now(), kind: "restore", bytes: 3_460_000 }, ...backups];
      return new Promise((res) => setTimeout(() => res(null), 900));
    }
    case "delete_backup":
      backups = backups.filter((b) => b.name !== a.name);
      return null;
    case "link_hand_installed": {
      const mine = installed.find((i) => i.id === "local:OldMapPins");
      if (!mine || !state.has_cf_key) return { linked: [] };
      installed = installed.map((i) => (i.id === "local:OldMapPins" ? { ...i, id: "cf:77", source: "curseforge" as const, new_version: "2.4.0" } : i));
      return { linked: ["OldMapPins"] };
    }
    case "plugin:dialog|open": {
      const exts = ((a.options as any)?.filters?.[0]?.extensions ?? []) as string[];
      return exts.includes("json") ? "C:\\Users\\you\\Downloads\\kobold-addons.json" : null;
    }
    case "plugin:dialog|save":
      return "C:\\Users\\you\\Downloads\\kobold-addons-2026-11-04.json";
    case "export_addon_list":
      return installed.length;
    case "preview_addon_list":
      return {
        to_install: [
          { id: "cf:10", name: "Sample Bags", source: "curseforge", version: "1.0", project_url: null, pinned: false, enabled: true },
          { id: "wowi:27236", name: "Sample Forever UI", source: "wowinterface", version: "0.10", project_url: null, pinned: true, enabled: true },
        ],
        present: installed.slice(0, 3).map((i) => ({ id: i.id, name: i.name, source: i.source, version: i.version, project_url: null, pinned: false, enabled: true })),
        manual: [{ id: "local:MyBars", name: "MyBars", source: null, version: null, project_url: "https://example.com", pinned: false, enabled: true }],
      };
    case "save_wago_key":
      state.has_wago_key = true;
      return null;
    case "clear_wago_key":
      state.has_wago_key = false;
      return null;
    case "save_gh_token":
      state.has_gh_token = true;
      return 5000;
    case "clear_gh_token":
      state.has_gh_token = false;
      return null;
    case "set_pinned":
      installed = installed.map((i) => (i.id === a.id ? { ...i, pinned: Boolean(a.pinned) } : i));
      return null;
    case "update_settings":
      state.settings = a.settings as StateView["settings"];
      return state.settings;
    case "delete_preview": {
      const x = installed.find((i) => i.id === a.id)!;
      const needed_by = installed.filter((i) => i.missing_deps.some((d) => d.id === a.id)).map((i) => i.name);
      return { name: x.name, folders: x.folders, kept: [], saved_variables: 2, needed_by: a.id === "cf:2" ? ["Quest Helper"] : needed_by };
    }
    case "remove_addon": {
      const x = installed.find((i) => i.id === a.id)!;
      installed = installed.filter((i) => i.id !== a.id);
      (globalThis as any).__undo = x;
      return { token: "t", name: x.name };
    }
    case "undo_remove":
      installed = [...installed, (globalThis as any).__undo].sort((p, q) => p.name.localeCompare(q.name));
      return null;
    case "set_enabled":
      installed = installed.map((i) => (i.id === a.id ? { ...i, enabled: Boolean(a.enabled) } : i));
      return null;
    case "repair_requirements":
      return { installed: [], needed_by: [], failed: [] };
    case "install_requirements": {
      const x = installed.find((i) => i.id === a.id)!;
      const names = x.missing_deps.map((d) => d.name);
      return new Promise((res) =>
        setTimeout(() => {
          installed = [...installed, ...x.missing_deps.map((d) => ({ ...base, id: d.id, name: d.name, author: "Sample Author", source: "curseforge" as const, version: "1.0.0", folders: [d.name] }))]
            .map((i) => (i.id === x.id ? { ...i, missing_deps: [] } : i))
            .sort((p, q) => p.name.localeCompare(q.name));
          res(names);
        }, 1200),
      );
    }
    case "install": {
      const r = [...results, ...wowiResults, ...wagoResults].find((x) => x.id === a.id);
      const existing = installed.find((x) => x.id === a.id);
      return new Promise((res) =>
        setTimeout(() => {
          if (existing) {
            existing.version = existing.new_version ?? existing.version;
            existing.new_version = null;
            res({ addon: existing, also_installed: [] });
          } else if (r) {
            const v: AddonView = { ...base, id: r.id, name: r.name, author: r.author, source: r.source, version: "1.0.0", icon_url: r.icon_url, folders: [r.name] };
            installed = [...installed, v];
            res({ addon: v, also_installed: r.id === "cf:10" ? ["Sample Bags Data"] : [] });
          }
        }, 900),
      );
    }
    default:
      return null;
  }
});
