<script lang="ts">
  import { api, type Backup } from "../lib/api";
  import { app, fail, refreshInstalled, toast } from "../lib/store.svelte";
  import Icon from "./Icon.svelte";
  import Modal from "./Modal.svelte";

  let backups = $state<Backup[]>([]);
  let busy = $state(false);
  let restoring = $state<Backup | null>(null);
  let showAll = $state(false);
  const running = $derived(app.state?.game?.running ?? false);
  const shown = $derived(showAll ? backups : backups.slice(0, 5));

  const KIND = { auto: "Before an update", manual: "Made by you", restore: "Before a restore" } as const;

  async function load() {
    try {
      backups = await api.listBackups();
    } catch (e) {
      fail(e, "list backups");
    }
  }
  $effect(() => {
    load();
  });

  function when(secs: number) {
    return new Date(secs * 1000).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
  }
  function size(bytes: number) {
    return bytes >= 1024 * 1024 ? `${(bytes / 1024 / 1024).toFixed(1)} MB` : `${Math.max(1, Math.round(bytes / 1024))} KB`;
  }

  async function backupNow() {
    busy = true;
    try {
      const b = await api.backupNow();
      await load();
      toast(`Settings backed up (${size(b.bytes)})`);
    } catch (e) {
      fail(e, "backup now");
    } finally {
      busy = false;
    }
  }

  async function restore() {
    const b = restoring;
    if (!b) return;
    busy = true;
    try {
      await api.restoreBackup(b.name);
      restoring = null;
      await load();
      await refreshInstalled();
      toast(`Addon settings restored to ${when(b.created)}. Your settings from before are kept as a backup.`, { ms: 7000 });
    } catch (e) {
      fail(e, "restore backup");
    } finally {
      busy = false;
    }
  }

  async function remove(b: Backup) {
    try {
      await api.deleteBackup(b.name);
      await load();
    } catch (e) {
      fail(e, "delete backup");
    }
  }
</script>

<div class="set">
  <div class="l">
    <b>Back up now</b>
    <small>Saves the whole WTF folder: addon settings, keybinds, macros and game options.</small>
  </div>
  <button class="btn sm" onclick={() => api.openFolder("backups")}><Icon name="folder" size={14} /> Open folder</button>
  <button class="btn sm" disabled={busy} onclick={backupNow}><Icon name="save" size={14} /> {busy && !restoring ? "Backing up…" : "Back up now"}</button>
</div>
{#if backups.length === 0}
  <div class="set"><div class="l"><small>No backups yet. One is made automatically the next time an addon updates.</small></div></div>
{:else}
  {#each shown as b (b.name)}
    <div class="set bk">
      <Icon name="history" size={16} class="muted" />
      <div class="l">
        <b>{when(b.created)}</b>
        <small>{KIND[b.kind]} · {size(b.bytes)}</small>
      </div>
      <button class="btn sm" disabled={busy} onclick={() => (restoring = b)}>Restore</button>
      <button class="icon-btn" title="Delete this backup" onclick={() => remove(b)}><Icon name="trash" size={15} /></button>
    </div>
  {/each}
  {#if backups.length > 5}
    <div class="set more"><button class="btn sm ghost" onclick={() => (showAll = !showAll)}>{showAll ? "Show fewer" : `Show all ${backups.length}`}</button></div>
  {/if}
{/if}

{#if restoring}
  <Modal title="Restore addon settings?" onclose={() => !busy && (restoring = null)}>
    <p>
      This puts the WTF folder back the way it was on <b>{when(restoring.created)}</b>. Settings changed since then are replaced,
      but Kobold backs up the current folder first, so you can switch back.
    </p>
    {#if running}
      <p class="warn">Close WoW Forever first. The game saves its settings when it exits and would overwrite the restore.</p>
    {/if}
    <div class="acts">
      <button class="btn ghost" disabled={busy} onclick={() => (restoring = null)}>Cancel</button>
      <button class="btn primary" disabled={busy || running} onclick={restore}>{busy ? "Restoring…" : "Restore"}</button>
    </div>
  </Modal>
{/if}

<style>
  .bk { gap: 10px; }
  .more { justify-content: center; }
  .icon-btn { width: 28px; height: 28px; border-radius: 7px; border: 0; background: none; color: var(--faint); display: grid; place-items: center; }
  .icon-btn:hover { background: var(--danger-soft); color: var(--danger); }
  .warn { color: var(--warn) !important; }
</style>
