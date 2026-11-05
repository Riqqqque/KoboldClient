<script lang="ts">
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { onMount } from "svelte";
  import { api } from "./lib/api";
  import { app, applyTheme, checkUpdates, fail, linkHandInstalled, play, refreshErrors, refreshInstalled, refreshState, repairRequirements, startListening, updateAll, type View } from "./lib/store.svelte";
  import { checkAppUpdate, installAppUpdate } from "./lib/updater";
  import Icon from "./components/Icon.svelte";
  import TitleBar from "./components/TitleBar.svelte";
  import Toasts from "./components/Toasts.svelte";
  import Browse from "./views/Browse.svelte";
  import Installed from "./views/Installed.svelte";
  import Settings from "./views/Settings.svelte";
  import Welcome from "./views/Welcome.svelte";

  let ready = $state(false);
  const game = $derived(app.state?.game ?? null);
  const updateCount = $derived(app.installed.filter((a) => a.new_version && !a.pinned).length);
  const installing = $derived(Object.keys(app.progress).length > 0);

  async function afterGameReady() {
    await refreshState();
    await refreshInstalled();
    refreshErrors();
    if (app.state!.has_cf_key) {
      await repairRequirements();
      await linkHandInstalled();
    }
    const s = app.state!.settings;
    if (s.check_updates_on_launch || s.auto_update_addons) {
      await checkUpdates(true);
      if (s.auto_update_addons) await updateAll();
    }
  }

  // The game writes a new log each launch and finishes it on exit, so re-read the
  // errors whenever it starts or stops.
  let wasRunning: boolean | null = null;
  $effect(() => {
    const running = app.state?.game?.running;
    if (running === undefined) return;
    if (wasRunning !== null && running !== wasRunning) refreshErrors();
    wasRunning = running;
  });

  onMount(() => {
    let timer: ReturnType<typeof setInterval> | undefined;
    (async () => {
      try {
        await startListening();
        await refreshState();
        applyTheme(app.state!.settings.theme);
      } catch (e) {
        fail(e, "startup");
      }
      ready = true;
      await getCurrentWindow().show();
      if (app.state?.game) await afterGameReady();
      if (app.state?.settings.check_updates_on_launch) checkAppUpdate(true);
      // Keep the "game is running" state fresh without hammering anything.
      timer = setInterval(() => refreshState().catch(() => {}), 15000);
    })();
    return () => clearInterval(timer);
  });

  const NAV: { id: View; label: string; icon: string }[] = [
    { id: "installed", label: "Installed", icon: "box" },
    { id: "browse", label: "Browse", icon: "search" },
    { id: "settings", label: "Settings", icon: "sliders" },
  ];

  function onkey(e: KeyboardEvent) {
    if (e.ctrlKey && e.key.toLowerCase() === "k") {
      e.preventDefault();
      app.view = "browse";
      setTimeout(() => document.querySelector<HTMLInputElement>(".bigsearch input")?.focus(), 30);
    }
  }
</script>

<svelte:window onkeydown={onkey} oncontextmenu={(e) => import.meta.env.PROD && e.preventDefault()} />

<div class="win">
  <TitleBar />
  {#if !ready}
    <div class="body"></div>
  {:else if !game}
    <Welcome ondone={afterGameReady} />
  {:else}
    <div class="body">
      <aside class="side">
        <nav class="nav">
          {#each NAV as n}
            <button class:on={app.view === n.id} onclick={() => (app.view = n.id)}>
              <Icon name={n.icon} size={17} />
              {n.label}
              {#if n.id === "installed" && updateCount}<span class="count">{updateCount}</span>{/if}
            </button>
          {/each}
        </nav>
        <div class="grow"></div>
        {#if app.appUpdate}
          <button class="appupd" onclick={installAppUpdate}>
            <b>Kobold Client {app.appUpdate.version}</b>
            <span>Ready to install. Click to update and restart.</span>
          </button>
        {/if}
        <div class="game" title={game.root}>
          <div class="t"><span class="dot" class:idle={!game.running}></span>WoW Forever</div>
          <div class="s">
            {game.version ? game.version.split(".").slice(0, 3).join(".") : "Unknown version"} · {game.running ? "Running" : "Not running"}<br />
            {app.installed.length} addon{app.installed.length === 1 ? "" : "s"}
          </div>
          <div class="path">{game.root}</div>
          <button
            class="play"
            class:playing={game.running}
            disabled={game.running || app.launching || installing || !!app.importing}
            title={installing && !app.launching ? "Wait for the current install to finish" : app.state?.settings.update_before_play ? "Update addons, then start WoW Forever" : "Start WoW Forever through Battle.net"}
            onclick={play}
          >
            {#if game.running}
              <span class="dot"></span> Playing
            {:else if app.playUpdating || (app.launching && installing)}
              <Icon name="spinner" size={16} stroke={2.4} class="spin" /> Updating addons…
            {:else if app.launching}
              <Icon name="spinner" size={16} stroke={2.4} class="spin" /> Starting…
            {:else}
              <svg width="14" height="14" viewBox="0 0 24 24" aria-hidden="true"><path d="M7 4.5v15a1 1 0 0 0 1.5.86l12.5-7.5a1 1 0 0 0 0-1.72L8.5 3.64A1 1 0 0 0 7 4.5Z" fill="currentColor" /></svg>
              Play
            {/if}
          </button>
        </div>
      </aside>
      <main class="main">
        {#if app.view === "installed"}
          <Installed />
        {:else if app.view === "browse"}
          <Browse />
        {:else}
          <Settings />
        {/if}
      </main>
    </div>
  {/if}
  <Toasts />
</div>

<style>
  .win { height: 100%; display: flex; flex-direction: column; border: 1px solid var(--border); }
  .body { flex: 1; display: flex; min-height: 0; }
  .side { width: 208px; flex: none; background: var(--panel); border-right: 1px solid var(--border); display: flex; flex-direction: column; padding: 12px 10px; }
  .nav { display: flex; flex-direction: column; gap: 2px; }
  .nav button { display: flex; align-items: center; gap: 10px; padding: 8px 10px; border-radius: 7px; border: 0; background: none; color: var(--muted); font-size: 13.5px; text-align: left; }
  .nav button:hover { background: var(--hover); color: var(--text); }
  .nav button.on { background: var(--accent-soft); color: var(--text); }
  .nav button.on :global(svg) { color: var(--accent); }
  .count { margin-left: auto; font-size: 11px; padding: 1px 7px; border-radius: 999px; background: var(--accent); color: var(--accent-ink); font-weight: 600; }
  .grow { flex: 1; }
  .appupd {
    text-align: left; border: 1px solid var(--accent); background: var(--accent-soft); border-radius: 9px; padding: 10px 11px;
    margin-bottom: 8px; display: flex; flex-direction: column; gap: 3px;
  }
  .appupd b { font-size: 12.5px; }
  .appupd span { font-size: 11.5px; color: var(--muted); line-height: 1.35; }
  .game { border: 1px solid var(--border); border-radius: 9px; padding: 10px 11px; background: var(--raised); font-size: 12px; }
  .t { display: flex; align-items: center; gap: 7px; font-weight: 600; font-size: 12.5px; }
  .dot { width: 7px; height: 7px; border-radius: 50%; background: var(--ok); box-shadow: 0 0 0 3px color-mix(in srgb, var(--ok) 20%, transparent); }
  .dot.idle { background: var(--faint); box-shadow: none; }
  .s { color: var(--muted); margin-top: 4px; line-height: 1.45; }
  .path { font-family: var(--mono); font-size: 10.5px; color: var(--faint); margin-top: 6px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .play {
    margin-top: 10px; width: 100%; height: 38px; border-radius: 8px; border: 0; display: flex; align-items: center;
    justify-content: center; gap: 8px; font-size: 14px; font-weight: 700; letter-spacing: 0.02em;
    background: var(--accent); color: var(--accent-ink); box-shadow: 0 4px 14px var(--accent-soft); transition: background 0.12s, transform 0.08s;
  }
  .play:hover:not(:disabled) { background: var(--accent-2); }
  .play:active:not(:disabled) { transform: translateY(1px); }
  .play:disabled { opacity: 1; cursor: default; }
  .play.playing { background: var(--panel); color: var(--ok); box-shadow: none; border: 1px solid var(--border); font-weight: 600; }
  .play:disabled:not(.playing) { opacity: 0.7; }
  .main { flex: 1; display: flex; flex-direction: column; min-width: 0; position: relative; }
</style>
