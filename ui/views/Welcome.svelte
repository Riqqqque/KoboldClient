<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import { api, type GameInfo } from "../lib/api";
  import { fail } from "../lib/store.svelte";
  import Logo from "../components/Logo.svelte";

  let { ondone }: { ondone: () => void } = $props();

  let searching = $state(true);
  let found = $state<GameInfo[]>([]);
  let picked = $state(0);
  let saving = $state(false);

  $effect(() => {
    api
      .detectGame()
      .then((g) => (found = g))
      .catch((e) => fail(e, "detect game"))
      .finally(() => (searching = false));
  });

  async function use(path: string) {
    saving = true;
    try {
      await api.setGameRoot(path);
      ondone();
    } catch (e) {
      fail(e, "set game folder");
      saving = false;
    }
  }

  async function choose() {
    const path = await open({ directory: true, multiple: false, title: "Pick your World of Warcraft or _forever_ folder" });
    if (path && !Array.isArray(path)) await use(path);
  }
</script>

<div class="welcome">
  <div class="ob">
    <Logo size={56} />
    <h2>Welcome to Kobold Client</h2>
    {#if searching}
      <p>Looking for World of Warcraft: Forever…</p>
      <div class="found"><span class="pulse"></span><div class="l"><b>Searching your drives</b><small>This only takes a moment</small></div></div>
    {:else if found.length}
      <p>You no take candle. We found your game.</p>
      {#each found as g, i}
        <button class="found" class:on={picked === i} onclick={() => (picked = i)}>
          <span class="dot"></span>
          <div class="l">
            <b>World of Warcraft: Forever{g.version ? ` ${g.version.split(".").slice(0, 3).join(".")}` : ""}</b>
            <small>{g.root}</small>
          </div>
        </button>
      {/each}
      <div class="acts">
        <button class="btn primary" disabled={saving} onclick={() => use(found[picked].root)}>Use this folder</button>
        <button class="btn ghost" onclick={choose}>Choose another…</button>
      </div>
    {:else}
      <p>Couldn't find WoW Forever automatically. Point Kobold at your <b>World of Warcraft</b> folder (the one that contains <code>_forever_</code>).</p>
      <div class="acts">
        <button class="btn primary" onclick={choose}>Choose folder…</button>
      </div>
    {/if}
  </div>
</div>

<style>
  .welcome { flex: 1; display: grid; place-items: center; }
  .ob { width: 460px; text-align: center; }
  h2 { font-family: var(--display); font-size: 24px; margin: 16px 0 6px; font-weight: 600; }
  p { color: var(--muted); font-size: 13.5px; margin: 0 0 20px; line-height: 1.5; }
  code { font-family: var(--mono); font-size: 12px; }
  .found {
    width: 100%; border: 1px solid var(--border); background: var(--panel); border-radius: var(--radius); padding: 14px 16px;
    text-align: left; display: flex; gap: 12px; align-items: center; margin-bottom: 10px;
  }
  button.found.on { border-color: var(--accent); box-shadow: 0 0 0 3px var(--accent-soft); }
  .l { flex: 1; min-width: 0; }
  .l b { font-size: 13.5px; }
  .l small { display: block; font-family: var(--mono); color: var(--muted); font-size: 11.5px; margin-top: 3px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .acts { display: flex; gap: 8px; justify-content: center; margin-top: 8px; }
  .dot, .pulse { width: 8px; height: 8px; border-radius: 50%; background: var(--ok); flex: none; box-shadow: 0 0 0 3px color-mix(in srgb, var(--ok) 20%, transparent); }
  .pulse { background: var(--accent); animation: pulse 1s ease-in-out infinite alternate; }
  @keyframes pulse { to { opacity: 0.3; } }
</style>
