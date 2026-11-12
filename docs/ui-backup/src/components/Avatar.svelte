<script lang="ts">
  let { name = '', color = '#8e8e93', size = 24 }: { name?: string; color?: string; size?: number } = $props();
</script>

<span class="av" style="--c:{color};width:{size}px;height:{size}px;font-size:{Math.round(size * 0.46)}px" aria-hidden="true">
  {(name || '?').trim().charAt(0).toUpperCase()}
</span>

<style>
  .av {
    display: inline-grid;
    place-items: center;
    flex: none;
    border-radius: 50%;
    background: color-mix(in srgb, var(--c) 16%, var(--bg));
    color: var(--c);
    font-weight: 650;
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--c) 30%, transparent);
  }
</style>
