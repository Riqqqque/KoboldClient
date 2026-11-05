// Development-only stand-in for the Rust backend, so the UI can be worked on in a
// plain browser with `npm run dev`. Never included in release builds.
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import type { AddonView, RemoteAddon, StateView } from "../lib/api";

mockWindows("main");

const gh = (login: string) => `https://avatars.githubusercontent.com/${login}?s=128`;

const state: StateView = {
  game: { root: "D:\\games\\World of Warcraft\\_forever_", version: "1.60.1.70790", addons_dir: "", running: !new URLSearchParams(location.search).has("idle") },
  settings: { game_root: "D:\\games\\World of Warcraft\\_forever_", theme: localStorage.getItem("kobold-theme") ?? "ember", check_updates_on_launch: true, auto_update_addons: false, verbose_logging: false },
  has_cf_key: new URLSearchParams(location.search).has("cf"),
  app_version: "0.1.0",
  log_dir: "C:\\Users\\you\\AppData\\Local\\com.rique.koboldclient\\logs",
};

const base = { summary: "", project_url: null, pinned: false, missing: false, built_for_forever: true, new_version: null, enabled: true, icon_url: null } as const;
let installed: AddonView[] = [
  { ...base, id: "gh:sample/bossmod", name: "Sample Boss Mod", author: "Sample Author", source: "github", version: "427.8", new_version: "428.0", icon_url: gh("github"), folders: ["SampleBoss", "SampleBoss_Core"] },
  { ...base, id: "cf:1", name: "Quest Helper", author: "Sample Author", source: "curseforge", version: "10.4.2", folders: ["QuestHelper"] },
  { ...base, id: "cf:2", name: "Tweaks Plus", author: "Sample Author", source: "curseforge", version: "1.60.08", enabled: false, folders: ["TweaksPlus"] },
  { ...base, id: "gh:sample/waypoints", name: "Waypoints", author: "Sample Author", source: "github", version: "4.1.0", pinned: true, icon_url: gh("octocat"), folders: ["Waypoints"] },
  { ...base, id: "local:MyBars", name: "MyBars", author: "", source: null, version: null, folders: ["MyBars", "MyBars_Options"] },
  { ...base, id: "local:OldMapPins", name: "OldMapPins", author: "Sample Author", source: null, version: "2.3.1", built_for_forever: false, folders: ["OldMapPins"] },
];

const results: RemoteAddon[] = [
  { id: "cf:10", source: "curseforge", name: "Sample Bags", author: "Sample Author", summary: "Clean bag and bank view with sorting, junk detection and cross-character search.", downloads: 9_400_000, updated: new Date(Date.now() - 864e5 * 2).toISOString(), icon_url: gh("octocat"), project_url: "https://example.com", downloadable: true, has_forever_build: true },
  { id: "cf:1", source: "curseforge", name: "Quest Helper", author: "Sample Author", summary: "The ultimate questing helper.", downloads: 48_200_000, updated: new Date().toISOString(), icon_url: null, project_url: "https://example.com", downloadable: true, has_forever_build: true },
  { id: "cf:12", source: "curseforge", name: "Sample Inventory", author: "Sample Author", summary: "Bag replacement with rule-based sorting and layouts for every container.", downloads: 30_100_000, updated: new Date(Date.now() - 864e5 * 9).toISOString(), icon_url: null, project_url: "https://example.com", downloadable: false, has_forever_build: true },
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
    case "search":
      return results.filter((r) => !a.query || r.name.toLowerCase().includes(String(a.query).toLowerCase()));
    case "update_settings":
      state.settings = a.settings as StateView["settings"];
      return state.settings;
    case "delete_preview": {
      const x = installed.find((i) => i.id === a.id)!;
      return { name: x.name, folders: x.folders, kept: [], saved_variables: 2 };
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
    case "install": {
      const r = results.find((x) => x.id === a.id);
      const existing = installed.find((x) => x.id === a.id);
      return new Promise((res) =>
        setTimeout(() => {
          if (existing) {
            existing.version = existing.new_version ?? existing.version;
            existing.new_version = null;
            res(existing);
          } else if (r) {
            const v: AddonView = { ...base, id: r.id, name: r.name, author: r.author, source: r.source, version: "1.0.0", icon_url: r.icon_url, folders: [r.name] };
            installed = [...installed, v];
            res(v);
          }
        }, 900),
      );
    }
    default:
      return null;
  }
});
