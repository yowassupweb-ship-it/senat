<script lang="ts">
  // The right-hand panel of a chat: the folder it works in — its files, or what changed — and the computer it is on.
  import { app, loadThreadGit, machineOf, projectById } from '../lib/store.svelte';
  import ChangesPanel from './ChangesPanel.svelte';
  import FilesPanel from './FilesPanel.svelte';
  import Icon from './Icon.svelte';
  import ChatWorkspace from './ChatWorkspace.svelte';
  import Splitter from './Splitter.svelte';
  import FolderPicker from './FolderPicker.svelte';
  import { get, post } from '../lib/api';
  import { readThisMachine } from '../lib/link';

  let { threadId, projectId = '' }: { threadId: string; projectId?: string } = $props();

  let settings = $state(false);
  const workers = $derived(app.live.filter(r => r.workspace?.project_id && r.workspace.project_id === projectId || r.thread_id === threadId));
  const machineName = (id?: string | null) => id ? app.machines.find(m => m.id === id)?.name ?? id : 'Нода Senate';
  const project = $derived(projectById(projectId));
  const hasFolder = $derived(!!project?.root_path);
  const machine = $derived(machineOf(project));
  // Files are always there: the project's folder, or — for a chat without one — the specialists' private folders
  // where its specialist works (on the computer or on the node).
  const tab = $derived(app.panelTab);
  const filesBase = $derived(hasFolder && project ? `/api/projects/${project.id}` : `/api/threads/${threadId}`);
  let place = $state<{ label: string; machine: string; machine_name: string } | null>(null);
  $effect(() => {
    threadId;
    place = null;
  });
  const changes = $derived(app.git['t:' + threadId]?.changes?.length ?? 0);
  let files = $state<FilesPanel>();

  // A project can be on several computers, in a different folder on each. When it is bound to another computer
  // and this one has no folder recorded, ask once whether it is here too.
  let me = $state<{ id: string; name: string } | null>(null);
  let locations = $state<{ machine: string; path: string }[] | null>(null);
  let herePath = $state('');
  let picking = $state(false);
  let hereBusy = $state(false);
  let hereError = $state('');
  let dismissed = $state(0);
  const hereKey = $derived(project && me ? `senate.here.${project.id}.${me.id}` : '');
  $effect(() => {
    readThisMachine().then((m) => (me = m));
  });
  $effect(() => {
    const id = threadId;
    void project?.root_path;
    void project?.node_id;
    locations = null;
    get(`/api/threads/${id}/workspace`)
      .then((r) => (locations = Array.isArray(r.locations) ? r.locations : []))
      .catch(() => {});
  });
  function wasDismissed(key: string) {
    void dismissed;
    try {
      return !!key && localStorage.getItem(key) === 'no';
    } catch {
      return false;
    }
  }
  const askHere = $derived(
    !!me && !!project?.root_path && !!project.node_id && project.node_id !== me.id && !!locations && !locations.some((l) => l.machine === me!.id) && !wasDismissed(hereKey)
  );
  async function saveHere() {
    if (!me || !herePath.trim()) return;
    hereBusy = true;
    hereError = '';
    try {
      // the main computer is away: work moves here; otherwise this folder is only remembered as a second place
      await post(`/api/threads/${threadId}/workspace`, { path: herePath.trim(), machine: me.id, activate: !!machine && !machine.online });
      app.projects = await get('/api/projects');
      locations = [...(locations ?? []), { machine: me.id, path: herePath.trim() }];
      loadThreadGit(threadId);
    } catch (e: any) {
      hereError = e.message;
    } finally {
      hereBusy = false;
    }
  }
  function notHere() {
    try {
      if (hereKey) localStorage.setItem(hereKey, 'no');
    } catch {
      // without storage the question simply comes back next time
    }
    dismissed++;
  }

  function pick(t: 'files' | 'changes') {
    app.panelTab = t;
    try {
      localStorage.setItem('senate.panelTab', t);
    } catch {
      // the choice is only remembered when storage works
    }
  }
  function refresh() {
    loadThreadGit(threadId);
    files?.refresh();
  }
</script>

<aside class="panel" aria-label="Проект">
  <Splitter pane="panel" edge="left" label="Ширина панели проекта" />
  <header>
    <div class="tabs" role="tablist" aria-label="Что показать">
      <button role="tab" aria-selected={tab === 'files'} class:on={tab === 'files'} onclick={() => pick('files')}>Файлы</button>
      <button role="tab" aria-selected={tab === 'changes'} class:on={tab === 'changes'} onclick={() => pick('changes')}>Изменения{#if changes}<span class="n">{changes}</span>{/if}</button>
    </div>
    <span class="grow"></span>
    <button class="icon-btn" onclick={refresh} aria-label="Обновить" title="Обновить"><Icon name="refresh" size={14} /></button>
    <button class="icon-btn" onclick={() => (app.panel = false)} aria-label="Закрыть панель" title="Закрыть панель"><Icon name="x" size={14} /></button>
  </header>
  <button class="btn sm" onclick={() => settings = !settings}>Папка и компьютер</button>
  {#if settings}<ChatWorkspace {threadId} {projectId} onclose={() => settings = false} />{/if}
  {#if workers.length}
    <section class="workers" aria-label="Кто работает в проекте">
      {#each workers as run (run.session_id)}
        <div class="worker"><b>{app.agents.find(a => a.id === run.agent_id)?.name ?? 'Участник'}</b> · {run.status === 'queued' ? 'в очереди' : run.awaiting ? 'ждёт ответа' : 'работает'}
          <div class="faint">{run.hint || run.phase}</div>
          <div class="faint">{run.workspace?.path ? `${machineName(run.workspace.machine)} · ${run.workspace.path}` : 'Рабочая папка уточняется при запуске'}</div>
        </div>
      {/each}
    </section>
  {/if}
  {#if hasFolder}
    <div class="where" title={project?.root_path}>
      <span class="dot" class:on={!machine || machine.online} aria-hidden="true"></span>
      <Icon name={machine ? 'monitor' : 'server'} size={12} />
      <span class="mname">{machine ? machine.name : 'Нода Senate'}</span>
      {#if machine && !machine.online}<span class="off">не в сети</span>{/if}
      <span class="root">{project?.root_path}</span>
    </div>
  {:else if place}
    <div class="where">
      <span class="dot on" aria-hidden="true"></span>
      <Icon name={place.machine ? 'monitor' : 'server'} size={12} />
      <span class="mname">{place.machine_name || 'Сервер'}</span>
      <span class="root">{place.label}</span>
    </div>
  {/if}
  {#if askHere && me}
    <section class="here" aria-label="Проект на этом компьютере">
      <b>Этот проект есть на этом компьютере?</b>
      <p class="faint">
        Сейчас он привязан к компьютеру «{machine?.name}»{#if machine && !machine.online}, а тот не в сети{/if}. Укажите папку проекта здесь:
        {machine && !machine.online ? 'работа пойдёт на этом компьютере.' : 'она станет запасным местом, когда тот компьютер не в сети.'}
      </p>
      <input class="field" bind:value={herePath} placeholder="Путь к папке проекта на «{me.name}»" aria-label="Путь к папке проекта на этом компьютере" />
      <div class="here-actions">
        <button class="btn sm" disabled={hereBusy} onclick={() => (picking = true)}>Выбрать папку</button>
        <button class="btn sm primary" disabled={hereBusy || !herePath.trim()} onclick={saveHere}>{hereBusy ? 'Проверяю папку…' : 'Сохранить'}</button>
        <button class="btn sm" disabled={hereBusy} onclick={notHere}>Здесь его нет</button>
      </div>
      {#if hereError}<p role="alert">{hereError}</p>{/if}
    </section>
  {/if}
  {#if picking && me}<FolderPicker start={herePath} machine={me.id} where={me.name} onpick={(p) => { herePath = p; picking = false; }} onclose={() => (picking = false)} />{/if}
  {#if tab === 'files'}
    {#key `${filesBase}:${project?.root_path}:${project?.node_id}`}<FilesPanel bind:this={files} base={filesBase} {threadId} onplace={(p) => (place = p)} />{/key}
  {:else}
    <ChangesPanel {threadId} bare />
  {/if}
</aside>

<style>
  .here { margin: 0 12px 10px; padding: 12px; display: grid; gap: 8px; border: 1px solid var(--line); border-radius: 12px; background: var(--bg-raised); font-size: 12.5px; }
  .here p { margin: 0; font-size: 12px; }
  .here-actions { display: flex; flex-wrap: wrap; gap: 6px; }
  .workers { padding: 0.75rem; border-bottom: 1px solid var(--line); max-height: 25vh; overflow: auto; }
  .worker { font-size: 0.75rem; overflow-wrap: anywhere; padding-block: 0.25rem; }
  .panel {
    width: var(--panel-w, 400px);
    position: relative;
    flex: none;
    border-left: 1px solid var(--line);
    background: var(--bg-sidebar);
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
  header {
    height: 46px;
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 0 10px 0 12px;
    flex: none;
  }
  .grow {
    flex: 1;
  }
  .tabs {
    display: flex;
    gap: 2px;
    padding: 2px;
    border-radius: 8px;
    background: var(--accent-soft);
  }
  .tabs button {
    display: flex;
    align-items: center;
    gap: 6px;
    height: 26px;
    padding: 0 10px;
    border-radius: 6px;
    font-size: 12.5px;
    color: var(--fg-2);
  }
  .tabs button.on {
    background: var(--bg-raised);
    color: var(--fg);
    box-shadow: var(--shadow-1);
  }
  .n {
    min-width: 16px;
    padding: 0 4px;
    border-radius: 8px;
    background: var(--accent-soft);
    font-size: 11px;
    font-variant-numeric: tabular-nums;
  }
  .where {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 0 14px 8px;
    font-size: 12px;
    color: var(--fg-2);
    min-width: 0;
  }
  .dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--fg-3);
    flex: none;
  }
  .dot.on {
    background: var(--ok);
  }
  .mname {
    font-weight: 500;
    white-space: nowrap;
  }
  .off {
    color: var(--warn);
    white-space: nowrap;
  }
  .root {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    direction: rtl;
    text-align: left;
    color: var(--fg-3);
  }
</style>
