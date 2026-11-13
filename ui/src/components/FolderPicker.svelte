<script lang="ts">
  import { focusScope } from "../lib/focus";
  import { get } from '../lib/api';
  import Icon from './Icon.svelte';

  // `machine`: the computer whose folders are shown ("" = the node itself).
  let { start = '', machine = '', where = '', onpick, onclose }: { start?: string; machine?: string; where?: string; onpick: (path: string) => void; onclose: () => void } = $props();

  let path = $state('');
  let entries = $state<{ name: string; path: string; git?: boolean }[]>([]);
  let parent = $state<string | null>(null);
  let isGit = $state(false);
  let error = $state('');

  async function load(p: string) {
    try {
      const r = await get(`/api/fs/list?path=${encodeURIComponent(p)}${machine ? `&machine=${encodeURIComponent(machine)}` : ''}`);
      path = r.path;
      entries = r.entries;
      parent = r.parent;
      isGit = !!r.git;
      error = '';
    } catch (e: any) {
      // the starting folder is not on this computer: show its drives instead of a dead end
      if (p && p === start) return load('');
      error = e.message;
    }
  }
  $effect(() => {
    machine;
    // a folder from another computer means nothing here: start from the top
    load(start);
  });
</script>

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="scrim" onclick={onclose}>
  <div class="dlg" use:focusScope={onclose} role="dialog" tabindex="-1" aria-modal="true" aria-label="Выбор папки" onclick={(e) => e.stopPropagation()}>
    <header>
      <b>Выберите рабочую папку{#if where} · {where}{/if}</b>
      <button class="icon-btn" onclick={onclose} aria-label="Закрыть"><Icon name="x" size={15} /></button>
    </header>
    <div class="cur mono" title={path}>{path || 'Расположения'}</div>
    <ul>
      {#if path}
        <li><button onclick={() => load(parent ?? '')}><Icon name="undo" size={14} />Выше</button></li>
      {/if}
      {#each entries as e (e.path)}
        <li>
          <button ondblclick={() => onpick(e.path)} onclick={() => load(e.path)}>
            <Icon name="folder" size={14} /><span>{e.name}</span>{#if e.git}<span class="chip">git</span>{/if}
          </button>
        </li>
      {/each}
      {#if error}<li class="err">{error}</li>{/if}
    </ul>
    <footer>
      <span class="faint">{isGit ? 'Репозиторий git — изменения будут видны рядом с чатом' : ''}</span>
      <button class="btn" onclick={onclose}>Отмена</button>
      <button class="btn primary" disabled={!path} onclick={() => onpick(path)}>Выбрать эту папку</button>
    </footer>
  </div>
</div>

<style>
  .scrim {
    position: fixed;
    inset: 0;
    z-index: 110;
    background: var(--scrim);
    backdrop-filter: blur(6px);
    display: grid;
    place-items: center;
  }
  .dlg {
    width: 540px;
    max-width: calc(100vw - 32px);
    height: 460px;
    max-height: 80vh;
    display: flex;
    flex-direction: column;
    background: var(--bg-raised);
    border-radius: var(--radius-l);
    box-shadow: var(--shadow-2);
    overflow: hidden;
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 12px 14px 8px 18px;
  }
  .cur {
    padding: 0 18px 8px;
    font-size: 12px;
    color: var(--fg-2);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  ul {
    flex: 1;
    overflow: auto;
    list-style: none;
    margin: 0;
    padding: 4px 8px;
    border-top: 1px solid var(--line);
    border-bottom: 1px solid var(--line);
  }
  li button {
    display: flex;
    align-items: center;
    gap: 9px;
    width: 100%;
    padding: 6px 10px;
    border-radius: var(--radius-s);
    text-align: left;
  }
  li button span:first-of-type {
    flex: 1;
  }
  li button:hover {
    background: var(--bg-hover);
  }
  .err {
    color: var(--bad);
    padding: 8px 10px;
  }
  footer {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 14px;
  }
  footer .faint {
    flex: 1;
    font-size: 12.5px;
  }
  footer { flex-wrap: wrap; }
</style>
