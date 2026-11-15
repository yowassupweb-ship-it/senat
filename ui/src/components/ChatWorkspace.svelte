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
  <div class="row"><b>Проект и рабочая папка</b><button class="btn sm" onclick={onclose}>Закрыть</button></div>
  <Dropdown label="Проект чата" value={project} options={app.projects.map(p => ({ id: p.id, label: p.name }))} onpick={pick} />
  <Dropdown label="Компьютер с папкой" value={machine} options={[{id:'',label:'Нода Senate'}, ...app.machines.filter(m => !m.this_node).map(m => ({id:m.id,label:m.name,hint:m.online ? 'В сети' : 'Не в сети'}))]} onpick={pickMachine} />
  <label>Папка проекта<input class="field" bind:value={path} placeholder="Абсолютный путь к существующей папке" /></label>
  <button class="btn sm" disabled={busy} onclick={() => choosing = true}>Выбрать папку на этом компьютере</button>
  <p class="faint">Пути разных компьютеров сохраняются отдельно. Переключение не синхронизирует файлы; выполняющиеся задачи остаются на прежнем месте.</p>
  <p class="faint">Ветка Git и изменения появятся под полем ввода. Файлы доступны в панели проекта. Папка применяется к следующим запускам всех участников проекта.</p>
  {#if !project}<p class="faint">Выберите проект. Новый проект можно создать в разделе «Проекты» или попросить ассистента.</p>{/if}
  {#if error}<p role="alert">{error}</p>{/if}
  <button class="btn sm primary" disabled={busy || !project} onclick={save}>{busy ? 'Проверяю папку…' : 'Сохранить привязку'}</button>
</section>
{#if choosing}<FolderPicker start={path} {machine} where={app.machines.find(m => m.id === machine)?.name ?? 'Нода Senate'} onpick={p => { path = p; choosing = false; }} onclose={() => choosing = false} />{/if}
<style>
  .workspace { background: var(--bg-raised); border: 1px solid var(--line); border-radius: 12px; padding: 16px; display: grid; gap: 12px; max-height: 50vh; overflow: auto; }
  .row { display: flex; align-items: center; justify-content: space-between; gap: 12px; }
  label { display: grid; gap: 6px; }
  p { margin: 0; font-size: 12px; }
</style>
