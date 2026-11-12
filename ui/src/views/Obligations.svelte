<script lang="ts">
  import { get, patch, post } from '../lib/api';
  import { agentById, app, go, onEvent, projectById } from '../lib/store.svelte';
  import { dateTime, STATE_LABEL, timeAgo } from '../lib/format';
  import type { Obligation } from '../lib/types';
  import Avatar from '../components/Avatar.svelte';
  import Icon from '../components/Icon.svelte';
  import Page from '../components/Page.svelte';
  import RoutineList from '../components/RoutineList.svelte';
  import TaskCalendar from '../components/TaskCalendar.svelte';

  let { id }: { id?: string } = $props();

  let all = $state<Obligation[]>([]);
  let creating = $state(false);
  let draft = $state({ title: '', result: '', criteria: '', owner_id: '', project_id: '' });
  let error = $state('');
  let showDone = $state(false);
  // the list of tasks, the calendar of what is planned, and the schedules themselves
  let tab = $state<'list' | 'calendar' | 'routines'>('list');

  async function loadAll() {
    all = await get('/api/obligations?all=1');
  }
  $effect(() => {
    loadAll();
    return onEvent((t) => {
      if (t === 'attention' || t === 'session') {
        loadAll();
        if (id) loadDetail();
      }
    });
  });

  let detail = $state<{ obligation: Obligation; journal: any[]; sessions: any[]; children: Obligation[]; thread_id: string | null } | null>(null);
  let answer = $state('');
  async function loadDetail() {
    if (!id) return (detail = null);
    try {
      detail = await get(`/api/obligations/${id}`);
    } catch (e: any) {
      error = e.message;
    }
  }
  $effect(() => {
    id;
    answer = '';
    loadDetail();
  });

  async function create() {
    try {
      const o = await post('/api/obligations', { ...draft, owner_id: draft.owner_id || null, project_id: draft.project_id || null, requested_by: 'user' });
      creating = false;
      draft = { title: '', result: '', criteria: '', owner_id: '', project_id: '' };
      await loadAll();
      go({ kind: 'obligations', id: o.id });
    } catch (e: any) {
      error = e.message;
    }
  }
  async function transition(to: string, extra: Record<string, unknown> = {}) {
    try {
      await post(`/api/obligations/${id}/transition`, { to, ...extra });
      error = '';
      await Promise.all([loadAll(), loadDetail()]);
    } catch (e: any) {
      error = e.message;
    }
  }
  async function reply() {
    await post(`/api/obligations/${id}/answer`, { answer });
    answer = '';
    await Promise.all([loadAll(), loadDetail()]);
  }
  async function reassign(to: string) {
    await post(`/api/obligations/${id}/reassign`, { to });
    loadDetail();
  }
  async function decideApproval(a: any, approve: boolean) {
    await post(`/api/approvals/${a.id}/decide`, { approve });
  }
  async function decideDecision(d: any, status: string) {
    await post(`/api/decisions/${d.id}/status`, { status });
  }
  function cancel() {
    const reason = prompt('Причина отмены');
    if (reason) transition('cancelled', { outcome: reason });
  }

  const open = $derived(all.filter((o) => o.state !== 'done' && o.state !== 'cancelled'));
  const finished = $derived(all.filter((o) => o.state === 'done' || o.state === 'cancelled'));
  const att = $derived(app.attention);
  const needs = $derived(att.approvals.length + att.needs_user.length + att.proposed_decisions.length);
  const ownerName = (o: Obligation) => agentById(o.owner_id)?.name ?? 'без ответственного';
  const waitText = (w: any) => (!w ? '' : w.type === 'time' ? `до ${dateTime(w.at)}` : w.type === 'user' ? 'ответа пользователя' : w.type === 'obligation' ? 'другой задачи' : `события ${w.kind}`);
  const actionLabel: Record<string, string> = {
    'obligation.delegate': 'Поручить коллеге',
    'obligation.create': 'Принять поручение',
    'memory.promote': 'В общую память',
    'decision.accept': 'Принять решение',
    'external.send': 'Отправить наружу',
    'profile.change': 'Изменить профиль',
    'project.create': 'Завести проект',
    'mcp.install': 'Установить MCP',
    'api.connect': 'Подключить API',
    'access.use': 'Выдать доступ',
  };
</script>

{#snippet row(o: Obligation)}
  <li><button class="item" onclick={() => go({ kind: 'obligations', id: o.id })}>
    <span class="dot {o.state}"></span>
    <span class="grow">
      <span class="title">{o.title}</span>
      <div class="sub">{ownerName(o)}{#if o.project_id} · {projectById(o.project_id)?.name}{/if}{#if o.state === 'blocked' && o.blocker} · {o.blocker}{:else if o.next_step} · дальше: {o.next_step}{/if}</div>
    </span>
    <span class="chip {o.state === 'blocked' ? 'bad' : o.state === 'review' ? 'warn' : o.state === 'running' ? 'accent' : o.state === 'done' ? 'ok' : ''}">{STATE_LABEL[o.state]}</span>
    <span class="faint when">{timeAgo(o.updated_at)}</span>
  </button></li>
{/snippet}

{#if id && detail}
  {@const o = detail.obligation}
  <Page title={o.title} sub="{ownerName(o)}{o.project_id ? ' · ' + projectById(o.project_id)?.name : ''}">
    {#snippet actions()}<button class="btn" onclick={() => go({ kind: 'obligations' })}>Все задача</button>{/snippet}
    <div class="split">
      <div>
        {#if o.state === 'blocked' || o.state === 'review' || (o.state === 'waiting' && o.wait_condition?.type === 'user')}
          <div class="card pad form hot" style="margin-bottom:18px">
            {#if o.state === 'blocked'}<b>Работа остановлена</b><p class="muted">{o.blocker}</p>
            {:else if o.state === 'review'}<b>Результат ждёт проверки</b><p class="muted">{o.outcome || 'Описание результата не приложено'}</p>
            {:else}<b>Специалист ждёт вашего ответа</b>{/if}
            {#if o.state === 'review'}
              <div class="row"><button class="btn primary" onclick={() => transition('done')}>Принять</button>
                <button class="btn" onclick={() => transition('accepted', { note: 'возвращено на доработку' })}>Вернуть в работу</button></div>
            {:else}
              <div class="row"><input class="field" bind:value={answer} placeholder="Ответ, уточнение или решение" onkeydown={(e) => e.key === 'Enter' && answer.trim() && reply()} />
                <button class="btn primary" disabled={!answer.trim()} onclick={reply}>Продолжить</button></div>
            {/if}
          </div>
        {/if}

        <div class="section">
          <h2>Содержание</h2>
          <dl class="card pad kv">
            <dt>Состояние</dt><dd><span class="chip">{STATE_LABEL[o.state]}</span>{#if o.state === 'waiting'} ждёт {waitText(o.wait_condition)}{/if}</dd>
            {#if o.result}<dt>Результат</dt><dd>{o.result}</dd>{/if}
            {#if o.criteria}<dt>Критерии</dt><dd>{o.criteria}</dd>{/if}
            {#if o.next_step}<dt>Следующий шаг</dt><dd>{o.next_step}</dd>{/if}
            {#if o.outcome}<dt>Итог</dt><dd>{o.outcome}</dd>{/if}
            {#if o.due_at}<dt>Срок</dt><dd>{dateTime(o.due_at)}</dd>{/if}
            <dt>Поставил</dt><dd>{o.requested_by === 'user' ? 'вы' : (agentById(o.requested_by)?.name ?? o.requested_by)}</dd>
            {#if o.spent_tokens}<dt>Расход</dt><dd>{o.spent_tokens} токенов</dd>{/if}
          </dl>
        </div>

        {#if detail.children.length}
          <div class="section"><h2>Делегировано</h2><ul class="list">{#each detail.children as c (c.id)}{@render row(c)}{/each}</ul></div>
        {/if}

        <div class="section">
          <h2>Журнал</h2>
          <ul class="list">
            {#each detail.journal as e (e.id)}
              <li class="item"><span class="grow sub">{e.kind.replace('obligation.', '')}{#if e.payload?.note} — {e.payload.note}{/if}</span><span class="faint">{dateTime(e.ts)}</span></li>
            {/each}
          </ul>
        </div>
      </div>

      <div>
        <div class="section">
          <h2>Действия</h2>
          <div class="card pad form">
            {#if detail.thread_id}<button class="btn" onclick={() => go({ kind: 'chat', id: detail!.thread_id! })}><Icon name="chat" size={14} />Открыть ход работы</button>{/if}
            {#if o.state !== 'done' && o.state !== 'cancelled'}
              <label>Ответственный
                <select class="field" value={o.owner_id ?? ''} onchange={(e) => reassign(e.currentTarget.value)}>
                  {#each app.agents as a}<option value={a.id}>{a.name}</option>{/each}</select></label>
              {#if o.state === 'blocked'}<button class="btn" onclick={() => transition('accepted', { note: 'повторный запуск' })}>Запустить снова</button>{/if}
              <button class="btn" onclick={() => transition('done', { outcome: prompt('Что получено?') || '' })}>Отметить выполненным</button>
              <button class="btn danger" onclick={cancel}>Отменить</button>
            {/if}
            {#if error}<p class="bad">{error}</p>{/if}
          </div>
        </div>
        <div class="section">
          <h2>Сессии <span class="count">{detail.sessions.length}</span></h2>
          <ul class="list">
            {#each detail.sessions as s (s.id)}
              <li class="item"><span class="chip {s.status === 'done' ? 'ok' : s.status === 'failed' ? 'bad' : ''}">{s.status}</span><span class="grow sub">{s.reason}</span><span class="faint">{timeAgo(s.created_at)}</span></li>
            {:else}<li class="empty-note">Запусков не было</li>{/each}
          </ul>
        </div>
      </div>
    </div>
  </Page>
{:else}
  <Page title="Задача">
    {#snippet actions()}<button class="btn primary" onclick={() => (creating = !creating)}><Icon name="plus" size={14} />Новая задача</button>{/snippet}

    <div class="tabbar" role="tablist" aria-label="Раздел задач">
      <button role="tab" aria-selected={tab === 'list'} class:on={tab === 'list'} onclick={() => (tab = 'list')}>Задачи</button>
      <button role="tab" aria-selected={tab === 'calendar'} class:on={tab === 'calendar'} onclick={() => (tab = 'calendar')}><Icon name="clock" size={13} />Календарь</button>
      <button role="tab" aria-selected={tab === 'routines'} class:on={tab === 'routines'} onclick={() => (tab = 'routines')}><Icon name="refresh" size={13} />Постоянные</button>
    </div>

    {#if tab === 'calendar'}
      <TaskCalendar />
    {:else if tab === 'routines'}
      <RoutineList />
    {:else}

    {#if creating}
      <form class="card pad form" style="margin-bottom:18px" onsubmit={(e) => { e.preventDefault(); create(); }}>
        <label>Что нужно сделать<input class="field" bind:value={draft.title} required /></label>
        <label>Ожидаемый результат<textarea class="field" rows="2" bind:value={draft.result}></textarea></label>
        <label>Критерии завершения <small>по ним результат будут проверять</small><textarea class="field" rows="2" bind:value={draft.criteria}></textarea></label>
        <div class="row">
          <label style="flex:1">Ответственный
            <select class="field" bind:value={draft.owner_id} required><option value="" disabled>Выберите…</option>{#each app.agents as a}<option value={a.id}>{a.name}</option>{/each}</select></label>
          <label style="flex:1">Проект
            <select class="field" bind:value={draft.project_id}><option value="">Без проекта</option>{#each app.projects as p}<option value={p.id}>{p.name}</option>{/each}</select></label>
        </div>
        <div class="row"><button class="btn primary" type="submit">Поручить</button><button class="btn" type="button" onclick={() => (creating = false)}>Отмена</button></div>
        {#if error}<p class="bad">{error}</p>{/if}
      </form>
    {/if}

    {#if needs}
      <div class="section">
        <h2>Требует вашего решения <span class="count">{needs}</span></h2>
        <ul class="list">
          {#each att.approvals as a (a.id)}
            <li class="item">
              <Avatar name={agentById(a.agent_id)?.name} color={agentById(a.agent_id)?.accent} size={26} />
              <span class="grow"><span class="title">{actionLabel[a.action] ?? a.action}</span>
                <div class="sub">{agentById(a.agent_id)?.name}: {a.detail?.args?.title ?? a.detail?.args?.body ?? ''}</div></span>
              <button class="btn sm primary" onclick={() => decideApproval(a, true)}>Разрешить</button>
              <button class="btn sm" onclick={() => decideApproval(a, false)}>Отклонить</button>
            </li>
          {/each}
          {#each att.proposed_decisions as d (d.id)}
            <li class="item"><Icon name="flag" size={16} /><span class="grow"><span class="title">{d.title}</span><div class="sub">Предложено решение · {projectById(d.project_id)?.name}</div></span>
              <button class="btn sm primary" onclick={() => decideDecision(d, 'accepted')}>Принять</button>
              <button class="btn sm" onclick={() => decideDecision(d, 'rejected')}>Отклонить</button></li>
          {/each}
          {#each att.needs_user as o (o.id)}{@render row(o)}{/each}
        </ul>
      </div>
    {/if}

    <div class="section">
      <h2>В работе <span class="count">{open.length}</span></h2>
      <ul class="list">
        {#each open as o (o.id)}{@render row(o)}{:else}<li class="empty-note">Открытых задач нет. Поручите что-нибудь специалисту — и оно появится здесь.</li>{/each}
      </ul>
    </div>

    {#if finished.length}
      <div class="section">
        <h2><button onclick={() => (showDone = !showDone)} style="display:inline-flex;gap:6px;align-items:center;color:inherit;font:inherit"><Icon name={showDone ? 'down' : 'right'} size={12} />Завершённые <span class="count">{finished.length}</span></button></h2>
        {#if showDone}<ul class="list">{#each finished.slice(0, 50) as o (o.id)}{@render row(o)}{/each}</ul>{/if}
      </div>
    {/if}
    {/if}
  </Page>
{/if}

<style>
  .tabbar { display: inline-flex; padding: 3px; gap: 2px; border-radius: 10px; background: var(--bg-hover); margin: 0 0 18px; }
  .tabbar button { display: inline-flex; align-items: center; gap: 6px; padding: 5px 14px; border-radius: 8px; font-size: 13px; color: var(--fg-2); }
  .tabbar button:hover { color: var(--fg); }
  .tabbar button.on { background: var(--bg-raised); color: var(--fg); box-shadow: var(--shadow-1); font-weight: 600; }
  .tabbar button:focus-visible { outline: none; box-shadow: var(--focus-ring); }
  .hot {
    box-shadow:
      0 0 0 1.5px var(--warn),
      var(--shadow-1);
  }
  .hot p {
    margin: 0;
  }
  .when {
    font-size: 12px;
    width: 52px;
    text-align: right;
  }
  .bad {
    color: var(--bad);
  }
</style>
