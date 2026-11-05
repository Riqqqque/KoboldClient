<script lang="ts">
  import { open, save } from "@tauri-apps/plugin-dialog";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { api, type ImportPreview } from "../lib/api";
  import { app, fail, importAddons, SOURCE_LABEL, toast } from "../lib/store.svelte";
  import Icon from "./Icon.svelte";
  import Modal from "./Modal.svelte";

  let preview = $state<ImportPreview | null>(null);

  async function exportList() {
    const date = new Date().toISOString().slice(0, 10);
    const path = await save({
      title: "Save your addon list",
      defaultPath: `kobold-addons-${date}.json`,
      filters: [{ name: "Kobold addon list", extensions: ["json"] }],
    });
    if (!path) return;
    try {
      const n = await api.exportAddonList(path);
      toast(`Saved ${n} addon${n === 1 ? "" : "s"} to ${path.split(/[\\/]/).pop()}`);
    } catch (e) {
      fail(e, "export addon list");
    }
  }

  async function importList() {
    const path = await open({ multiple: false, directory: false, filters: [{ name: "Kobold addon list", extensions: ["json"] }] });
    if (!path || Array.isArray(path)) return;
    try {
      preview = await api.previewAddonList(path);
    } catch (e) {
      fail(e, "read addon list");
    }
  }

  function start() {
    const entries = preview!.to_install;
    preview = null;
    app.view = "installed";
    importAddons(entries);
  }

  const needsCf = $derived(!app.state?.has_cf_key && !!preview?.to_install.some((e) => e.id.startsWith("cf:")));
  const needsWago = $derived(!app.state?.has_wago_key && !!preview?.to_install.some((e) => e.id.startsWith("wago:")));
</script>

<div class="set">
  <div class="l">
    <b>Save your addon list</b>
    <small>A small file you can use to set up the same addons on another PC, or to share your setup.</small>
  </div>
  <button class="btn sm" disabled={!app.installed.length} onclick={exportList}><Icon name="save" size={14} /> Save list…</button>
</div>
<div class="set">
  <div class="l">
    <b>Install from a list</b>
    <small>Installs everything in a saved list that isn't here yet.</small>
  </div>
  <button class="btn sm" disabled={!!app.importing} onclick={importList}><Icon name="upload" size={14} /> Open list…</button>
</div>

{#if preview}
  <Modal title="Install from this list?" onclose={() => (preview = null)} width={520}>
    {#if preview.to_install.length}
      <p>
        Kobold will install {preview.to_install.length} addon{preview.to_install.length > 1 ? "s" : ""}, along with anything they need.
        Each gets its newest version; pinned and disabled ones are set up the same way afterwards.
      </p>
      <div class="box">
        {#each preview.to_install as e}
          <div class="it"><span>{e.name}</span><em>{e.source ? SOURCE_LABEL[e.source] : ""}{e.pinned ? " · pinned" : ""}{!e.enabled ? " · disabled" : ""}</em></div>
        {/each}
      </div>
    {:else}
      <p>Everything Kobold can install from this list is already here.</p>
    {/if}
    {#if needsCf}<p class="warn">Some of these come from CurseForge. Connect CurseForge in Settings first, or they'll fail.</p>{/if}
    {#if needsWago}<p class="warn">Some of these come from Wago. Add a Wago API key in Settings first, or they'll fail.</p>{/if}
    {#if preview.manual.length}
      <p>These were installed by hand, so you'll need to get them yourself:</p>
      <div class="box">
        {#each preview.manual as e}
          <div class="it">
            <span>{e.name}</span>
            {#if e.project_url}<button class="link" onclick={() => openUrl(e.project_url!)}>Open page <Icon name="external" size={11} /></button>{/if}
          </div>
        {/each}
      </div>
    {/if}
    {#if preview.present.length}
      <p class="small">{preview.present.length} already installed here.</p>
    {/if}
    <div class="acts">
      <button class="btn ghost" onclick={() => (preview = null)}>Cancel</button>
      <button class="btn primary" disabled={!preview.to_install.length} onclick={start}>Install{preview.to_install.length ? ` ${preview.to_install.length}` : ""}</button>
    </div>
  </Modal>
{/if}

<style>
  .box { border: 1px solid var(--border); border-radius: 8px; background: var(--raised); padding: 4px 0; margin-bottom: 12px; max-height: 190px; overflow: auto; }
  .it { display: flex; align-items: center; gap: 8px; padding: 5px 12px; font-size: 13px; }
  .it span { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .it em { font-style: normal; color: var(--faint); font-size: 11.5px; }
  .link { background: none; border: 0; padding: 0; color: var(--accent); font-size: 12px; display: inline-flex; align-items: center; gap: 3px; }
  .warn { color: var(--warn) !important; }
  .small { font-size: 12px !important; }
</style>
