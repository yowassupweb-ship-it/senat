<script lang="ts">
  // "Who has access", built for a crowd: a summary in the row (faces + how many) and one button; the full list,
  // with search, "everyone" and "nobody", opens in a panel. Used by MCP, API and the equipment slots.
  import { app } from '../lib/store.svelte';
  import { roleLabel } from '../lib/format';
  import Avatar from './Avatar.svelte';
  import Icon from './Icon.svelte';

  let { selected, shared = false, onchange, disabled = false }: { selected: string[]; shared?: boolean; onchange: (ids: string[]) => void; disabled?: boolean } = $props();

  const total = $derived(app.agents.length);
  const holders = $derived(shared ? app.agents : app.agents.filter((a) => selected.includes(a.id)));
  const SHOW = 5;

  let open = $state(false);
  let q = $state('');
  let btn = $state<HTMLButtonElement>();
  let pos = $state({ top: 0, left: 0, up: false });
  let panel = $state<HTMLDivElement>();

  const shown = $derived(app.agents.filter((a) => !q.trim() || (a.name + ' ' + (roleLabel(a) ?? '') + ' ' + a.slug).toLowerCase().includes(q.trim().toLowerCase())));

  function toggleOpen() {
    if (disabled || shared) return;
    open = !open;
    q = '';
    if (open && btn) {
      const r = btn.getBoundingClientRect();
      const up = r.bottom + 360 > window.innerHeight && r.top > 360;
      pos = { top: up ? r.top - 6 : r.bottom + 6, left: Math.max(8, Math.min(r.left, window.innerWidth - 320)), up };
    }
  }
  function toggle(id: string) {
    onchange(selected.includes(id) ? selected.filter((x) => x !== id) : [...selected, id]);
  }
  function all() {
    onchange(app.agents.map((a) => a.id));
  }
  function none() {
    onchange([]);
  }
  function onWindowPointer(e: Event) {
    if (!open) return;
    const t = e.target as Node;
    if (panel?.contains(t) || btn?.contains(t)) return;
    open = false;
  }
  function onWindowKey(e: KeyboardEvent) {
    if (open && e.key === 'Escape') {
      open = false;
      btn?.focus();
    }
  }
  $effect(() => {
    if (open) panel?.querySelector<HTMLInputElement>('input')?.focus();
  });
</script>

<svelte:window onpointerdown={onWindowPointer} onkeydown={onWindowKey} onresize={() => (open = false)} />

<div class="who">
  <span class="stack" aria-hidden="true">
    {#each holders.slice(0, SHOW) as a (a.id)}<span class="f"><Avatar name={a.name} color={a.accent} size={22} /></span>{/each}
    {#if holders.length > SHOW}<span class="more">+{holders.length - SHOW}</span>{/if}
  </span>
  <span class="sum">{#if shared}Всем специалистам{:else if !holders.length}Никому не выдан{:else if holders.length === total && total > 1}Всем ({total}){:else if holders.length <= 2}{holders.map((a) => a.name).join(', ')}{:else}Выдан {holders.length} из {total}{/if}</span>
  {#if !shared}
    <button class="edit" type="button" bind:this={btn} onclick={toggleOpen} {disabled} aria-haspopup="dialog" aria-expanded={open}>Кому выдан<Icon name="down" size={12} /></button>
  {/if}
</div>

{#if open}
  <div class="panel" role="dialog" aria-label="Кому выдан доступ" bind:this={panel} style="left:{pos.left}px;{pos.up ? `bottom:${window.innerHeight - pos.top}px` : `top:${pos.top}px`}">
    <div class="search"><Icon name="search" size={14} /><input type="search" placeholder="Найти специалиста" bind:value={q} aria-label="Найти специалиста" /></div>
    <div class="bulk"><button type="button" class="lnk" onclick={all}>Выбрать всех</button><span class="faint">·</span><button type="button" class="lnk" onclick={none}>Снять всех</button><span class="grow"></span><span class="faint">{holders.length} из {total}</span></div>
    <ul>
      {#each shown as a (a.id)}
        {@const on = selected.includes(a.id)}
        <li>
          <label class:on>
            <input type="checkbox" checked={on} onchange={() => toggle(a.id)} />
            <Avatar name={a.name} color={a.accent} size={22} />
            <span class="nm"><b>{a.name}</b>{#if roleLabel(a)}<small>{roleLabel(a)}</small>{/if}</span>
          </label>
        </li>
      {:else}
        <li class="none faint">Никого не нашлось</li>
      {/each}
    </ul>
  </div>
{/if}

<style>
  .who { display: inline-flex; align-items: center; gap: 10px; min-width: 0; flex-wrap: wrap; }
  .stack { display: inline-flex; align-items: center; }
  .f { display: inline-flex; margin-left: -6px; border-radius: 50%; box-shadow: 0 0 0 2px var(--bg-raised); }
  .f:first-child { margin-left: 0; }
  .more { margin-left: 4px; font-size: 12px; color: var(--fg-3); font-variant-numeric: tabular-nums; }
  .sum { font-size: 13px; color: var(--fg-2); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .edit { display: inline-flex; align-items: center; gap: 5px; height: 26px; padding: 0 10px; border-radius: 13px; font-size: 12.5px; box-shadow: inset 0 0 0 1px var(--line-strong); color: var(--fg-2); }
  .edit:hover:not(:disabled) { background: var(--bg-hover); color: var(--fg); }
  .edit:focus-visible { outline: none; box-shadow: var(--focus-ring); }

  .panel { position: fixed; z-index: 90; width: 300px; max-height: 360px; display: flex; flex-direction: column; border-radius: var(--radius); background: var(--bg-raised); box-shadow: 0 0 0 1px var(--line-strong), var(--shadow-2); overflow: hidden; }
  .search { display: flex; align-items: center; gap: 8px; padding: 10px 12px; border-bottom: 1px solid var(--line); color: var(--fg-3); }
  .search input { flex: 1; min-width: 0; border: 0; outline: 0; background: none; font-size: 13px; color: var(--fg); }
  .bulk { display: flex; align-items: center; gap: 8px; padding: 6px 12px; font-size: 12px; }
  .grow { flex: 1; }
  .lnk { color: var(--accent); font-size: 12px; }
  .lnk:hover { text-decoration: underline; }
  ul { list-style: none; margin: 0; padding: 0 6px 8px; overflow-y: auto; }
  li label { display: flex; align-items: center; gap: 9px; padding: 6px 8px; border-radius: var(--radius-s); cursor: pointer; font-size: 13px; }
  li label:hover { background: var(--bg-hover); }
  li label.on { background: var(--accent-soft); }
  li input[type='checkbox'] { accent-color: var(--accent); }
  .nm { display: flex; flex-direction: column; min-width: 0; line-height: 1.25; }
  .nm small { color: var(--fg-3); font-size: 11.5px; }
  .none { padding: 10px 8px; font-size: 12.5px; }
</style>
