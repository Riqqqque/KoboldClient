<script lang="ts">
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { api, type AddonErrors, type ErrorGroup } from "../lib/api";
  import { app, dismissErrors, fail, installAddon, refreshErrors, refreshInstalled, toast } from "../lib/store.svelte";
  import Icon from "./Icon.svelte";
  import Modal from "./Modal.svelte";

  let { onclose }: { onclose: () => void } = $props();
  const report = $derived(app.errors);
  const running = $derived(app.state?.game?.running ?? false);
  let busy = $state<string | null>(null);

  function groupText(g: ErrorGroup) {
    return [`${g.message}${g.count > 1 ? `  (x${g.count})` : ""}`, ...g.stack].join("\n");
  }

  async function copyAll() {
    if (!report) return;
    const parts = report.addons.map((a) => `== ${a.name} (${a.folder}) ==\n${a.groups.map(groupText).join("\n\n")}`);
    if (report.other.length) parts.push(`== Not tied to an addon ==\n${report.other.map(groupText).join("\n\n")}`);
    try {
      await navigator.clipboard.writeText(parts.join("\n\n"));
      toast("Errors copied. Paste them into a bug report for the addon's author.");
    } catch (e) {
      fail(e, "copy errors");
    }
  }

  async function disable(a: AddonErrors) {
    if (!a.id) return;
    busy = a.id;
    try {
      await api.setEnabled(a.id, false);
      await refreshInstalled();
      await refreshErrors();
      toast(`${a.name} disabled${running ? ". Type /reload in game." : ""}`);
    } catch (e) {
      fail(e, "disable from errors");
    } finally {
      busy = null;
    }
  }

  async function update(a: AddonErrors) {
    if (!a.id) return;
    busy = a.id;
    await installAddon(a.id, a.name);
    await refreshErrors();
    busy = null;
  }

  async function dismiss() {
    await dismissErrors();
    onclose();
  }
</script>

<Modal title="Lua errors {running ? 'this session' : 'last session'}" {onclose} width={640}>
  {#if !report || (report.total === 0 && report.other.length === 0)}
    <p>No Lua errors in the game's log{report?.started ? ` since ${report.started}` : ""}.</p>
    <div class="acts"><button class="btn" onclick={onclose}>Close</button></div>
  {:else}
    <p>
      From the game's log{report.started ? `, session started ${report.started}` : ""}. The game starts a new log each time it launches.
      {#if running}Errors keep coming in while you play; this list refreshes when you reopen it.{/if}
    </p>
    <div class="list">
      {#each report.addons as a (a.id ?? a.folder)}
        <div class="addon">
          <div class="top">
            <b>{a.name}</b>
            <span class="tag danger">{a.count} error{a.count > 1 ? "s" : ""}</span>
            {#if !a.enabled}<span class="tag mute">Disabled</span>{/if}
            <span class="sp"></span>
            {#if a.id && a.new_version}
              <button class="btn sm primary" disabled={busy === a.id} onclick={() => update(a)}>Update to {a.new_version}</button>
            {/if}
            {#if a.id && a.enabled}
              <button class="btn sm" disabled={busy === a.id} onclick={() => disable(a)}>Disable</button>
            {/if}
            {#if a.project_url}
              <button class="btn sm ghost" title="Open the addon's page to report the error" onclick={() => openUrl(a.project_url!)}><Icon name="external" size={13} /></button>
            {/if}
          </div>
          {#if a.groups.some((g) => g.blame === "taint")}
            <div class="hint">
              <Icon name="info" size={14} />
              <span>The game's own interface failed because {a.name} touched protected code. It's often harmless, but if something stops working, update or disable {a.name}.</span>
            </div>
          {/if}
          {#each a.groups as g}
            <details class="err">
              <summary>
                <span class="msg mono">{g.message}</span>
                <span class="when">{g.count > 1 ? `${g.count}× · ${g.first} – ${g.last.split(" ")[1] ?? g.last}` : g.first}</span>
              </summary>
              {#if g.stack.length}<pre class="mono selectable">{g.stack.join("\n")}</pre>{/if}
            </details>
          {/each}
        </div>
      {/each}
      {#if report.other.length}
        <div class="addon">
          <div class="top"><b class="muted">Not tied to an addon</b><span class="tag mute">{report.other.reduce((n, g) => n + g.count, 0)}</span></div>
          <div class="hint"><Icon name="info" size={14} /><span>Usually macros, /run scripts or the game itself.</span></div>
          {#each report.other as g}
            <details class="err">
              <summary><span class="msg mono">{g.message}</span><span class="when">{g.count > 1 ? `${g.count}×` : g.first}</span></summary>
              {#if g.stack.length}<pre class="mono selectable">{g.stack.join("\n")}</pre>{/if}
            </details>
          {/each}
        </div>
      {/if}
    </div>
    <div class="acts">
      <button class="btn ghost" onclick={copyAll}><Icon name="copy" size={14} /> Copy errors</button>
      <span class="sp"></span>
      {#if report.total && !report.seen}<button class="btn" onclick={dismiss}>Dismiss</button>{/if}
      <button class="btn primary" onclick={onclose}>Close</button>
    </div>
  {/if}
</Modal>

<style>
  .list { max-height: min(460px, calc(100vh - 230px)); overflow: auto; display: flex; flex-direction: column; gap: 10px; padding-right: 2px; }
  .addon { border: 1px solid var(--border); border-radius: 9px; background: var(--raised); padding: 10px 12px; }
  .top { display: flex; align-items: center; gap: 8px; }
  .top b { font-size: 13.5px; }
  .sp { flex: 1; }
  .hint { display: flex; gap: 7px; align-items: flex-start; color: var(--muted); font-size: 12px; line-height: 1.45; margin-top: 8px; }
  .hint :global(svg) { margin-top: 1px; color: var(--accent); }
  .err { margin-top: 8px; border-top: 1px solid color-mix(in srgb, var(--border) 70%, transparent); padding-top: 7px; }
  .err summary { display: flex; gap: 10px; align-items: baseline; cursor: pointer; list-style: none; }
  .err summary::-webkit-details-marker { display: none; }
  .msg { flex: 1; min-width: 0; font-size: 11.5px; line-height: 1.45; word-break: break-word; user-select: text; }
  .when { flex: none; font-size: 11px; color: var(--faint); white-space: nowrap; }
  pre { margin: 7px 0 0; padding: 8px 10px; background: var(--bg); border-radius: 6px; font-size: 11px; line-height: 1.5; color: var(--muted); white-space: pre-wrap; word-break: break-all; max-height: 180px; overflow: auto; }
  .acts .sp { flex: 1; }
</style>
