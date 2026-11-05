<script lang="ts">
  import { app, dismissToast } from "../lib/store.svelte";
</script>

<div class="toasts" aria-live="polite">
  {#each app.toasts as t (t.id)}
    <div class="toast" class:error={t.kind === "error"}>
      <span class="selectable">{t.message}</span>
      {#if t.action}
        <button
          onclick={() => {
            t.action!.run();
            dismissToast(t.id);
          }}>{t.action.label}</button>
      {/if}
      <button class="x" aria-label="Dismiss" onclick={() => dismissToast(t.id)}>✕</button>
    </div>
  {/each}
</div>

<style>
  .toasts {
    position: fixed; bottom: 18px; left: 50%; transform: translateX(-50%); display: flex; flex-direction: column;
    gap: 8px; align-items: center; z-index: 60; pointer-events: none; width: max-content; max-width: 70vw;
  }
  .toast {
    pointer-events: auto; background: var(--text); color: var(--bg); padding: 9px 8px 9px 16px; border-radius: 9px;
    display: flex; align-items: center; gap: 10px; font-size: 13px; box-shadow: 0 10px 30px rgba(0, 0, 0, 0.35);
    animation: up 0.2s ease-out; line-height: 1.4;
  }
  .toast.error { background: var(--danger); color: #fff; }
  .toast button { background: none; border: 0; color: var(--accent); font-weight: 600; font-size: 13px; padding: 4px 6px; white-space: nowrap; }
  :global([data-theme="parchment"]) .toast button { color: #f0b48f; }
  .toast.error button { color: #fff; text-decoration: underline; }
  .toast .x { opacity: 0.6; font-weight: 400; text-decoration: none !important; }
  @keyframes up { from { transform: translateY(12px); opacity: 0; } }
</style>
