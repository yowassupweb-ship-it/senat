<script lang="ts">
  import { app } from '../lib/store.svelte';
  import Icon from './Icon.svelte';
  import LoadMeter from './LoadMeter.svelte';

  // The window header of the desktop app (the system frame is off): logo, a search bar, window buttons.
  const call = (cmd: string) => {
    try {
      (window as any).__TAURI_INTERNALS__.invoke(`plugin:window|${cmd}`, { label: 'main' });
    } catch {
      /* not inside the desktop shell */
    }
  };
</script>

<header class="titlebar" data-tauri-drag-region>
  <div class="left" data-tauri-drag-region>
    <div class="brand" data-tauri-drag-region><img class="mark" src="/logo.png" alt="" width="24" height="24" data-tauri-drag-region /><span class="name" data-tauri-drag-region>Senate</span></div>
    {#if app.ready}<span class="meter"><LoadMeter /></span>{/if}
  </div>
  <div class="mid" data-tauri-drag-region>
    {#if app.ready}
      <button class="search" onclick={() => (app.palette = true)} aria-label="Поиск и команды">
        <Icon name="search" size={14} /><span>Поиск и команды</span><kbd>Ctrl K</kbd>
      </button>
    {/if}
  </div>
  <div class="ctl">
    <button class="wb" onclick={() => call('minimize')} aria-label="Свернуть"><svg width="10" height="10" viewBox="0 0 10 10"><path d="M0 5h10" stroke="currentColor" fill="none" /></svg></button>
    <button class="wb" onclick={() => call('toggle_maximize')} aria-label="Развернуть"><svg width="10" height="10" viewBox="0 0 10 10"><rect x="0.5" y="0.5" width="9" height="9" stroke="currentColor" fill="none" /></svg></button>
    <button class="wb close" onclick={() => call('close')} aria-label="Закрыть"><svg width="10" height="10" viewBox="0 0 10 10"><path d="M0 0l10 10M10 0L0 10" stroke="currentColor" fill="none" /></svg></button>
  </div>
</header>

<style>
  .titlebar {
    height: 38px;
    display: grid;
    /* equal side columns: the search stands in the very middle of the window */
    grid-template-columns: minmax(0, 1fr) minmax(0, 480px) minmax(0, 1fr);
    align-items: center;
    box-shadow: inset 0 -1px 0 var(--line);
    user-select: none;
    -webkit-user-select: none;
  }
  .brand { display: flex; align-items: center; gap: 9px; margin-left: 18px; pointer-events: none; }
  .mark { display: block; object-fit: contain; }
  .name { font-size: 15px; font-weight: 650; letter-spacing: -0.02em; color: var(--fg); }
  .left { display: flex; align-items: center; align-self: stretch; min-width: 0; }
  .meter { margin-left: auto; padding-right: 10px; }
  .mid { display: flex; justify-content: center; align-self: stretch; align-items: center; }
  .search {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    height: 26px;
    padding: 0 8px 0 11px;
    border-radius: 8px;
    background: var(--bg-raised);
    box-shadow: inset 0 0 0 1px var(--line);
    color: var(--fg-3);
    font-size: 13px;
  }
  .search:hover { color: var(--fg-2); background: var(--bg-hover); }
  .search span { flex: 1; min-width: 0; text-align: left; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .mid { min-width: 0; gap: 8px; }
  kbd { font: 11px var(--mono, monospace); padding: 2px 6px; border-radius: 5px; background: var(--bg-hover); color: var(--fg-2); }
  .ctl { display: flex; align-self: stretch; justify-self: end; }
  .wb { width: 46px; display: grid; place-items: center; color: var(--fg-2); border-radius: 0; }
  .wb:hover { background: var(--bg-hover); color: var(--fg); }
  .wb.close:hover { background: #e5484d; color: #fff; }
  @media (max-width: 900px) { .brand { margin-left: 12px; } .name { display: none; } kbd { display: none; } .meter { display: none; } }
</style>
