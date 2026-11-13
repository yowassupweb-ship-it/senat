<script lang="ts">
  // The macOS activity indicator: twelve rounded bars fading round, turned a step at a time. `fill` centres it in the
  // place that is loading; otherwise it sits inline (a folder opening in the tree).
  let { size = 20, fill = false, label = 'Загрузка' }: { size?: number; fill?: boolean; label?: string } = $props();
  const bars = Array.from({ length: 12 }, (_, i) => i);
</script>

<span class="spin" class:fill role="status" aria-label={label}>
  <svg width={size} height={size} viewBox="0 0 32 32" aria-hidden="true">
    {#each bars as i}
      <rect x="14.5" y="2" width="3" height="8.5" rx="1.5" transform="rotate({i * 30} 16 16)" opacity={0.18 + (i / 11) * 0.82} />
    {/each}
  </svg>
</span>

<style>
  .spin {
    display: inline-flex;
    color: var(--fg-2);
    line-height: 0;
  }
  .spin.fill {
    display: flex;
    flex: 1;
    align-items: center;
    justify-content: center;
    width: 100%;
    min-height: 64px;
  }
  svg {
    fill: currentColor;
    animation: turn 1s steps(12) infinite;
  }
  @keyframes turn {
    to {
      transform: rotate(360deg);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    svg {
      animation-duration: 3s;
    }
  }
</style>
