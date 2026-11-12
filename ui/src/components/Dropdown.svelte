<script lang="ts">
  import { tick, type Snippet } from 'svelte';
  import BrandMark from './BrandMark.svelte';
  import Icon from './Icon.svelte';

  type Opt = { id: string; label: string; hint?: string; disabled?: boolean; group?: string; brand?: string };
  let {
    value,
    options,
    onpick,
    label = '',
    up = false,
    align = 'left',
    block = false,
    selected,
    ontoggle,
    children,
  }: {
    value: string;
    options: Opt[];
    onpick: (id: string) => void;
    label?: string;
    up?: boolean;
    align?: 'left' | 'right';
    block?: boolean;
    /** Several choices at once: these ids are ticked, a click toggles one and the menu stays open. */
    selected?: string[];
    ontoggle?: (id: string) => void;
    children?: Snippet<[Opt | undefined]>;
  } = $props();

  let open = $state(false);
  let root: HTMLElement;
  let trigger: HTMLButtonElement;
  let menu = $state<HTMLDivElement>();
  let position = $state('');
  async function show() {
    open = !open;
    if (!open) return;
    const r = trigger.getBoundingClientRect();
    const width = Math.min(Math.max(block ? r.width : 280, 240), window.innerWidth - 24);
    const left = Math.max(12, Math.min(align === 'right' ? r.right - width : r.left, window.innerWidth - width - 12));
    const above = up && r.top > 180;
    const space = above ? r.top - 20 : window.innerHeight - r.bottom - 20;
    position = `left:${left}px;width:${width}px;min-width:0;max-height:${Math.max(80, Math.min(420, space))}px;${above ? `bottom:${window.innerHeight - r.top + 6}px;top:auto` : `top:${r.bottom + 6}px;bottom:auto`}`;
    await tick();
    (menu?.querySelector<HTMLButtonElement>('[aria-selected="true"]:not(:disabled)') ?? menu?.querySelector<HTMLButtonElement>('button:not(:disabled)'))?.focus();
  }
  function navigate(e: KeyboardEvent) {
    if (!open || !root.contains(e.target as Node)) return;
    if (e.key === 'Tab') { open = false; return; }
    if (!['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(e.key)) return;
    e.preventDefault();
    const choices = Array.from(menu!.querySelectorAll<HTMLButtonElement>('button:not(:disabled)'));
    const i = choices.indexOf(document.activeElement as HTMLButtonElement);
    const next = e.key === 'Home' ? 0 : e.key === 'End' ? choices.length - 1 : (i + (e.key === 'ArrowDown' ? 1 : -1) + choices.length) % choices.length;
    choices[next]?.focus();
  }
  const multi = $derived(selected !== undefined);
  const isOn = (id: string) => (multi ? (id === '' ? selected!.length === 0 : selected!.includes(id)) : id === value);
  const current = $derived(options.find((o) => o.id === value));

  function onDoc(e: MouseEvent) {
    if (open && !root.contains(e.target as Node)) open = false;
  }
  function onKey(e: KeyboardEvent) {
    if (e.key === 'Escape' && open) { open = false; trigger?.focus(); }
    navigate(e);
  }
</script>

<svelte:window onmousedown={onDoc} onkeydown={onKey} onresize={() => (open = false)} />

<div class="dd" class:block bind:this={root}>
  <button bind:this={trigger} class="trigger" class:block aria-haspopup="listbox" aria-expanded={open} aria-label={label} onclick={show} onkeydown={(e) => { if (!open && ['ArrowDown', 'ArrowUp'].includes(e.key)) { e.preventDefault(); e.stopPropagation(); show(); } }}>
    {#if children}{@render children(current)}{:else}
      {#if current?.brand}<BrandMark id={current.brand} size={14} tint />{/if}
      <span class="tl">{current?.label ?? label}</span>
    {/if}
    <Icon name="down" size={12} />
  </button>
  {#if open}
    <div bind:this={menu} class="menu" class:up class:right={align === 'right'} class:block role="listbox" aria-multiselectable={multi || undefined} aria-label={label} style={position}>
      {#each options as o, i (o.id + i)}
        {#if o.group && o.group !== options[i - 1]?.group}<div class="grp">{o.group}</div>{/if}
        <button
          class="opt"
          role="option"
          aria-selected={isOn(o.id)}
          disabled={o.disabled}
          onclick={() => {
            if (multi) {
              ontoggle?.(o.id);
              return;
            }
            open = false;
            onpick(o.id);
            trigger.focus();
          }}
        >
          <span class="tick" class:box={multi}>{#if isOn(o.id)}<Icon name="check" size={13} />{/if}</span>
          {#if o.brand}<span class="mk"><BrandMark id={o.brand} size={14} tint /></span>{/if}
          <span class="txt"><b>{o.label}</b>{#if o.hint}<small>{o.hint}</small>{/if}</span>
        </button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .dd {
    position: relative;
    display: inline-block;
  }
  .dd.block {
    display: block;
  }
  .trigger {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 26px;
    padding: 0 8px;
    border-radius: var(--radius-s);
    color: var(--fg-2);
    font-size: 13px;
  }
  .trigger.block {
    width: 100%;
    height: 34px;
    padding: 0 10px;
    border: 1px solid var(--line-strong);
    background: var(--bg-raised);
    color: var(--fg);
    justify-content: space-between;
  }
  .tl {
    flex: 1;
    text-align: left;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .trigger:hover,
  .trigger[aria-expanded='true'] {
    background: var(--bg-hover);
    color: var(--fg);
  }
  .trigger.block:hover {
    background: var(--bg-raised);
    border-color: var(--fg-3);
  }
  .menu {
    position: fixed;
    z-index: 50;
    top: calc(100% + 4px);
    left: 0;
    min-width: 250px;
    max-height: min(420px, 60vh);
    overflow-y: auto;
    padding: 4px;
    border-radius: var(--radius);
    background: var(--bg-raised);
    box-shadow: var(--shadow-2);
    animation: fadeIn 0.12s var(--ease);
  }
  .menu.block {
    min-width: 100%;
  }
  .menu.up {
    top: auto;
    bottom: calc(100% + 4px);
  }
  .menu.right {
    left: auto;
    right: 0;
  }
  .grp {
    padding: 8px 8px 3px;
    font-size: 11.5px;
    font-weight: 600;
    letter-spacing: 0.02em;
    color: var(--fg-3);
    text-transform: uppercase;
  }
  .opt {
    display: flex;
    width: 100%;
    gap: 6px;
    align-items: flex-start;
    padding: 6px 8px;
    border-radius: var(--radius-s);
    text-align: left;
  }
  .opt:hover:not(:disabled) {
    background: var(--bg-hover);
  }
  .opt:focus-visible { background: var(--selection); color: var(--selection-fg); outline-offset: -2px; }
  .opt:disabled {
    opacity: 0.5;
  }
  .tick {
    width: 14px;
    color: var(--accent);
    padding-top: 2px;
    flex: none;
  }
  .tick.box {
    width: 15px;
    height: 15px;
    margin-top: 2px;
    padding: 0;
    display: grid;
    place-items: center;
    border-radius: 4px;
    box-shadow: inset 0 0 0 1.5px var(--line-strong);
  }
  .opt[aria-selected='true'] .tick.box {
    background: var(--accent-soft, var(--bg-active));
    box-shadow: inset 0 0 0 1.5px var(--accent);
  }
  .mk {
    padding-top: 2px;
    display: inline-flex;
  }
  .txt {
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  .txt b {
    font-weight: 500;
  }
  .txt small {
    color: var(--fg-3);
    font-size: 12px;
    line-height: 1.35;
  }
</style>
