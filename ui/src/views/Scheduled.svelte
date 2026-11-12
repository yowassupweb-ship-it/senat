<script lang="ts">
  import { del, get, patch, post } from '../lib/api';
  import { agentById, app, onEvent } from '../lib/store.svelte';
  import { dateTime } from '../lib/format';
  import { describeCron } from '../lib/cron';
  import type { Routine } from '../lib/types';
  import Avatar from '../components/Avatar.svelte';
  import Dropdown from '../components/Dropdown.svelte';
  import Icon from '../components/Icon.svelte';
  import Page from '../components/Page.svelte';
  import { ctx, type MenuItem } from '../lib/contextmenu.svelte';
  import SchedulePicker from '../components/SchedulePicker.svelte';
  import TaskCalendar from '../components/TaskCalendar.svelte';
  import Switch from '../components/Switch.svelte';

  // "Запланировано": a specialist does something by itself on a schedule. Describe it in a line, pick who and when.
  let list = $state<Routine[]>([]);
  let error = $state('');
  let note = $state('');
  let filter = $state<'active' | 'all' | 'off'>('active');
  let filterOpen = $state(false);
  let view = $state<'list' | 'month'>((() => { try { return localStorage.getItem('senate.sched.view') === 'month' ? 'month' : 'list'; } catch { return 'list'; } })());
  function setView(v: 'list' | 'month') {
    view = v;
    try { localStorage.setItem('senate.sched.view', v); } catch { /* storage unavailable */ }
  }
  let text = $state('');
  let agentId = $state('');
  let cron = $state('0 9 * * 1');
  let sugOpen = $state(true);
  let menu = $state<string | null>(null);
  let editing = $state<string | null>(null);
  let edit = $state({ name: '', cron: '', prompt: '' });

  const SUGGESTIONS: { icon: string; title: string; text: string; cron: string }[] = [
    { icon: 'folder', title: 'Сводка по проектам', text: 'Раз в неделю свожи, что сделано по проектам, что застряло и что ждёт моего решения.', cron: '0 9 * * 1' },
    { icon: 'checks', title: 'Просроченные задачи', text: 'Каждое утро находи просроченные задачи и называй, что их блокирует.', cron: '0 9 * * 1-5' },
    { icon: 'book', title: 'Порядок в памяти', text: 'Раз в неделю проверяй память: устаревшие и противоречивые записи, предлагай исправления.', cron: '0 9 * * 4' },
    { icon: 'globe', title: 'Проверка сайта', text: 'Каждый день проходи по главным страницам сайта и сообщай, что не открывается или сломано.', cron: '0 9 * * *' },
    { icon: 'sparkle', title: 'Изменения моделей', text: 'Каждый понедельник выделяй изменения у провайдеров моделей, которые влияют на Senate.', cron: '0 9 * * 1' },
    { icon: 'clock', title: 'План на неделю', text: 'В понедельник составляй план недели по проектам и срокам и присылай на согласование.', cron: '0 9 * * 1' },
  ];

  async function load() {
    try {
      list = await get('/api/routines');
      error = '';
    } catch (e: any) {
      error = String(e.message || e).includes('JSON') ? 'Эта нода не поддерживает расписание: обновите ноду.' : e.message;
    }
  }
  $effect(() => {
    if (!agentId && app.agents.length) agentId = app.agents[0].id;
  });
  $effect(() => {
    load();
    return onEvent((t) => {
      if (t === 'routines') load();
    });
  });

  const agentOpts = $derived(app.agents.map((a) => ({ id: a.id, label: a.name })));
  const shown = $derived(list.filter((r) => (filter === 'all' ? true : filter === 'active' ? r.enabled : !r.enabled)));
  const filterLabel = { active: 'Активно', all: 'Все', off: 'Выключено' } as const;

  async function add(name: string, prompt: string, c: string, who = agentId) {
    error = '';
    if (!who) {
      error = 'Сначала заведите специалиста.';
      return false;
    }
    try {
      await post('/api/routines', { agent_id: who, name: name.slice(0, 60), cron: c, title: name.slice(0, 80), prompt });
      await load();
      return true;
    } catch (e: any) {
      error = e.message;
      return false;
    }
  }
  async function submit(e: Event) {
    e.preventDefault();
    const t = text.trim();
    if (!t) return;
    if (await add(t.split(/[.\n]/)[0], t, cron)) text = '';
  }
  async function toggle(r: Routine, enabled: boolean) {
    try {
      await patch(`/api/routines/${r.id}`, { enabled });
      await load();
    } catch (e: any) {
      error = e.message;
    }
  }
  async function runNow(r: Routine) {
    menu = null;
    try {
      const o = await post(`/api/routines/${r.id}/run`);
      note = `Запущено: ${o.title}`;
      setTimeout(() => (note = ''), 4000);
    } catch (e: any) {
      error = e.message;
    }
  }
  function startEdit(r: Routine) {
    menu = null;
    editing = r.id;
    edit = { name: r.name, cron: r.cron, prompt: r.prompt };
  }
  async function saveEdit() {
    if (!editing) return;
    try {
      await patch(`/api/routines/${editing}`, edit);
      editing = null;
      await load();
    } catch (e: any) {
      error = e.message;
    }
  }
  async function remove(r: Routine) {
    menu = null;
    if (!confirm(`Удалить «${r.name}»?`)) return;
    await del(`/api/routines/${r.id}`);
    load();
  }
  const routineMenu = (r: Routine): MenuItem[] => [
    { label: 'Запустить сейчас', icon: 'refresh', run: () => runNow(r) },
    { label: 'Изменить', icon: 'pencil', run: () => startEdit(r) },
    { label: r.enabled ? 'Выключить' : 'Включить', icon: 'clock', run: () => toggle(r, !r.enabled) },
    { sep: true },
    { label: 'Копировать описание', icon: 'copy', run: () => navigator.clipboard?.writeText(r.prompt || r.title) },
    { sep: true },
    { label: 'Удалить', icon: 'trash', danger: true, run: () => remove(r) },
  ];
  const when = (r: Routine) => (r.enabled && r.next_run_at ? `Следующий запуск ${dateTime(r.next_run_at)}` : r.last_run_at ? `Прошлый запуск ${dateTime(r.last_run_at)}` : '');
</script>

<svelte:window onclick={() => { menu = null; filterOpen = false; }} />

<Page title="Запланировано" sub="Попросите специалиста запланировать задачи, установить напоминания или отслеживать обновления.">
  {#snippet actions()}
    <div class="seg" role="tablist" aria-label="Вид">
      <button role="tab" aria-selected={view === 'list'} class:on={view === 'list'} onclick={() => setView('list')}><Icon name="plus" size={14} />Новая задача</button>
      <button role="tab" aria-selected={view === 'month'} class:on={view === 'month'} onclick={() => setView('month')}><Icon name="clock" size={14} />Календарь</button>
    </div>
    <div class="fwrap" class:ghost={view !== 'list'} aria-hidden={view !== 'list'} inert={view !== 'list'}>
      <button class="pill" onclick={(e) => { e.stopPropagation(); filterOpen = !filterOpen; }} aria-haspopup="menu" aria-expanded={filterOpen}><Icon name="list" size={14} />{filterLabel[filter]}</button>
      {#if filterOpen}
        <div class="menu" role="menu">
          {#each Object.entries(filterLabel) as [k, l]}
            <button role="menuitemradio" aria-checked={filter === k} onclick={() => (filter = k as typeof filter)}>{l}{#if filter === k}<Icon name="check" size={14} />{/if}</button>
          {/each}
        </div>
      {/if}
    </div>
  {/snippet}

  <div class="wrap" class:wide={view === 'month'}>
    {#if view === 'list'}
    <form class="compose" onsubmit={submit}>
      <span class="plus" aria-hidden="true"><Icon name="plus" size={18} /></span>
      <input bind:value={text} placeholder="Запланируйте задачу" aria-label="Что запланировать" />
      <div class="chipdd"><Dropdown value={agentId} options={agentOpts} label="Специалист" align="right" onpick={(id) => (agentId = id)} /></div>
      <SchedulePicker value={cron} onchange={(c) => (cron = c)} />
      <button class="send" type="submit" disabled={!text.trim()} aria-label="Запланировать"><Icon name="up" size={18} /></button>
    </form>
    {/if}


    {#if error}<p class="bad" role="alert">{error}</p>{/if}
    {#if note}<p class="ok" role="status">{note}</p>{/if}

    {#if view === 'month'}
      <TaskCalendar />
    {:else}
    {#if shown.length}
      <ul class="rows">
        {#each shown as r (r.id)}
          {@const ag = agentById(r.agent_id)}
          <li class:off={!r.enabled} use:ctx={() => routineMenu(r)}>
            <div class="line">
              <Avatar name={ag?.name} color={ag?.accent} size={34} />
              <div class="main">
                <div class="ttl">{r.name}</div>
                <div class="desc">{r.prompt || r.title}</div>
                <div class="meta">{ag?.name ?? '—'} · {describeCron(r.cron)}{when(r) ? ` · ${when(r)}` : ''}</div>
              </div>
              <Switch on={r.enabled} label="Включить {r.name}" onchange={(v) => toggle(r, v)} />
              <div class="mwrap">
                <button class="icon-btn" aria-label="Действия" aria-haspopup="menu" onclick={(e) => { e.stopPropagation(); menu = menu === r.id ? null : r.id; }}><Icon name="more" size={16} /></button>
                {#if menu === r.id}
                  <div class="menu" role="menu">
                    <button role="menuitem" onclick={() => runNow(r)}>Запустить сейчас</button>
                    <button role="menuitem" onclick={() => startEdit(r)}>Изменить</button>
                    <button role="menuitem" class="danger" onclick={() => remove(r)}>Удалить</button>
                  </div>
                {/if}
              </div>
            </div>
            {#if editing === r.id}
              <form class="edit" onsubmit={(e) => { e.preventDefault(); saveEdit(); }}>
                <input class="field" bind:value={edit.name} required aria-label="Название" />
                <SchedulePicker block value={edit.cron} onchange={(c) => (edit.cron = c)} />
                <textarea class="field" rows="3" bind:value={edit.prompt} aria-label="Что сделать"></textarea>
                <div class="row"><button class="btn primary" type="submit">Сохранить</button><button class="btn" type="button" onclick={() => (editing = null)}>Отмена</button></div>
              </form>
            {/if}
          </li>
        {/each}
      </ul>
    {:else if !error}
      <p class="empty muted">{filter === 'active' ? 'Активных задач по расписанию пока нет. Опишите задачу выше или выберите из рекомендуемых.' : 'Здесь пусто.'}</p>
    {/if}

    <button class="sug-h" onclick={() => (sugOpen = !sugOpen)} aria-expanded={sugOpen}>Рекомендуемые <Icon name={sugOpen ? 'down' : 'right'} size={14} /></button>
    {#if sugOpen}
      <ul class="sug">
        {#each SUGGESTIONS as s (s.title)}
          <li>
            <span class="ic"><Icon name={s.icon} size={18} /></span>
            <div class="main"><div class="ttl">{s.title}</div><div class="desc one">{s.text}</div></div>
            <button class="add" aria-label={`Запланировать: ${s.title}`} title="Запланировать" onclick={() => add(s.title, s.text, s.cron)}><Icon name="plus" size={18} /></button>
          </li>
        {/each}
      </ul>
    {/if}
    {/if}
  </div>
</Page>

<style>
  .wrap { max-width: 860px; display: flex; flex-direction: column; gap: 18px; padding-top: 6px; }
  .wrap.wide { max-width: none; }
  .seg { display: inline-flex; padding: 3px; border-radius: 999px; background: var(--bg-raised); box-shadow: inset 0 0 0 1px var(--line); margin-right: 8px; }
  .seg button { display: inline-flex; align-items: center; gap: 6px; height: 28px; padding: 0 12px; border-radius: 999px; font-size: 13px; color: var(--fg-2); }
  .seg button.on { background: var(--bg-active); color: var(--fg); font-weight: 600; }
  .compose { display: flex; align-items: center; gap: 8px; padding: 8px 8px 8px 16px; border-radius: 999px; background: var(--bg-raised); box-shadow: inset 0 0 0 1px var(--line); }
  .compose:focus-within { box-shadow: inset 0 0 0 1px var(--line-strong); }
  .plus { color: var(--fg-2); display: grid; }
  .compose input { flex: 1; min-width: 0; background: none; border: 0; outline: none; box-shadow: none; color: var(--fg); font: inherit; font-size: 15px; height: 40px; }
  .chipdd :global(.trigger) { height: 32px; padding: 0 12px; border-radius: 999px; background: var(--bg-hover); color: var(--fg-2); font-size: 12.5px; }
  .chipdd :global(.trigger:hover), .chipdd :global(.trigger[aria-expanded='true']) { color: var(--fg); background: var(--bg-active); }
  .chipdd :global(.trigger:focus-visible) { outline: none; box-shadow: inset 0 0 0 1px var(--fg-3); }
  .chipdd :global(.tl) { max-width: 190px; }
  .send { width: 38px; height: 38px; border-radius: 50%; display: grid; place-items: center; background: var(--fg); color: var(--bg); flex: none; }
  .send:disabled { opacity: 0.35; cursor: default; }
  .pill { display: inline-flex; align-items: center; gap: 6px; height: 34px; padding: 0 14px; border-radius: 999px; background: var(--bg-raised); box-shadow: inset 0 0 0 1px var(--line); font-weight: 600; font-size: 13px; }
  .pill:hover { background: var(--bg-hover); }
  .fwrap, .mwrap { position: relative; }
  .fwrap.ghost { visibility: hidden; }
  .menu { position: absolute; right: 0; top: calc(100% + 6px); z-index: 30; min-width: 170px; padding: 6px; border-radius: 12px; background: var(--bg-raised); box-shadow: var(--shadow-2), inset 0 0 0 1px var(--line); display: flex; flex-direction: column; }
  .menu button { display: flex; justify-content: space-between; align-items: center; gap: 12px; padding: 8px 10px; border-radius: 8px; text-align: left; font-size: 13.5px; }
  .menu button:hover { background: var(--bg-hover); }
  .menu .danger { color: var(--bad); }
  .rows, .sug { list-style: none; margin: 0; padding: 0; }
  .rows li, .sug li { border-top: 1px solid var(--line); padding: 14px 4px; }
  .rows li:first-child, .sug li:first-child { border-top: 0; }
  .rows li.off .main { opacity: 0.55; }
  .line, .sug li { display: flex; align-items: center; gap: 14px; }
  .main { flex: 1; min-width: 0; }
  .ttl { font-weight: 650; font-size: 15px; }
  .desc { color: var(--fg-2); font-size: 13.5px; line-height: 1.45; margin-top: 2px; display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; }
  .desc.one { -webkit-line-clamp: 1; line-clamp: 1; }
  .meta { color: var(--fg-3); font-size: 12.5px; margin-top: 4px; }
  .sug-h { display: inline-flex; align-items: center; gap: 6px; align-self: flex-start; color: var(--fg-2); font-size: 14px; padding: 4px 6px; margin-top: 6px; }
  .sug-h:hover { color: var(--fg); }
  .ic { width: 34px; height: 34px; display: grid; place-items: center; color: var(--fg-2); flex: none; }
  .add { width: 36px; height: 36px; border-radius: 50%; display: grid; place-items: center; color: var(--fg-2); flex: none; }
  .add:hover { background: var(--bg-hover); color: var(--fg); }
  .edit { display: grid; gap: 8px; padding: 12px 0 0 48px; }
  .bad { color: var(--bad); margin: 0; font-size: 13px; }
  .ok { color: var(--ok); margin: 0; font-size: 13px; }
  .empty { margin: 0; padding: 8px 4px; font-size: 14px; }
  @media (max-width: 760px) { .compose { flex-wrap: wrap; border-radius: 20px; } .chipdd { flex: 1; } }
</style>
