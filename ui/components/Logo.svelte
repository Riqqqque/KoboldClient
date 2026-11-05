<script lang="ts">
  let { size = 18 }: { size?: number } = $props();
</script>

<svg width={size} height={size} viewBox="0 0 24 24" aria-hidden="true">
  <path d="M12 2.5c2.6 3 4.6 5.6 4.6 8.6A4.6 4.6 0 0 1 12 15.7a4.6 4.6 0 0 1-4.6-4.6c0-1.6.7-3 1.7-4.3.3 1.3 1 2.2 2 2.6-.4-2.5.4-4.9.9-6.9Z" fill="var(--accent)" />
  <rect x="8.5" y="16.6" width="7" height="5" rx="1.4" fill="var(--text)" opacity=".85" />
</svg>
