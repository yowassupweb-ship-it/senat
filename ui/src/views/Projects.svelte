<script lang="ts">
  import TeamWork from '../components/TeamWork.svelte';
  import { coalescedRefresh } from '../lib/refresh';
  import Spinner from '../components/Spinner.svelte';
  import Dropdown from '../components/Dropdown.svelte';
  import { get, patch, post, put } from '../lib/api';
  import { app, go, onEvent } from '../lib/store.svelte';
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

  async function load(refreshOnly = false) {
    if (!id) return (d = null);
    try {
      const requested = id;
      const result = await get<typeof d>(`/api/projects/${requested}`);
      if (id !== requested) return;
      d = result;
      if (!refreshOnly) f = { goal: d!.project.goal, criteria: d!.project.criteria, constraints: d!.project.constraints };
    } catch (e: any) {
      error = e.message;
    }
  }
  $effect(() => {
    id;
    load();
    const refresh = coalescedRefresh(() => load(true), 250);
    return onEvent((type) => { if (['attention', 'session', 'thread'].includes(type)) refresh(); });
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
  // The list reads like a macOS source list: what is being worked on now first, then by last activity, so the
  // project the person returns to is at the top and the forgotten ones sink.
  let q = $state('');
  type Row = { p: Project; working: string[]; waiting: boolean; last: number };
  const rows = $derived.by(() => {
    const needle = q.trim().toLowerCase();
    const out: Row[] = [];
    for (const p of app.projects) {
      if (needle && !(p.name + ' ' + p.goal).toLowerCase().includes(needle)) continue;
      const threads = app.threads.filter((t) => t.project_id === p.id && !t.archived);
      const ids = new Set(threads.map((t) => t.id));
      const runs = app.live.filter((r) => r.workspace?.project_id === p.id || (!!r.thread_id && ids.has(r.thread_id)));
      out.push({
        p,
        working: [...new Set(runs.map((r) => r.agent_id))],
        waiting: runs.some((r) => r.awaiting) || threads.some((t) => t.awaiting),
        last: Math.max(0, ...threads.map((t) => t.updated_at)),
      });
    }
    return out.sort((a, b) => Number(b.working.length > 0) - Number(a.working.length > 0) || b.last - a.last || a.p.name.localeCompare(b.p.name));
  });
  const WEEK = 7 * 864e5;
  const groups = $derived(
    [
      { title: 'Сейчас в работе', rows: rows.filter((r) => r.working.length) },
      { title: 'Недавние', rows: rows.filter((r) => !r.working.length && r.last > Date.now() - WEEK) },
      { title: 'Остальные', rows: rows.filter((r) => !r.working.length && r.last <= Date.now() - WEEK) },
    ].filter((g) => g.rows.length),
  );
  const placeOf = (p: Project) => {
    if (!p.root_path) return null;
    const m = p.node_id ? app.machines.find((x) => x.id === p.node_id) : null;
    return { name: p.node_id ? (m?.name ?? 'компьютер') : 'сервер', online: !p.node_id || !!m?.online };
  };
  const agentName = (id: string) => app.agents.find((a) => a.id === id)?.name ?? 'специалист';
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
    {#if app.projects.length > 6}
      <div class="psearch"><Icon name="search" size={13} /><input bind:value={q} placeholder="Найти проект" aria-label="Найти проект" /></div>
    {/if}
    {#each groups as g (g.title)}
      <section class="pgroup" aria-label={g.title}>
        <h2 class="gtitle">{g.title}<span class="gcount">{g.rows.length}</span></h2>
        <ul class="plist">
          {#each g.rows as r (r.p.id)}
            {@const place = placeOf(r.p)}
            <li>
              <button class="prow" onclick={() => go({ kind: 'projects', id: r.p.id })}>
                <span class="pico" class:live={r.working.length > 0}><Icon name="folder" size={16} /></span>
                <span class="pmain">
                  <span class="pname"><span class="pn">{r.p.name}</span>{#if r.waiting}<span class="wait">ждёт вас</span>{/if}</span>
                  <span class="pgoal" class:faint={!r.p.goal}>{r.p.goal || 'Цель не задана'}</span>
                </span>
                <span class="pmeta">
                  {#if r.working.length}
                    <span class="who" title={r.working.map(agentName).join(', ')}>
                      {#each r.working.slice(0, 3) as w (w)}{@const a = app.agents.find((x) => x.id === w)}<Avatar name={a?.name} color={a?.accent} size={20} />{/each}
                      <span class="pulse" aria-hidden="true"></span>работают
                    </span>
                  {:else if r.last}
                    <span class="when">{timeAgo(r.last)}</span>
                  {/if}
                  {#if r.p.open_obligations}<span class="tag">{count(r.p.open_obligations, N.task)}</span>{/if}
                  {#if place}<span class="tag" title={r.p.root_path}><span class="dot" class:on={place.online}></span>{place.name}</span>{/if}
                </span>
                <span class="chev" aria-hidden="true"><Icon name="right" size={12} /></span>
              </button>
            </li>
          {/each}
        </ul>
      </section>
    {:else}
      <p class="empty-note">{q ? 'Ничего не нашлось.' : 'Проектов пока нет. У проекта будут цель, участники и рабочая папка.'}</p>
    {/each}
  </Page>
{:else if d}
  <Page title={d.project.name} sub={d.project.root_path || 'Без рабочей папки'} back={{ label: 'Проекты', onclick: () => go({ kind: 'projects' }) }}>
    {#snippet actions()}
      <button class="btn primary" onclick={() => go({ kind: 'new', project: id })}><Icon name="chat" size={14} />Новый чат</button>
    {/snippet}
    {#if d.project.root_path}<p class="muted">Общая папка: специалисты работают по очереди, чтобы не перезаписывать изменения друг друга.</p>{/if}
    <TeamWork obligations={d.obligations} members={d.members} />
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
  <Page title="Проект">{#if error}<p class="muted">{error}</p>{:else}<Spinner size={28} fill />{/if}</Page>
{/if}

<style>
  .psearch { display: flex; align-items: center; gap: 8px; max-width: 360px; height: 32px; margin-bottom: 18px; padding: 0 10px; border-radius: 8px; background: var(--bg-raised); border: 1px solid var(--line); color: var(--fg-3); }
  .psearch input { flex: 1; min-width: 0; background: none; border: 0; outline: 0; color: var(--fg); font: inherit; font-size: 13px; }
  .psearch:focus-within { border-color: var(--accent); }
  .pgroup + .pgroup { margin-top: 22px; }
  .gtitle { margin: 0 0 6px 14px; font-size: 12px; font-weight: 600; color: var(--fg-3); }
  .gcount { font-weight: 400; margin-left: 6px; }
  .plist { list-style: none; margin: 0; padding: 0; border-radius: 12px; background: var(--bg-raised); border: 1px solid var(--line); overflow: hidden; }
  .plist li + li .prow { border-top: 1px solid var(--line); }
  .prow { display: flex; align-items: center; gap: 12px; width: 100%; min-height: 58px; padding: 9px 14px; text-align: left; transition: background 0.12s var(--ease); }
  .prow:hover { background: var(--accent-soft); }
  .prow:active { background: var(--bg-active); }
  .prow:focus-visible { outline: 2px solid var(--accent); outline-offset: -2px; }
  .pico { flex: none; display: grid; place-items: center; width: 32px; height: 32px; border-radius: 8px; background: var(--bg-active); color: var(--fg-2); }
  .pico.live { background: var(--accent-soft); color: var(--accent); }
  .pmain { flex: 1; min-width: 0; display: grid; gap: 2px; }
  .pname { display: flex; align-items: center; gap: 8px; min-width: 0; font-weight: 600; font-size: 14px; color: var(--fg); }
  .pn { overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
  .pgoal { font-size: 12.5px; color: var(--fg-2); overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
  .pgoal.faint { color: var(--fg-3); }
  .wait { flex: none; padding: 0 7px; border-radius: 9px; background: var(--warn-soft); color: var(--warn); font-size: 11px; font-weight: 600; line-height: 18px; }
  .pmeta { flex: none; display: flex; align-items: center; gap: 8px; font-size: 12px; color: var(--fg-3); }
  .who { display: inline-flex; align-items: center; gap: 4px; color: var(--accent); font-weight: 500; }
  .pulse { width: 6px; height: 6px; margin: 0 2px 0 4px; border-radius: 50%; background: var(--ok); animation: pulse 1.6s var(--ease) infinite; }
  @keyframes pulse { 50% { opacity: 0.35; } }
  .when { font-variant-numeric: tabular-nums; }
  .tag { display: inline-flex; align-items: center; gap: 5px; padding: 0 8px; border-radius: 9px; background: var(--bg-active); color: var(--fg-2); line-height: 20px; white-space: nowrap; max-width: 140px; overflow: hidden; text-overflow: ellipsis; }
  .dot { width: 6px; height: 6px; border-radius: 50%; background: var(--fg-3); flex: none; }
  .dot.on { background: var(--ok); }
  .chev { flex: none; color: var(--fg-3); }
  @media (prefers-reduced-motion: reduce) { .pulse { animation: none; } .prow { transition: none; } }
  @media (max-width: 600px) {
    .prow { flex-wrap: wrap; }
    .pmeta { width: 100%; padding-left: 44px; flex-wrap: wrap; }
    .chev { display: none; }
  }
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
