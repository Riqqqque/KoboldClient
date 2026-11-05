<script lang="ts">
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { api } from "../lib/api";
  import { app, fail, refreshState, toast } from "../lib/store.svelte";
  import Icon from "./Icon.svelte";

  let { compact = false }: { compact?: boolean } = $props();

  const CONSOLE = "https://console.curseforge.com/";
  const DOCS = "https://docs.curseforge.com/rest-api/#getting-started";

  let key = $state("");
  let saving = $state(false);
  const connected = $derived(app.state?.has_cf_key ?? false);

  async function save() {
    saving = true;
    try {
      await api.saveCfKey(key);
      key = "";
      await refreshState();
      toast("CurseForge connected");
    } catch (e) {
      fail(e, "save CurseForge key");
    } finally {
      saving = false;
    }
  }

  async function disconnect() {
    await api.clearCfKey();
    await refreshState();
    toast("CurseForge key removed");
  }
</script>

<div class="cf" class:compact>
  {#if connected}
    <div class="row">
      <span class="ok-txt"><Icon name="check" size={14} stroke={2.4} /> CurseForge is connected</span>
      <span class="muted">The key is stored encrypted for your Windows account.</span>
      <span class="sp"></span>
      <button class="btn sm ghost" onclick={disconnect}>Remove key</button>
    </div>
  {:else}
    {#if !compact}
      <p class="lead">CurseForge needs a free API key before Kobold can search and download from it. It takes about two minutes:</p>
    {/if}
    <ol>
      <li>
        <b>Open the CurseForge for Studios console</b> and sign in with your CurseForge account (or create one, it's free).
        <button class="link" onclick={() => openUrl(CONSOLE)}>console.curseforge.com <Icon name="external" size={12} /></button>
      </li>
      <li>
        <b>Open “API keys”</b> in the console and copy your key. If no key is offered, CurseForge's
        <button class="link" onclick={() => openUrl(DOCS)}>getting-started page <Icon name="external" size={12} /></button>
        links the short form to request one.
      </li>
      <li><b>Paste the key here</b> and press Connect. Kobold checks it with CurseForge first.</li>
    </ol>
    <form
      class="row"
      onsubmit={(e) => {
        e.preventDefault();
        if (key.trim()) save();
      }}
    >
      <Icon name="key" size={16} class="muted" />
      <input class="field mono grow" type="password" placeholder="Paste your CurseForge API key" bind:value={key} autocomplete="off" spellcheck="false" />
      <button class="btn primary" disabled={!key.trim() || saving}>{saving ? "Checking…" : "Connect"}</button>
    </form>
  {/if}
</div>

<style>
  .cf { padding: 14px 16px; }
  .lead { margin: 0 0 10px; color: var(--muted); line-height: 1.5; }
  ol { margin: 0 0 14px; padding-left: 20px; line-height: 1.55; color: var(--muted); font-size: 13px; }
  ol li { margin-bottom: 6px; }
  ol b { color: var(--text); font-weight: 600; }
  .row { display: flex; align-items: center; gap: 10px; }
  .row .sp { flex: 1; }
  .grow { flex: 1; }
  .link {
    background: none; border: 0; padding: 0; color: var(--accent); display: inline-flex; align-items: center; gap: 3px;
    font-size: 13px; text-decoration: underline; text-underline-offset: 2px;
  }
</style>
