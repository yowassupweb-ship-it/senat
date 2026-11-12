<script lang="ts">
  // The mark of Senate: the rotunda. Three arcs of seats around the one who speaks. One tone, no gradient.
  let { size = 26 }: { size?: number; id?: string } = $props();
</script>

<svg class="emblem" width={size} height={size} viewBox="0 0 64 64" role="img" aria-label="Senate">
  <rect x="0.75" y="0.75" width="62.5" height="62.5" rx="13.5" fill="none" stroke="currentColor" stroke-opacity="0.35" stroke-width="1.5" />
  <g fill="none" stroke="currentColor" stroke-linecap="round">
    <path d="M12 42 A20 20 0 0 1 52 42" stroke-width="3.2" />
    <path d="M19 42 A13 13 0 0 1 45 42" stroke-width="3.2" stroke-opacity="0.72" />
    <path d="M26 42 A6 6 0 0 1 38 42" stroke-width="3.2" stroke-opacity="0.48" />
  </g>
  <circle cx="32" cy="49" r="3.2" fill="currentColor" />
</svg>

<style>
  .emblem {
    display: block;
    flex: none;
    color: var(--fg);
  }
</style>
