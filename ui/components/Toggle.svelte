<script lang="ts">
  let { on, label, disabled = false, onchange }: { on: boolean; label: string; disabled?: boolean; onchange: (v: boolean) => void } = $props();
</script>

<button class="tog" class:on role="switch" aria-checked={on} aria-label={label} title={label} {disabled} onclick={() => onchange(!on)}></button>

<style>
  .tog {
    width: 34px; height: 20px; border-radius: 99px; background: var(--raised); border: 1px solid var(--border);
    position: relative; padding: 0; transition: 0.15s; flex: none;
  }
  .tog::after {
    content: ""; position: absolute; top: 2px; left: 2px; width: 14px; height: 14px; border-radius: 50%;
    background: var(--faint); transition: 0.15s;
  }
  .tog.on { background: var(--accent); border-color: transparent; }
  .tog.on::after { left: 16px; background: var(--accent-ink); }
</style>
