<script lang="ts">
  import type { Snippet } from 'svelte';
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
    children,
  }: {
    value: string;
    options: Opt[];
    onpick: (id: string) => void;
    label?: string;
    up?: boolean;
    align?: 'left' | 'right';
    block?: boolean;
    children?: Snippet<[Opt | undefined]>;
  } = $props();

  let open = $state(false);
  let root: HTMLElement;
  const current = $derived(options.find((o) => o.id === value));

  function onDoc(e: MouseEvent) {
    if (open && !root.contains(e.target as Node)) open = false;
  }
  function onKey(e: KeyboardEvent) {
    if (e.key === 'Escape') open = false;
  }
</script>

<svelte:window onmousedown={onDoc} onkeydown={onKey} />

<div class="dd" class:block bind:this={root}>
  <button class="trigger" class:block aria-haspopup="listbox" aria-expanded={open} aria-label={label} onclick={() => (open = !open)}>
    {#if children}{@render children(current)}{:else}
      {#if current?.brand}<BrandMark id={current.brand} size={14} tint />{/if}
      <span class="tl">{current?.label ?? label}</span>
    {/if}
    <Icon name="down" size={12} />
  </button>
  {#if open}
    <div class="menu" class:up class:right={align === 'right'} class:block role="listbox">
      {#each options as o, i (o.id + i)}
        {#if o.group && o.group !== options[i - 1]?.group}<div class="grp">{o.group}</div>{/if}
        <button
          class="opt"
          role="option"
          aria-selected={o.id === value}
          disabled={o.disabled}
          onclick={() => {
            open = false;
            onpick(o.id);
          }}
        >
          <span class="tick">{#if o.id === value}<Icon name="check" size={13} />{/if}</span>
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
    position: absolute;
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
  .opt:disabled {
    opacity: 0.5;
  }
  .tick {
    width: 14px;
    color: var(--accent);
    padding-top: 2px;
    flex: none;
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
