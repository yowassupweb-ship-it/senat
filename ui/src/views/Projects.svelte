<script lang="ts">
  import Dropdown from '../components/Dropdown.svelte';
  import { get, patch, post, put } from '../lib/api';
  import { app, go } from '../lib/store.svelte';
  import { MODE_LABEL, N, STATE_LABEL, count, timeAgo } from '../lib/format';
  import type { Agent, Decision, Obligation, Project, Thread } from '../lib/types';
  import Avatar from '../components/Avatar.svelte';
  import FolderPicker from '../components/FolderPicker.svelte';
  import Icon from '../components/Icon.svelte';
  import Page from '../components/Page.svelte';

  let { id }: { id?: string } = $props();

  let creating = $state(false);
  // A new project's folder is looked for on the computer the person used last.
  const lastMachine = () => [...app.machines].filter((m) => m.online).sort((a, b) => (b.last_seen ?? 0) - (a.last_seen ?? 0))[0]?.id ?? '';
  let draft = $state({ name: '', goal: '', root_path: '', company_id: '', node_id: '' });
  let picking = $state<'new' | 'edit' | null>(null);
  /** The computer the folder picker browses (for an existing project: chosen before the folder). */
  let pickMachine = $state('');
  const machineOptions = $derived([
    ...app.machines.map((m) => ({ id: m.id, label: m.name, hint: m.online ? 'в сети' : 'не в сети' })),
    { id: '', label: 'Сервер', hint: 'папка на самой ноде' },
  ]);
  const machineName = (id: string) => (id ? (app.machines.find((m) => m.id === id)?.name ?? 'Компьютер') : 'Сервер');
  let error = $state('');

  let d = $state<{ project: Project; members: Agent[]; decisions: Decision[]; obligations: Obligation[]; threads: Thread[]; memory: any[] } | null>(null);
  let f = $state({ goal: '', criteria: '', constraints: '' });
  let newDecision = $state('');

  async function load() {
    if (!id) return (d = null);
    try {
      d = await get(`/api/projects/${id}`);
      f = { goal: d!.project.goal, criteria: d!.project.criteria, constraints: d!.project.constraints };
    } catch (e: any) {
      error = e.message;
    }
  }
  $effect(() => {
    id;
    load();
  });

  async function create() {
    try {
      const p = await post('/api/projects', draft);
      app.projects = await get('/api/projects');
      creating = false;
      draft = { name: '', goal: '', root_path: '', company_id: '', node_id: '' };
      go({ kind: 'projects', id: p.id });
    } catch (e: any) {
      error = e.message;
    }
  }
  async function save(fields: Record<string, string>) {
    try {
      await patch(`/api/projects/${id}`, fields);
      app.projects = await get('/api/projects');
      await load();
      error = '';
    } catch (e: any) {
      error = e.message;
    }
  }
  async function toggleMember(a: Agent) {
    if (!d) return;
    const ids = d.members.some((m) => m.id === a.id) ? d.members.filter((m) => m.id !== a.id).map((m) => m.id) : [...d.members.map((m) => m.id), a.id];
    await put(`/api/projects/${id}/members`, { agents: ids });
    load();
  }
  async function propose() {
    if (!newDecision.trim()) return;
    await post(`/api/projects/${id}/decisions`, { title: newDecision });
    newDecision = '';
    load();
  }
  async function decide(dec: Decision, status: string) {
    await post(`/api/decisions/${dec.id}/status`, { status });
    load();
  }
  const dstatus: Record<string, string> = { proposed: 'предложено', accepted: 'принято', rejected: 'отклонено', superseded: 'заменено' };
</script>

{#if picking}
  <FolderPicker
    start={picking === 'edit' ? (pickMachine === (d?.project.node_id ?? '') ? (d?.project.root_path ?? '') : '') : draft.root_path}
    machine={picking === 'edit' ? pickMachine : draft.node_id}
    where={machineName(picking === 'edit' ? pickMachine : draft.node_id)}
    onclose={() => (picking = null)}
    onpick={(p) => {
      if (picking === 'new') draft.root_path = p;
      else save({ root_path: p, node_id: pickMachine });
      picking = null;
    }}
  />
{/if}

{#if !id}
  <Page title="Проекты">
    {#snippet actions()}<button class="btn primary" onclick={() => (creating = !creating)}><Icon name="plus" size={14} />Новый проект</button>{/snippet}
    {#if creating}
      <form class="card pad form" style="margin-bottom:18px" onsubmit={(e) => { e.preventDefault(); create(); }}>
        <label>Название<input class="field" bind:value={draft.name} required /></label>
        <label>Цель<textarea class="field" rows="2" bind:value={draft.goal}></textarea></label>
        <label>Компания
          <Dropdown block value={draft.company_id} options={[{ id: '', label: 'Без компании' }, ...app.companies.map((c) => ({ id: c.id, label: c.name }))]} label="Компания" onpick={(v) => (draft.company_id = v)} /></label>
        <label>Рабочая папка <small>необязательно — без неё специалисты работают без доступа к вашим файлам</small>
          <div class="row">
            {#if app.machines.length}<Dropdown value={draft.node_id} options={machineOptions} label="Компьютер с папкой" onpick={(v) => ((draft.node_id = v), (draft.root_path = ''))} />{/if}
            <input class="field" bind:value={draft.root_path} placeholder="Не выбрана" /><button class="btn" type="button" onclick={() => (picking = 'new')}><Icon name="folder" size={14} />Выбрать</button></div></label>
        <div class="row"><button class="btn primary" type="submit">Создать</button><button class="btn" type="button" onclick={() => (creating = false)}>Отмена</button></div>
        {#if error}<p class="bad">{error}</p>{/if}
      </form>
    {/if}
    <div class="cards">
      {#each app.projects as p (p.id)}
        {@const _m = p.node_id ? machineName(p.node_id) : ''}
        <button class="card pad click pcard" onclick={() => go({ kind: 'projects', id: p.id })}>
          <div class="row"><Icon name="folder" size={16} /><b>{p.name}</b></div>
          <p class="clamp" class:muted={!!p.goal} class:faint={!p.goal}>{p.goal || 'Цель не задана'}</p>
          <div class="row">{#if p.open_obligations}<span class="chip">{count(p.open_obligations, N.task)}</span>{/if}{#if p.root_path}<span class="chip" title={p.root_path}>{_m ? `папка на ${_m}` : 'папка подключена'}</span>{/if}
            <span class="row" style="margin-left:auto;gap:0">{#each (p.members ?? []).slice(0, 4) as a (a.id)}<Avatar name={a.name} color={a.accent} size={20} />{/each}</span></div>
        </button>
      {:else}<p class="empty-note">Проектов пока нет.</p>{/each}
    </div>
  </Page>
{:else if d}
  <Page title={d.project.name} sub={d.project.root_path || 'Без рабочей папки'} back={{ label: 'Проекты', onclick: () => go({ kind: 'projects' }) }}>
    {#snippet actions()}
      <button class="btn primary" onclick={() => go({ kind: 'new', project: id })}><Icon name="chat" size={14} />Новый чат</button>
    {/snippet}
    <div class="split">
      <div>
        <div class="section">
          <h2>Картина работы</h2>
          <div class="card pad form">
            <label>Цель<textarea class="field" rows="2" bind:value={f.goal}></textarea></label>
            <label>Критерии результата<textarea class="field" rows="2" bind:value={f.criteria}></textarea></label>
            <label>Ограничения<textarea class="field" rows="2" bind:value={f.constraints}></textarea></label>
            <div class="row">
              <button class="btn primary" disabled={f.goal === d.project.goal && f.criteria === d.project.criteria && f.constraints === d.project.constraints} onclick={() => save(f)}>Сохранить</button>
              <small class="faint">Изменение требований попадёт в журнал и станет основанием пересмотреть связанную работу.</small>
            </div>
          </div>
        </div>

        <div class="section">
          <h2>Решения <span class="count">{d.decisions.length}</span></h2>
          <ul class="list">
            {#each d.decisions as dec (dec.id)}
              <li class="item">
                <span class="grow"><span class="title">{dec.title}</span>{#if dec.body}<div class="sub">{dec.body}</div>{/if}</span>
                <span class="chip {dec.status === 'accepted' ? 'ok' : dec.status === 'proposed' ? 'warn' : ''}">{dstatus[dec.status]}</span>
                {#if dec.status === 'proposed'}
                  <button class="btn sm primary" onclick={() => decide(dec, 'accepted')}>Принять</button>
                  <button class="btn sm" onclick={() => decide(dec, 'rejected')}>Отклонить</button>
                {/if}
              </li>
            {:else}<li class="empty-note">Решений пока нет</li>{/each}
          </ul>
          <form class="row" style="margin-top:8px" onsubmit={(e) => { e.preventDefault(); propose(); }}>
            <input class="field" bind:value={newDecision} placeholder="Предложить решение, например: «Храним данные в SQLite»" />
            <button class="btn" type="submit">Предложить</button>
          </form>
        </div>

        <div class="section">
          <h2>Задача <span class="count">{d.obligations.length}</span></h2>
          <ul class="list">
            {#each d.obligations.slice(0, 20) as o (o.id)}
              <li><button class="item" onclick={() => go({ kind: 'obligations', id: o.id })}>
                <span class="dot {o.state}"></span><span class="grow"><span class="title">{o.title}</span></span>
                <span class="chip">{STATE_LABEL[o.state]}</span></button></li>
            {:else}<li class="empty-note">Нет задач</li>{/each}
          </ul>
        </div>
      </div>

      <div>
        <div class="section">
          <h2>Рабочая папка</h2>
          <div class="card pad form">
            {#if d.project.root_path}
              <div class="faint" style="font-size:12px">{machineName(d.project.node_id ?? '')}{#if d.project.node_id && !app.machines.find((m) => m.id === d!.project.node_id)?.online} · не в сети{/if}</div>
              <div class="mono" style="font-size:12.5px;overflow-wrap:anywhere">{d.project.root_path}</div>
            {:else}<div class="faint" style="font-size:13px">Не выбрана — специалисты работают без доступа к вашим файлам</div>{/if}
            <div class="row">
              {#if app.machines.length}<Dropdown value={d.project.node_id ?? ''} options={machineOptions} label="Компьютер с папкой" onpick={(v) => ((pickMachine = v), (picking = 'edit'))} />{/if}
              <button class="btn" onclick={() => ((pickMachine = d!.project.node_id ?? ''), (picking = 'edit'))}><Icon name="folder" size={14} />Выбрать…</button>
              {#if d.project.root_path}<button class="btn" onclick={() => save({ root_path: '', node_id: '' })}>Отвязать</button>{/if}</div>
            <p class="faint" style="font-size:12px;margin:0">Специалисты работают над проектом на том компьютере, где лежит папка: там его файлы, сервер разработки и инструменты.</p>
            <label>Компания
              <Dropdown block value={d.project.company_id ?? ''} options={[{ id: '', label: 'Без компании' }, ...app.companies.map((c) => ({ id: c.id, label: c.name }))]} label="Компания" onpick={(v) => save({ company_id: v })} /></label>
            <label>Режим по умолчанию
              <Dropdown block value={d.project.mode} options={Object.entries(MODE_LABEL).map(([k, v]) => ({ id: k, label: v.label, hint: v.hint }))} label="Режим по умолчанию" onpick={(v) => save({ mode: v })} /></label>
          </div>
          {#if error}<p class="bad">{error}</p>{/if}
        </div>
        <div class="section">
          <h2>Участники</h2>
          <ul class="list">
            {#each app.agents as a (a.id)}
              <li><button class="item" onclick={() => toggleMember(a)}>
                <Avatar name={a.name} color={a.accent} size={22} /><span class="grow">{a.name}</span>
                {#if d.members.some((m) => m.id === a.id)}<Icon name="check" size={15} />{/if}</button></li>
            {/each}
          </ul>
        </div>
        <div class="section">
          <h2>Чаты</h2>
          <ul class="list">
            {#each d.threads.slice(0, 8) as t (t.id)}
              <li><button class="item" onclick={() => go({ kind: 'chat', id: t.id })}><span class="grow sub">{t.title || 'Новый чат'}</span><span class="faint">{timeAgo(t.updated_at)}</span></button></li>
            {:else}<li class="empty-note">Чатов нет</li>{/each}
          </ul>
        </div>
      </div>
    </div>
  </Page>
{:else}
  <Page title="Проект"><p class="muted">{error || 'Загрузка…'}</p></Page>
{/if}

<style>
  /* a card reads from the top: names line up across a row whatever the text below them */
  .pcard { display: flex; flex-direction: column; justify-content: flex-start; align-items: stretch; gap: 6px; }
  .pcard p { margin: 0; }
  .pcard > .row:last-child:not(:empty) { margin-top: auto; padding-top: 4px; }
  .clamp {
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
    margin: 10px 0;
    font-size: 13px;
  }
  .bad {
    color: var(--bad);
  }
</style>
