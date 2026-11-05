<script lang="ts">
  import type { Snippet } from "svelte";
  import Icon from "./Icon.svelte";

  let {
    connected,
    connectedText,
    placeholder,
    save,
    remove,
    children,
  }: {
    connected: boolean;
    connectedText: string;
    placeholder: string;
    /** Resolves when the value was checked and saved; throws to keep the field. */
    save: (value: string) => Promise<void>;
    remove: () => Promise<void>;
    children?: Snippet;
  } = $props();

  let value = $state("");
  let saving = $state(false);

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    if (!value.trim() || saving) return;
    saving = true;
    try {
      await save(value);
      value = "";
    } catch {
      /* the caller already showed the error */
    } finally {
      saving = false;
    }
  }
</script>

<div class="sf">
  {#if connected}
    <div class="row">
      <span class="ok-txt"><Icon name="check" size={14} stroke={2.4} /> {connectedText}</span>
      <span class="muted">Stored encrypted for your Windows account.</span>
      <span class="sp"></span>
      <button class="btn sm ghost" onclick={remove}>Remove</button>
    </div>
  {:else}
    {#if children}<div class="help">{@render children()}</div>{/if}
    <form class="row" onsubmit={submit}>
      <Icon name="key" size={16} class="muted" />
      <input class="field mono grow" type="password" {placeholder} bind:value autocomplete="off" spellcheck="false" />
      <button class="btn primary" disabled={!value.trim() || saving}>{saving ? "Checking…" : "Save"}</button>
    </form>
  {/if}
</div>

<style>
  .sf { padding: 12px 16px; }
  .row { display: flex; align-items: center; gap: 10px; }
  .sp, .grow { flex: 1; }
  .help { color: var(--muted); font-size: 12.5px; line-height: 1.5; margin-bottom: 10px; }
  .help :global(.link) {
    background: none; border: 0; padding: 0; color: var(--accent); display: inline-flex; align-items: center; gap: 3px;
    font-size: 12.5px; text-decoration: underline; text-underline-offset: 2px;
  }
</style>
