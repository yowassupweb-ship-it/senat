<script lang="ts">
  // "Who has access": every specialist as a toggle. Used by MCP, API and the sheet's equipment slots.
  import { app } from '../lib/store.svelte';
  import Avatar from './Avatar.svelte';

  let { selected, shared = false, onchange, disabled = false }: { selected: string[]; shared?: boolean; onchange: (ids: string[]) => void; disabled?: boolean } = $props();

  function toggle(id: string) {
    if (disabled || shared) return;
    onchange(selected.includes(id) ? selected.filter((x) => x !== id) : [...selected, id]);
  }
</script>

<div class="chips" role="group" aria-label="Доступ специалистов">
  {#each app.agents as a (a.id)}
    {@const on = shared || selected.includes(a.id)}
    <button class="c" class:on class:locked={shared} {disabled} aria-pressed={on} title={shared ? 'Доступно всем' : on ? 'Доступ есть — нажмите, чтобы убрать' : 'Нажмите, чтобы выдать доступ'} onclick={() => toggle(a.id)}>
      <Avatar name={a.name} color={a.accent} size={18} />{a.name}
    </button>
  {/each}
</div>

<style>
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .c {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 26px;
    padding: 0 10px 0 4px;
    border-radius: 13px;
    font-size: 13px;
    color: var(--fg-3);
    background: transparent;
    box-shadow: inset 0 0 0 1px var(--line-strong);
    transition: background 0.12s var(--ease);
  }
  .c :global(.av) {
    opacity: 0.55;
  }
  .c:hover:not(:disabled):not(.locked) {
    background: var(--bg-hover);
  }
  .c.on {
    color: var(--fg);
    background: var(--accent-soft);
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--accent) 55%, transparent);
  }
  .c.on :global(.av) {
    opacity: 1;
  }
  .c.locked {
    cursor: default;
  }
</style>
