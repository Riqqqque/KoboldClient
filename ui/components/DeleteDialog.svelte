<script lang="ts">
  import { api, type AddonView, type DeletePreview } from "../lib/api";
  import { app, fail, joinNames, refreshInstalled, toast } from "../lib/store.svelte";
  import Icon from "./Icon.svelte";
  import Modal from "./Modal.svelte";

  let { addon, onclose }: { addon: AddonView; onclose: () => void } = $props();

  let preview = $state<DeletePreview | null>(null);
  let deleteSettings = $state(false);
  let busy = $state(false);

  $effect(() => {
    api.deletePreview(addon.id).then(
      (p) => (preview = p),
      (e) => {
        fail(e, "delete preview");
        onclose();
      },
    );
  });

  async function confirm() {
    busy = true;
    try {
      const removed = await api.remove(addon.id, deleteSettings);
      onclose();
      await refreshInstalled();
      toast(`${removed.name} moved to the Recycle Bin`, {
        action: {
          label: "Undo",
          run: async () => {
            try {
              await api.undoRemove(removed.token);
              await refreshInstalled();
              toast(`${removed.name} restored`);
            } catch (e) {
              fail(e, "undo remove");
            }
          },
        },
      });
    } catch (e) {
      fail(e, "remove");
      busy = false;
    }
  }
</script>

<Modal title="Remove {addon.name}?" {onclose}>
  <p>These folders will be moved to the Recycle Bin. You can restore them from there or with Undo.</p>
  {#if preview}
    <div class="folders mono">
      {#each preview.folders as f}
        <div><Icon name="folder" size={14} /> Interface\AddOns\{f}</div>
      {/each}
      {#each preview.kept as [f, other]}
        <div class="keep"><Icon name="folder" size={14} /> {f} <em>Kept, also used by {other}</em></div>
      {/each}
    </div>
    {#if preview.needed_by.length}
      <p class="warn">{joinNames(preview.needed_by)} {preview.needed_by.length > 1 ? "need" : "needs"} this addon and won't work without it.</p>
    {/if}
    {#if preview.saved_variables > 0}
      <label class="check">
        <input type="checkbox" bind:checked={deleteSettings} />
        Also delete its saved settings ({preview.saved_variables} file{preview.saved_variables > 1 ? "s" : ""})
      </label>
    {/if}
    {#if app.state?.game?.running}
      <p class="warn">WoW is running. Type <b>/reload</b> in game afterwards.</p>
    {/if}
  {:else}
    <div class="folders"><div class="muted">Checking…</div></div>
  {/if}
  <div class="acts">
    <button class="btn ghost" onclick={onclose}>Cancel</button>
    <button class="btn danger" disabled={!preview || busy} onclick={confirm}>{busy ? "Removing…" : "Remove"}</button>
  </div>
</Modal>

<style>
  .folders { border: 1px solid var(--border); border-radius: 8px; background: var(--raised); padding: 6px 0; margin-bottom: 12px; max-height: 200px; overflow: auto; }
  .folders div { display: flex; align-items: center; gap: 8px; padding: 5px 12px; }
  .folders .keep { color: var(--faint); }
  .folders .keep em { font-family: var(--font); font-style: normal; margin-left: auto; font-size: 11.5px; }
  .check { display: flex; align-items: center; gap: 9px; font-size: 13px; cursor: pointer; }
  .check input { accent-color: var(--accent); width: 15px; height: 15px; }
  .warn { color: var(--warn) !important; margin: 10px 0 12px !important; font-size: 12.5px !important; }
</style>
