<script lang="ts">
  import { untrack } from 'svelte';
  import { get, patch, post } from '../lib/api';
  import { app, loadMachines, loadThreadGit, projectById, refreshThreads } from '../lib/store.svelte';
  import FolderPicker from './FolderPicker.svelte';
  import Dropdown from './Dropdown.svelte';
  let { threadId, projectId, onclose }: { threadId: string; projectId?: string | null; onclose: () => void } = $props();
  let project = $state(untrack(() => projectId ?? ''));
  let path = $state(untrack(() => projectById(projectId)?.root_path ?? ''));
  let machine = $state(untrack(() => projectById(projectId)?.node_id ?? ''));
  let choosing = $state(false);
  let locations = $state<{machine: string; path: string}[]>([]);
  let busy = $state(false);
  let error = $state('');
  $effect(() => { untrack(loadMachines); });
  $effect(() => {
    const id = threadId;
    get(`/api/threads/${id}/workspace`).then(r => locations = Array.isArray(r.locations) ? r.locations : []).catch(() => {});
  });
  function pickMachine(id: string) { machine = id; path = locations.find(l => l.machine === id)?.path ?? ''; }

  function pick(id: string) { locations = []; project = id; const p = projectById(id); path = p?.root_path ?? ''; machine = p?.node_id ?? ''; }
  async function save() {
    busy = true; error = '';
    try {
      if (project !== projectId) await patch(`/api/threads/${threadId}`, { project_id: project });
      if (path.trim()) await post(`/api/threads/${threadId}/workspace`, { path: path.trim(), machine });
      app.projects = await get('/api/projects');
      await refreshThreads(); await loadThreadGit(threadId); onclose();
    } catch (e: any) { error = e.message; }
    finally { busy = false; }
  }
</script>
<section class="workspace" aria-label="Настройки проекта чата">
  <header class="top">
    <b>Проект и рабочая папка</b>
    <button class="btn sm" onclick={onclose}>Закрыть</button>
  </header>
  <div class="grid">
    <span class="lbl">Проект</span>
    <Dropdown block label="Проект чата" value={project} options={app.projects.map(p => ({ id: p.id, label: p.name }))} onpick={pick} />
    <span class="lbl">Компьютер</span>
    <Dropdown block label="Компьютер с папкой" value={machine} options={[{id:'',label:'Нода Senate',hint:'сервер'}, ...app.machines.filter(m => !m.this_node).map(m => ({id:m.id,label:m.name,hint:m.online ? 'в сети' : 'не в сети'}))]} onpick={pickMachine} />
    <label class="lbl" for="ws-path">Папка проекта</label>
    <div class="path">
      <input id="ws-path" class="field" bind:value={path} placeholder="Абсолютный путь к папке проекта на выбранном компьютере" />
      <button class="btn sm" disabled={busy} onclick={() => choosing = true}>Обзор</button>
    </div>
  </div>
  {#if locations.length > 1}
    <div class="known" aria-label="Где проект уже есть">
      <span class="lbl">Уже известно</span>
      <ul>
        {#each locations as l (l.machine)}
          <li><button class="loc" class:on={l.machine === machine} onclick={() => pickMachine(l.machine)}><span class="lm">{app.machines.find(m => m.id === l.machine)?.name ?? 'Нода Senate'}</span><span class="lp">{l.path}</span></button></li>
        {/each}
      </ul>
    </div>
  {/if}
  <p class="faint">У проекта своя папка на каждом компьютере. Файлы между ними Senate не переносит: для этого git. Новая папка действует со следующего запуска.</p>
  {#if !project}<p class="faint">Выберите проект. Новый можно создать в разделе «Проекты» или попросить ассистента.</p>{/if}
  {#if error}<p class="err" role="alert">{error}</p>{/if}
  <footer class="actions">
    <button class="btn sm primary" disabled={busy || !project} onclick={save}>{busy ? 'Проверяю папку…' : 'Сохранить привязку'}</button>
  </footer>
</section>
{#if choosing}<FolderPicker start={path} {machine} where={app.machines.find(m => m.id === machine)?.name ?? 'Нода Senate'} onpick={p => { path = p; choosing = false; }} onclose={() => choosing = false} />{/if}
<style>
  .workspace { width: 100%; max-width: 720px; margin: 0 auto 10px; box-sizing: border-box; background: var(--bg-raised); border: 1px solid var(--line); border-radius: 14px; padding: 14px 16px 16px; display: grid; gap: 12px; max-height: 60vh; overflow: auto; box-shadow: var(--shadow-1); }
  .top { display: flex; align-items: center; justify-content: space-between; gap: 12px; font-size: 14px; }
  .grid { display: grid; grid-template-columns: 112px minmax(0, 1fr); align-items: center; gap: 8px 12px; }
  .lbl { font-size: 12.5px; color: var(--fg-2); }
  .path { display: flex; gap: 6px; min-width: 0; }
  .path .field { flex: 1; min-width: 0; }
  .known { display: grid; grid-template-columns: 112px minmax(0, 1fr); gap: 8px 12px; align-items: start; }
  .known ul { list-style: none; margin: 0; padding: 0; display: grid; gap: 2px; min-width: 0; }
  .loc { display: flex; gap: 8px; width: 100%; padding: 4px 8px; border-radius: 6px; font-size: 12.5px; text-align: left; min-width: 0; }
  .loc:hover, .loc.on { background: var(--accent-soft); }
  .loc:focus-visible { outline: 2px solid var(--accent); outline-offset: 1px; }
  .lm { font-weight: 500; white-space: nowrap; }
  .lp { color: var(--fg-3); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; }
  .actions { display: flex; justify-content: flex-end; }
  p { margin: 0; font-size: 12px; }
  .err { color: var(--danger, var(--warn)); }
  @media (max-width: 520px) {
    .grid, .known { grid-template-columns: minmax(0, 1fr); }
  }
</style>
