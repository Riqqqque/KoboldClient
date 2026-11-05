<script lang="ts">
  import type { Snippet } from "svelte";
  let { title, onclose, children, width = 420 }: { title: string; onclose: () => void; children: Snippet; width?: number } = $props();

  function onkey(e: KeyboardEvent) {
    if (e.key === "Escape") onclose();
  }
</script>

<svelte:window onkeydown={onkey} />

<div class="scrim" role="presentation" onclick={(e) => e.target === e.currentTarget && onclose()}>
  <div class="modal" role="dialog" aria-modal="true" aria-label={title} style:width="{width}px">
    <h3>{title}</h3>
    {@render children()}
  </div>
</div>

<style>
  .scrim {
    position: fixed; inset: 0; background: rgba(0, 0, 0, 0.45); display: grid; place-items: center;
    z-index: 50; backdrop-filter: blur(2px); animation: fade 0.12s ease-out;
  }
  .modal {
    max-width: calc(100vw - 40px); background: var(--panel); border: 1px solid var(--border); border-radius: 12px;
    box-shadow: var(--shadow); padding: 20px; animation: pop 0.16s ease-out;
  }
  h3 { margin: 0 0 8px; font-family: var(--display); font-size: 17px; font-weight: 600; }
  @keyframes pop { from { transform: scale(0.97); opacity: 0; } }
  @keyframes fade { from { opacity: 0; } }
  :global(.modal p) { margin: 0 0 12px; color: var(--muted); font-size: 13px; line-height: 1.5; }
  :global(.modal .acts) { display: flex; justify-content: flex-end; gap: 8px; margin-top: 16px; }
</style>
