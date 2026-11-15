<script lang="ts">
  // The right-hand panel of a chat: the folder it works in — its files, or what changed — and the computer it is on.
  import { app, loadThreadGit, machineOf, projectById } from '../lib/store.svelte';
  import ChangesPanel from './ChangesPanel.svelte';
  import FilesPanel from './FilesPanel.svelte';
  import Icon from './Icon.svelte';
  import ChatWorkspace from './ChatWorkspace.svelte';
  import Splitter from './Splitter.svelte';

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
  {#if tab === 'files'}
    {#key `${filesBase}:${project?.root_path}:${project?.node_id}`}<FilesPanel bind:this={files} base={filesBase} {threadId} onplace={(p) => (place = p)} />{/key}
  {:else}
    <ChangesPanel {threadId} bare />
  {/if}
</aside>

<style>
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
