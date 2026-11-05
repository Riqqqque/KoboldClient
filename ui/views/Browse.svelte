<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { api, asError, type RemoteAddon } from "../lib/api";
  import { app, fail, formatCount, installAddon, refreshInstalled, timeAgo, toast } from "../lib/store.svelte";
  import AddonIcon from "../components/AddonIcon.svelte";
  import CurseForgeSetup from "../components/CurseForgeSetup.svelte";
  import Icon from "../components/Icon.svelte";
  import Modal from "../components/Modal.svelte";

  type Sort = "popular" | "downloads" | "updated";
  let query = $state("");
  let sort = $state<Sort>("popular");
  let results = $state<RemoteAddon[]>([]);
  let loading = $state(false);
  let error = $state<string | null>(null);
  let addUrlOpen = $state(false);
  let repoInput = $state("");
  let importing = $state(false);
  let searchSeq = 0;

  const hasKey = $derived(app.state?.has_cf_key ?? false);
  const installedIds = $derived(new Map(app.installed.map((a) => [a.id, a])));

  async function runSearch() {
    if (!hasKey) return;
    const seq = ++searchSeq;
    loading = true;
    error = null;
    try {
      const r = await api.search("curseforge", query, sort);
      if (seq === searchSeq) results = r;
    } catch (e) {
      if (seq === searchSeq) {
        const err = asError(e);
        error = err.message;
        api.log("warn", `search: [${err.code}] ${err.message}`);
      }
    } finally {
      if (seq === searchSeq) loading = false;
    }
  }

  // Search as you type, with a short pause so we don't spam the API.
  $effect(() => {
    void query;
    void sort;
    void hasKey;
    const t = setTimeout(runSearch, query ? 350 : 0);
    return () => clearTimeout(t);
  });

  async function addFromGitHub() {
    const input = repoInput.trim();
    const m = input.match(/^(?:https?:\/\/)?(?:www\.)?(?:github\.com\/)?([\w.-]+)\/([\w.-]+?)(?:\.git)?(?:\/.*)?$/i);
    if (!m) {
      toast("Enter a GitHub repository, like owner/name or its URL.", { kind: "error" });
      return;
    }
    addUrlOpen = false;
    repoInput = "";
    await installAddon(`gh:${m[1]}/${m[2]}`.toLowerCase(), m[2]);
  }

  async function importZip() {
    const path = await open({ multiple: false, directory: false, filters: [{ name: "Addon archive", extensions: ["zip"] }] });
    if (!path || Array.isArray(path)) return;
    importing = true;
    try {
      const v = await api.importZip(path);
      await refreshInstalled();
      toast(`${v.name} installed from zip`);
    } catch (e) {
      fail(e, "import zip");
    } finally {
      importing = false;
    }
  }
</script>

<section class="view">
  <div class="head">
    <div>
      <h2>Browse</h2>
      <div class="sub">Only addons with a Forever build are shown</div>
    </div>
    <div class="sp"></div>
    <button class="btn" onclick={() => (addUrlOpen = true)}><Icon name="link" size={15} stroke={2} /> Add from GitHub</button>
    <button class="btn" disabled={importing} onclick={importZip}><Icon name="upload" size={15} stroke={2} /> {importing ? "Importing…" : "Import .zip"}</button>
  </div>

  {#if !hasKey}
    <div class="setup">
      <h3>Connect CurseForge to browse addons</h3>
      <CurseForgeSetup />
      <p class="alt">You can still add addons from GitHub or import a .zip without a key.</p>
    </div>
  {:else}
    <label class="bigsearch">
      <Icon name="search" size={18} stroke={2} />
      <input placeholder="Search CurseForge addons" bind:value={query} />
      {#if loading}<Icon name="spinner" size={16} stroke={2} class="spin" />{/if}
    </label>
    <div class="chips">
      {#each [["popular", "Popular"], ["downloads", "Most downloaded"], ["updated", "Recently updated"]] as [s, label]}
        <button class="chip" class:on={sort === s} onclick={() => (sort = s as Sort)}>{label}</button>
      {/each}
    </div>
    <div class="grid">
      {#if error}
        <div class="empty wide">
          <h3>Couldn't load addons</h3>
          {error}
          <div style="margin-top:14px"><button class="btn" onclick={runSearch}>Try again</button></div>
        </div>
      {:else if !loading && results.length === 0}
        <div class="empty wide">No Forever addons match “{query}”.</div>
      {/if}
      {#each results as r (r.id)}
        {@const inst = installedIds.get(r.id)}
        {@const prog = app.progress[r.id]}
        <div class="card">
          <AddonIcon name={r.name} url={r.icon_url} size={44} />
          <div class="info">
            <b>{r.name}</b>
            <div class="by">{r.author}</div>
            <p>{r.summary}</p>
            <div class="meta">
              <span>↓ {formatCount(r.downloads)}</span>
              {#if r.updated}<span>Updated {timeAgo(r.updated)}</span>{/if}
            </div>
          </div>
          <div class="side-act">
            {#if prog}
              <div class="prog" style="width:96px"><div class="bar"><i style:width="{prog.pct}%"></i></div><small>{prog.pct}%</small></div>
            {:else if inst}
              {#if inst.new_version}
                <button class="btn sm primary" onclick={() => installAddon(r.id, r.name)}>Update</button>
              {:else}
                <span class="ok-txt"><Icon name="check" size={13} stroke={2.4} /> Installed</span>
              {/if}
            {:else if !r.downloadable}
              <button class="btn sm" onclick={() => openUrl(r.project_url)}>Open page <Icon name="external" size={12} /></button>
              <div class="note">Author only allows downloads from the CurseForge site</div>
            {:else}
              <button class="btn sm primary" onclick={() => installAddon(r.id, r.name)}>Install</button>
            {/if}
            <button class="page" title="Open on CurseForge" onclick={() => openUrl(r.project_url)}><Icon name="external" size={13} /></button>
          </div>
        </div>
      {/each}
    </div>
  {/if}
</section>

{#if addUrlOpen}
  <Modal title="Add from GitHub" onclose={() => (addUrlOpen = false)}>
    <p>Paste a GitHub repository that publishes releases. Kobold picks the Forever build from the latest release.</p>
    <form
      onsubmit={(e) => {
        e.preventDefault();
        addFromGitHub();
      }}
    >
      <!-- svelte-ignore a11y_autofocus -->
      <input class="field mono" style="width:100%" placeholder="https://github.com/owner/addon" bind:value={repoInput} autofocus spellcheck="false" />
      <div class="acts">
        <button type="button" class="btn ghost" onclick={() => (addUrlOpen = false)}>Cancel</button>
        <button class="btn primary" disabled={!repoInput.trim()}>Install</button>
      </div>
    </form>
  </Modal>
{/if}

<style>
  .bigsearch {
    margin: 0 22px 12px; display: flex; align-items: center; gap: 10px; height: 44px; padding: 0 14px;
    border: 1px solid var(--border); border-radius: 10px; background: var(--raised); color: var(--muted);
  }
  .bigsearch input { flex: 1; border: 0; outline: 0; background: none; font-size: 14.5px; }
  .bigsearch:focus-within { border-color: var(--accent); box-shadow: 0 0 0 3px var(--accent-soft); }
  .grid { flex: 1; overflow: auto; padding: 0 22px 18px; display: grid; grid-template-columns: repeat(auto-fill, minmax(360px, 1fr)); gap: 10px; align-content: start; }
  .wide { grid-column: 1 / -1; }
  .card { border: 1px solid var(--border); background: var(--panel); border-radius: var(--radius); padding: 14px; display: flex; gap: 12px; transition: border-color 0.15s; }
  .card:hover { border-color: var(--faint); }
  .info { flex: 1; min-width: 0; }
  .info b { font-size: 14px; font-weight: 600; }
  .by { color: var(--muted); font-size: 12px; margin-top: 1px; }
  .info p {
    margin: 7px 0 9px; font-size: 12.5px; color: var(--muted); line-height: 1.45;
    display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden;
  }
  .meta { display: flex; gap: 12px; font-size: 11.5px; color: var(--faint); }
  .side-act { display: flex; flex-direction: column; justify-content: space-between; align-items: flex-end; gap: 8px; }
  .note { font-size: 11px; color: var(--warn); max-width: 120px; text-align: right; line-height: 1.35; }
  .page { border: 0; background: none; color: var(--faint); padding: 4px; border-radius: 6px; }
  .page:hover { color: var(--text); background: var(--hover); }
  .setup { margin: 4px 22px; border: 1px solid var(--border); border-radius: var(--radius); background: var(--panel); max-width: 680px; }
  .setup h3 { margin: 0; padding: 16px 16px 0; font-family: var(--display); font-size: 16px; font-weight: 600; }
  .alt { margin: 0; padding: 12px 16px; border-top: 1px solid var(--border); color: var(--muted); font-size: 12.5px; }
</style>
