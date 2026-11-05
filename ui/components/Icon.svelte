<script lang="ts">
  const PATHS: Record<string, string> = {
    box: "M21 8 12 3 3 8v8l9 5 9-5V8Z M3 8l9 5 9-5 M12 13v8",
    search: "M11 4a7 7 0 1 0 0 14 7 7 0 0 0 0-14Z M20 20l-3.5-3.5",
    sliders: "M4 7h10 M18 7h2 M4 17h4 M12 17h8 M16 5a2 2 0 1 1 0 4 2 2 0 0 1 0-4Z M10 15a2 2 0 1 1 0 4 2 2 0 0 1 0-4Z",
    download: "M12 4v11 M7.5 10.5 12 15l4.5-4.5 M5 20h14",
    trash: "M4 7h16 M10 11v6 M14 11v6 M6 7l1 13h10l1-13 M9 7V4h6v3",
    link: "M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1 M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1",
    upload: "M4 14v5h16v-5 M12 4v10 M8 10l4 4 4-4",
    info: "M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18Z M12 8v5 M12 16.5v.01",
    refresh: "M20 11a8 8 0 0 0-14.3-4.9L4 8 M4 4v4h4 M4 13a8 8 0 0 0 14.3 4.9L20 16 M20 20v-4h-4",
    external: "M14 4h6v6 M20 4l-9 9 M18 14v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h5",
    folder: "M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V7Z",
    pin: "M9 4h6l-1 6 4 3v2H6v-2l4-3-1-6Z M12 15v5",
    more: "M12 6v.01 M12 12v.01 M12 18v.01",
    check: "M5 12.5l4.5 4.5L19 7.5",
    x: "M6 6l12 12 M18 6 6 18",
    key: "M14 10a4 4 0 1 0-1.2 2.8L20 20 M17 17l2-2 M15 15l1.5-1.5",
    spinner: "M12 3a9 9 0 1 0 9 9",
  };
  let { name, size = 16, stroke = 1.8, class: cls = "" }: { name: string; size?: number; stroke?: number; class?: string } = $props();
</script>

<svg class={cls} width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width={stroke} stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
  {#each (PATHS[name] ?? "").split(" M") as d, i}
    <path d={i === 0 ? d : "M" + d} />
  {/each}
</svg>
