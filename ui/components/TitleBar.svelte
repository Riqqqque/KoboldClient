<script lang="ts">
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import Logo from "./Logo.svelte";

  const win = getCurrentWindow();
</script>

<header class="titlebar" data-tauri-drag-region>
  <div class="brand" data-tauri-drag-region>
    <Logo />
    <span data-tauri-drag-region>Kobold Client</span>
  </div>
  <div class="spacer" data-tauri-drag-region></div>
  <button class="wc" aria-label="Minimize" onclick={() => win.minimize()}>
    <svg width="10" height="10"><path d="M0 5h10" stroke="currentColor" /></svg>
  </button>
  <button class="wc" aria-label="Maximize" onclick={() => win.toggleMaximize()}>
    <svg width="10" height="10"><rect x=".5" y=".5" width="9" height="9" fill="none" stroke="currentColor" /></svg>
  </button>
  <button class="wc x" aria-label="Close" onclick={() => win.close()}>
    <svg width="10" height="10"><path d="M0 0l10 10M10 0L0 10" stroke="currentColor" /></svg>
  </button>
</header>

<style>
  .titlebar {
    height: 38px; display: flex; align-items: center; padding-left: 14px; flex: none;
    border-bottom: 1px solid var(--border); background: var(--panel);
  }
  .brand { display: flex; align-items: center; gap: 8px; font-size: 13px; font-weight: 600; letter-spacing: 0.01em; }
  .spacer { flex: 1; align-self: stretch; }
  .wc { width: 46px; height: 38px; display: grid; place-items: center; background: none; border: 0; color: var(--muted); }
  .wc:hover { background: var(--hover); color: var(--text); }
  .wc.x:hover { background: #c42b1c; color: #fff; }
</style>
