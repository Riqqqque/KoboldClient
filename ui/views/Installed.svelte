<script lang="ts">
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { api, type AddonView } from "../lib/api";
  import { app, checkUpdates, fail, installAddon, installRequirements, joinNames, linkHandInstalled, refreshErrors, refreshInstalled, SOURCE_LABEL, toast, updateAll } from "../lib/store.svelte";
  import AddonIcon from "../components/AddonIcon.svelte";
  import DeleteDialog from "../components/DeleteDialog.svelte";
  import ErrorsDialog from "../components/ErrorsDialog.svelte";
  import Icon from "../components/Icon.svelte";
  import Toggle from "../components/Toggle.svelte";

  type Filter = "all" | "updates" | "unmanaged" | "disabled";
  let filter = $state<Filter>("all");
  let query = $state("");
  let deleting = $state<AddonView | null>(null);
  let menuFor = $state<string | null>(null);

  const updates = $derived(app.installed.filter((a) => a.new_version && !a.pinned));
  const shown = $derived(
    app.installed
      .filter((a) =>
        filter === "updates" ? !!a.new_version : filter === "unmanaged" ? !a.source : filter === "disabled" ? !a.enabled : true,
      )
      .filter((a) => !query || a.name.toLowerCase().includes(query.toLowerCase())),
  );
  const errorCount = $derived(new Map((app.errors?.addons ?? []).filter((e) => e.id).map((e) => [e.id!, e.count])));
  const showErrorBanner = $derived(!!app.errors && app.errors.total > 0 && !app.errors.seen);
  const handCount = $derived(app.installed.filter((a) => !a.source || a.source === "zip").length);
  let linking = $state(false);

  async function openErrors() {
    await refreshErrors();
    app.errorsOpen = true;
  }

  async function link() {
    linking = true;
    await linkHandInstalled(true);
    linking = false;
  }

  async function toggle(a: AddonView, on: boolean) {
    try {
      await api.setEnabled(a.id, on);
      await refreshInstalled();
      toast(`${a.name} ${on ? "enabled" : "disabled"}${app.state?.game?.running ? ". Type /reload in game." : ""}`);
    } catch (e) {
      fail(e, "toggle");
    }
  }

  async function pin(a: AddonView) {
    menuFor = null;
    try {
      await api.setPinned(a.id, !a.pinned);
      await refreshInstalled();
      toast(a.pinned ? `${a.name} will get updates again` : `${a.name} is pinned at ${a.version}`);
    } catch (e) {
      fail(e, "pin");
    }
  }

  function stageLabel(stage: string) {
    return stage === "download" ? "Downloading" : stage === "verify" ? "Verifying" : stage === "backup" ? "Backing up settings" : "Installing";
  }
</script>

<svelte:window onclick={() => (menuFor = null)} />

<section class="view">
  <div class="head">
    <div>
      <h2>Installed</h2>
      <div class="sub">
        {app.installed.length} addon{app.installed.length === 1 ? "" : "s"} ·
        {updates.length ? `${updates.length} update${updates.length > 1 ? "s" : ""} available` : "all up to date"}
      </div>
    </div>
    <div class="sp"></div>
    <label class="search">
      <Icon name="search" size={15} stroke={2} />
      <input placeholder="Filter installed" bind:value={query} />
    </label>
    <button class="btn" title="Check for updates" disabled={app.checking} onclick={() => checkUpdates()}>
      <Icon name="refresh" size={15} class={app.checking ? "spin" : ""} />
    </button>
    <button class="btn primary" disabled={!updates.length} onclick={updateAll}>
      <Icon name="download" size={15} stroke={2.2} />
      {updates.length ? `Update all (${updates.length})` : "Up to date"}
    </button>
  </div>

  <div class="chips">
    {#each [["all", "All"], ["updates", "Updates"], ["unmanaged", "Installed by hand"], ["disabled", "Disabled"]] as [f, label]}
      <button class="chip" class:on={filter === f} onclick={() => (filter = f as Filter)}>{label}</button>
    {/each}
  </div>

  {#if app.importing}
    <div class="banner">
      <Icon name="spinner" size={16} stroke={2.2} class="spin" />
      Installing from your addon list: {app.importing.done + 1} of {app.importing.total}{app.importing.name ? ` · ${app.importing.name}` : ""}
    </div>
  {/if}

  {#if showErrorBanner}
    {@const names = app.errors!.addons.map((a) => a.name)}
    <div class="banner bad">
      <Icon name="alert" size={16} stroke={2} />
      <span>
        {joinNames(names.slice(0, 3))}{names.length > 3 ? ` and ${names.length - 3} more` : ""}
        had Lua errors {app.state?.game?.running ? "this session" : "last time you played"}.
      </span>
      <button class="btn sm" onclick={openErrors}>View errors</button>
    </div>
  {/if}

  {#if filter === "unmanaged" && handCount && app.state?.has_cf_key}
    <div class="banner">
      <Icon name="info" size={16} stroke={2} />
      Kobold can't update addons you installed by hand unless it finds them on CurseForge. It checks each launch.
      <button class="btn sm" style="margin-left:auto" disabled={linking} onclick={link}>{linking ? "Checking…" : "Check now"}</button>
    </div>
  {/if}

  {#if app.state?.game?.running && !app.bannerDismissed}
    <div class="banner">
      <Icon name="info" size={16} stroke={2} />
      WoW is running. Changes take effect after <b>/reload</b> in game.
      <button class="x" aria-label="Dismiss" onclick={() => (app.bannerDismissed = true)}>✕</button>
    </div>
  {/if}

  <div class="list">
    {#if app.installed.length === 0}
      <div class="empty">
        <Icon name="box" size={36} stroke={1.4} />
        <h3>No addons yet</h3>
        Find some in Browse, add one from GitHub, or import a .zip you downloaded.
        <div style="margin-top:16px"><button class="btn primary" onclick={() => (app.view = "browse")}>Browse addons</button></div>
      </div>
    {:else if shown.length === 0}
      <div class="empty">Nothing matches this filter.</div>
    {/if}
    {#each shown as a (a.id)}
      {@const prog = app.progress[a.id]}
      <div class="row" class:off={!a.enabled}>
        <AddonIcon name={a.name} url={a.icon_url} dim={!a.enabled} />
        <div class="nm">
          <b>
            <span class="title">{a.name}</span>
            {#if !a.source}<span class="tag mute" title="Kobold didn't install this one, so it can't update it">Installed by hand</span>{/if}
            {#if !a.built_for_forever}<span class="tag warn" title="No TOC for interface 16001. It may not load.">Not built for Forever</span>{/if}
            {#if a.missing}<span class="tag danger">Files missing</span>{/if}
            {#if errorCount.get(a.id)}
              <button class="tag danger tagbtn" title="Show the Lua errors {a.name} caused" onclick={openErrors}>
                <Icon name="alert" size={10} stroke={2.4} /> {errorCount.get(a.id)} error{errorCount.get(a.id)! > 1 ? "s" : ""}
              </button>
            {/if}
            {#if a.missing_deps.length}<span class="tag danger" title="{a.name} won't work until {joinNames(a.missing_deps.map((d) => d.name))} {a.missing_deps.length > 1 ? 'are' : 'is'} installed">Needs {joinNames(a.missing_deps.map((d) => d.name))}</span>{/if}
            {#if a.pinned}<span class="tag mute"><Icon name="pin" size={10} stroke={2.2} /> Pinned</span>{/if}
          </b>
          <small>
            {[a.author, a.source ? SOURCE_LABEL[a.source] : "Local folder"].filter(Boolean).join(" · ")}{#if a.version}{" · "}<span class="ver">{a.version}</span>{/if}
            {#if a.new_version}<span class="new"> → {a.new_version}</span>{/if}
          </small>
        </div>
        <div class="act">
          {#if prog}
            <div class="prog"><div class="bar"><i style:width="{prog.pct}%"></i></div><small>{stageLabel(prog.stage)} · {prog.pct}%</small></div>
          {:else if app.fixing[a.id]}
            <span class="ok-txt"><Icon name="refresh" size={13} class="spin" /> Installing…</span>
          {:else if a.missing && a.source && a.source !== "zip"}
            <button class="btn sm" onclick={() => installAddon(a.id, a.name)}>Reinstall</button>
          {:else if a.missing_deps.length}
            <button class="btn sm primary" title="Install {joinNames(a.missing_deps.map((d) => d.name))}" onclick={() => installRequirements(a)}>Install missing</button>
          {:else if a.new_version}
            <button class="btn sm primary" onclick={() => installAddon(a.id, a.name)}>Update</button>
          {:else if a.source}
            <span class="ok-txt"><Icon name="check" size={13} stroke={2.4} /> Up to date</span>
          {/if}
        </div>
        <Toggle on={a.enabled} label={a.enabled ? "Disable" : "Enable"} onchange={(v) => toggle(a, v)} />
        <div class="tools">
          <button
            class="icon-btn"
            title="More"
            onclick={(e) => {
              e.stopPropagation();
              menuFor = menuFor === a.id ? null : a.id;
            }}><Icon name="more" size={18} stroke={2.6} /></button>
          <button class="icon-btn del" title="Delete" onclick={() => (deleting = a)}><Icon name="trash" size={16} /></button>
          {#if menuFor === a.id}
            <div class="menu" role="menu">
              {#if a.source && a.source !== "zip"}
                <button onclick={() => pin(a)}><Icon name="pin" size={14} /> {a.pinned ? "Unpin, allow updates" : "Pin this version"}</button>
              {/if}
              {#if a.project_url}
                <button onclick={() => openUrl(a.project_url!)}><Icon name="external" size={14} /> Open addon page</button>
              {/if}
              <button onclick={() => api.openFolder("addons")}><Icon name="folder" size={14} /> Open AddOns folder</button>
              <div class="folders mono">{a.folders.join(", ")}</div>
            </div>
          {/if}
        </div>
      </div>
    {/each}
  </div>
</section>

{#if deleting}
  <DeleteDialog addon={deleting} onclose={() => (deleting = null)} />
{/if}

{#if app.errorsOpen}
  <ErrorsDialog onclose={() => (app.errorsOpen = false)} />
{/if}

<style>
  .list { flex: 1; overflow: auto; padding: 0 14px 16px; }
  .row {
    display: grid; grid-template-columns: 40px minmax(0, 1fr) minmax(112px, auto) 40px 66px; align-items: center; gap: 12px;
    padding: 10px 8px; border-radius: 9px; border-bottom: 1px solid color-mix(in srgb, var(--border) 60%, transparent);
  }
  .row:hover { background: var(--panel); }
  .nm { min-width: 0; }
  .nm b { font-weight: 600; font-size: 13.5px; display: flex; align-items: center; gap: 7px; min-width: 0; }
  .nm .title { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .nm .tag { display: inline-flex; align-items: center; gap: 3px; }
  .nm small { display: block; color: var(--muted); font-size: 12px; margin-top: 2px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .ver { font-family: var(--mono); font-size: 11.5px; }
  .new { color: var(--accent); font-family: var(--mono); font-size: 11.5px; }
  .off .title { color: var(--muted); }
  .tagbtn { border: 0; cursor: pointer; font-family: inherit; }
  .tagbtn:hover { filter: brightness(1.15); }
  .banner.bad { background: var(--danger-soft); }
  .banner.bad > :global(svg) { color: var(--danger); }
  .banner.bad .btn { margin-left: auto; }
  .act { display: flex; justify-content: flex-end; }
  .tools { position: relative; display: flex; gap: 2px; }
  .icon-btn { width: 30px; height: 30px; border-radius: 7px; border: 0; background: none; color: var(--faint); display: grid; place-items: center; }
  .icon-btn:hover { background: var(--hover); color: var(--text); }
  .icon-btn.del:hover { background: var(--danger-soft); color: var(--danger); }
  .menu {
    position: absolute; right: 0; top: 34px; z-index: 20; min-width: 220px; background: var(--panel); border: 1px solid var(--border);
    border-radius: 9px; box-shadow: var(--shadow); padding: 5px; animation: pop 0.12s ease-out;
  }
  .menu button { display: flex; align-items: center; gap: 9px; width: 100%; text-align: left; padding: 7px 9px; border: 0; background: none; border-radius: 6px; font-size: 13px; }
  .menu button:hover { background: var(--hover); }
  .menu .folders { padding: 6px 9px 4px; color: var(--faint); font-size: 11px; border-top: 1px solid var(--border); margin-top: 4px; word-break: break-all; }
  @keyframes pop { from { transform: translateY(-4px); opacity: 0; } }
</style>
