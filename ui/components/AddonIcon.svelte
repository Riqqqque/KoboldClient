<script lang="ts">
  let { name, url, size = 38, dim = false }: { name: string; url: string | null; size?: number; dim?: boolean } = $props();

  const COLORS = ["#c0563a", "#3f7fc4", "#5a9a4c", "#9b59b6", "#c08a2f", "#2f9a8f", "#8a6a4a", "#b04a6a", "#4a6ab0", "#6f8f2f"];
  let failed = $state(false);
  // Reset the fallback when the image changes.
  $effect(() => {
    void url;
    failed = false;
  });

  const initials = $derived(
    name
      .replace(/[^A-Za-z0-9 _-]/g, "")
      .split(/[\s_-]+/)
      .filter(Boolean)
      .map((w) => w[0])
      .join("")
      .slice(0, 2)
      .toUpperCase() || "?",
  );
  const color = $derived.by(() => {
    let h = 0;
    for (const c of name) h = (h * 31 + c.charCodeAt(0)) >>> 0;
    return COLORS[h % COLORS.length];
  });
</script>

<div class="ic" class:dim style:width="{size}px" style:height="{size}px" style:--c={color}>
  {#if url && !failed}
    <img src={url} alt="" loading="lazy" decoding="async" referrerpolicy="no-referrer" draggable="false" onerror={() => (failed = true)} />
  {:else}
    <span style:font-size="{Math.round(size * 0.38)}px">{initials}</span>
  {/if}
</div>

<style>
  .ic {
    position: relative;
    flex: none;
    border-radius: 22%;
    overflow: hidden;
    display: grid;
    place-items: center;
    background: linear-gradient(145deg, color-mix(in srgb, var(--c) 92%, white), color-mix(in srgb, var(--c) 80%, black));
    transition: opacity 0.2s;
  }
  .ic::after {
    content: "";
    position: absolute;
    inset: 0;
    border-radius: inherit;
    box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.08), inset 0 -1px 0 rgba(0, 0, 0, 0.2);
    pointer-events: none;
  }
  .ic.dim { opacity: 0.45; filter: grayscale(0.6); }
  img { width: 100%; height: 100%; object-fit: cover; display: block; background: var(--raised); }
  span { color: #fff; font-weight: 700; letter-spacing: -0.02em; text-shadow: 0 1px 1px rgba(0, 0, 0, 0.25); }
</style>
