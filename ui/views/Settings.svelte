<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { api, type Settings } from "../lib/api";
  import { app, applyTheme, fail, refreshInstalled, refreshState, THEMES, toast } from "../lib/store.svelte";
  import { checkAppUpdate } from "../lib/updater";
  import CurseForgeSetup from "../components/CurseForgeSetup.svelte";
  import Icon from "../components/Icon.svelte";
  import Toggle from "../components/Toggle.svelte";

  const s = $derived(app.state?.settings);

  async function patch(change: Partial<Settings>) {
    if (!s) return;
    try {
      await api.updateSettings({ ...s, ...change });
      await refreshState();
    } catch (e) {
      fail(e, "save settings");
    }
  }

  async function setTheme(id: string) {
    applyTheme(id);
    await patch({ theme: id });
  }

  async function changeFolder() {
    const path = await open({ directory: true, multiple: false, title: "Pick your World of Warcraft or _forever_ folder" });
    if (!path || Array.isArray(path)) return;
    try {
      await api.setGameRoot(path);
      await refreshState();
      await refreshInstalled();
      toast("Game folder updated");
    } catch (e) {
      fail(e, "set game folder");
    }
  }

  let detecting = $state(false);
  async function redetect() {
    detecting = true;
    try {
      const found = await api.detectGame();
      if (!found.length) {
        toast("Couldn't find WoW Forever automatically. Pick the folder instead.", { kind: "error" });
        return;
      }
      await api.setGameRoot(found[0].root);
      await refreshState();
      await refreshInstalled();
      toast(`Found WoW Forever at ${found[0].root}`);
    } catch (e) {
      fail(e, "detect game");
    } finally {
      detecting = false;
    }
  }

  async function exportDiag() {
    try {
      const path = await api.exportDiagnostics();
      toast(`Saved ${path.split(/[\\/]/).pop()} to your Downloads folder`);
    } catch (e) {
      fail(e, "export diagnostics");
    }
  }
</script>

<section class="view">
  <div class="head">
    <div>
      <h2>Settings</h2>
      <div class="sub">Kobold Client {app.state?.app_version} · by Rique</div>
    </div>
  </div>

  {#if s}
    <div class="settings">
      <div class="sec">
        <h3>Appearance</h3>
        <div class="themes">
          {#each THEMES as t}
            <button class="theme" class:on={s.theme === t.id} onclick={() => setTheme(t.id)}>
              <span class="sw">{#each t.swatch as c}<i style:background={c}></i>{/each}</span>
              <b>{t.name}</b>
              <small>{t.blurb}</small>
            </button>
          {/each}
        </div>
      </div>

      <div class="sec">
        <h3>Game</h3>
        <div class="set">
          <div class="l">
            <b>WoW Forever folder</b>
            <small class="mono selectable">{app.state?.game?.root ?? "Not set"}{app.state?.game?.version ? ` · ${app.state.game.version}` : ""}</small>
          </div>
          <button class="btn sm" disabled={detecting} onclick={redetect}>{detecting ? "Searching…" : "Find automatically"}</button>
          <button class="btn sm" onclick={changeFolder}>Choose…</button>
        </div>
        <div class="set">
          <div class="l"><b>AddOns folder</b><small>Open it in File Explorer</small></div>
          <button class="btn sm" disabled={!app.state?.game} onclick={() => api.openFolder("addons")}><Icon name="folder" size={14} /> Open</button>
        </div>
      </div>

      <div class="sec">
        <h3>CurseForge</h3>
        <CurseForgeSetup compact />
      </div>

      <div class="sec">
        <h3>Updates</h3>
        <div class="set">
          <div class="l"><b>Check for addon updates on launch</b><small>Only checks. Nothing installs unless you turn on the option below.</small></div>
          <Toggle on={s.check_updates_on_launch} label="Check on launch" onchange={(v) => patch({ check_updates_on_launch: v })} />
        </div>
        <div class="set">
          <div class="l"><b>Update addons automatically</b><small>Installs addon updates on launch. Pinned addons are skipped.</small></div>
          <Toggle on={s.auto_update_addons} label="Auto-update addons" onchange={(v) => patch({ auto_update_addons: v })} />
        </div>
        <div class="set">
          <div class="l"><b>Kobold Client updates</b><small>New versions install themselves from GitHub after you confirm.</small></div>
          <button class="btn sm" onclick={() => checkAppUpdate(false)}>Check now</button>
        </div>
      </div>

      <div class="sec">
        <h3>Troubleshooting</h3>
        <div class="set">
          <div class="l"><b>Detailed logging</b><small>Writes more detail to the log files. Turn on when reporting a bug.</small></div>
          <Toggle on={s.verbose_logging} label="Detailed logging" onchange={(v) => patch({ verbose_logging: v })} />
        </div>
        <div class="set">
          <div class="l"><b>Logs</b><small class="mono selectable">{app.state?.log_dir}</small></div>
          <button class="btn sm" onclick={() => api.openFolder("logs")}>Open folder</button>
          <button class="btn sm" onclick={exportDiag}>Export diagnostics</button>
        </div>
        <div class="set">
          <div class="l"><b>Report a problem</b><small>Attach the diagnostics zip so the issue can be traced.</small></div>
          <button class="btn sm" onclick={() => openUrl("https://github.com/Riqqqque/KoboldClient/issues")}>Open issues <Icon name="external" size={12} /></button>
        </div>
      </div>
    </div>
  {/if}
</section>

<style>
  .settings { flex: 1; overflow: auto; padding: 0 22px 22px; }
  .sec { border: 1px solid var(--border); background: var(--panel); border-radius: var(--radius); margin-bottom: 12px; max-width: 820px; }
  .sec h3 {
    margin: 0; padding: 12px 16px; font-size: 11.5px; text-transform: uppercase; letter-spacing: 0.08em; color: var(--muted);
    font-weight: 600; border-bottom: 1px solid var(--border);
  }
  .set { display: flex; align-items: center; gap: 12px; padding: 12px 16px; border-bottom: 1px solid color-mix(in srgb, var(--border) 60%, transparent); }
  .set:last-child { border-bottom: 0; }
  .l { flex: 1; min-width: 0; }
  .l b { font-size: 13.5px; font-weight: 500; display: block; }
  .l small { color: var(--muted); font-size: 12px; display: block; margin-top: 2px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .themes { display: grid; grid-template-columns: repeat(4, 1fr); gap: 10px; padding: 14px 16px; }
  .theme { text-align: left; padding: 11px 12px; border-radius: 9px; border: 1px solid var(--border); background: var(--raised); display: flex; flex-direction: column; gap: 3px; }
  .theme:hover { border-color: var(--faint); }
  .theme.on { border-color: var(--accent); box-shadow: 0 0 0 3px var(--accent-soft); }
  .theme b { font-size: 13px; }
  .theme small { color: var(--muted); font-size: 11.5px; line-height: 1.35; }
  .sw { display: flex; gap: 3px; margin-bottom: 4px; }
  .sw i { width: 14px; height: 14px; border-radius: 4px; box-shadow: inset 0 0 0 1px rgba(128, 128, 128, 0.3); }
</style>
