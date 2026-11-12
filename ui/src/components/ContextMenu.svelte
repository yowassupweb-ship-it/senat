<script lang="ts">
  import { tick } from 'svelte';
  import { closeMenu, menu } from '../lib/contextmenu.svelte';
  import Icon from './Icon.svelte';

  let el = $state<HTMLDivElement>();
  let pos = $state({ x: 0, y: 0 });

  $effect(() => {
    if (!menu.open) return;
    pos = { x: menu.x, y: menu.y };
    tick().then(() => {
      if (!el) return;
      const r = el.getBoundingClientRect();
      pos = { x: Math.max(8, Math.min(menu.x, window.innerWidth - r.width - 8)), y: Math.max(8, Math.min(menu.y, window.innerHeight - r.height - 8)) };
      el.querySelector<HTMLButtonElement>('button:not(:disabled)')?.focus();
    });
  });

  function key(e: KeyboardEvent) {
    if (!menu.open) return;
    if (e.key === 'Escape') {
      e.preventDefault();
      closeMenu();
      return;
    }
    if (!['ArrowDown', 'ArrowUp'].includes(e.key) || !el) return;
    e.preventDefault();
    const b = Array.from(el.querySelectorAll<HTMLButtonElement>('button:not(:disabled)'));
    const i = b.indexOf(document.activeElement as HTMLButtonElement);
    b[(i + (e.key === 'ArrowDown' ? 1 : -1) + b.length) % b.length]?.focus();
  }
  async function pick(run?: () => unknown) {
    closeMenu();
    await run?.();
  }
</script>

<svelte:window onkeydown={key} onmousedown={(e) => { if (menu.open && el && !el.contains(e.target as Node)) closeMenu(); }} onblur={closeMenu} onresize={closeMenu} onwheel={closeMenu} />

{#if menu.open}
  <div class="cm" bind:this={el} role="menu" tabindex="-1" style="left:{pos.x}px;top:{pos.y}px" oncontextmenu={(e) => e.preventDefault()}>
    {#each menu.items as it}
      {#if it.sep}
        <div class="sep" role="separator"></div>
      {:else}
        <button role="menuitem" class:danger={it.danger} disabled={it.disabled} onclick={() => pick(it.run)}>
          <span class="ic">{#if it.icon}<Icon name={it.icon} size={14} />{/if}</span>
          <span class="lb">{it.label}</span>
          {#if it.hint}<span class="ht">{it.hint}</span>{/if}
        </button>
      {/if}
    {/each}
  </div>
{/if}

<style>
  .cm { position: fixed; z-index: 200; min-width: 200px; padding: 6px; border-radius: 12px; background: var(--bg-raised); box-shadow: var(--shadow-2), inset 0 0 0 1px var(--line-strong); display: flex; flex-direction: column; }
  button { display: flex; align-items: center; gap: 10px; padding: 7px 10px; border-radius: 8px; font-size: 13.5px; text-align: left; color: var(--fg); }
  button:hover:not(:disabled), button:focus-visible { background: var(--bg-hover); outline: none; }
  button:disabled { color: var(--fg-3); cursor: default; }
  .danger { color: var(--bad); }
  .ic { width: 16px; display: grid; place-items: center; color: var(--fg-2); flex: none; }
  .danger .ic { color: var(--bad); }
  .lb { flex: 1; white-space: nowrap; }
  .ht { color: var(--fg-3); font-size: 12px; margin-left: 18px; }
  .sep { height: 1px; margin: 5px 4px; background: var(--line); }
</style>
