<script lang="ts">
  import { del, get, patch, post } from '../lib/api';
  import { agentById, app, go, onEvent } from '../lib/store.svelte';
  import { dateTime } from '../lib/format';
  import { CRON_PRESETS, describeCron } from '../lib/cron';
  import type { Routine } from '../lib/types';
  import Avatar from './Avatar.svelte';
  import Icon from './Icon.svelte';
  import Switch from './Switch.svelte';

  // Work a specialist does by itself on a schedule. Each time it is due a task is made for them, and it goes the usual
  // way: they do it, report, and anything that needs you shows up under "needs your decision".
  let { agentId }: { agentId?: string } = $props();

  let list = $state<Routine[]>([]);
  let error = $state('');
  let note = $state('');
  let editing = $state<string | null>(null);
  let adding = $state(false);
  const blank = () => ({ agent_id: agentId ?? '', name: '', cron: '0 9 * * 1', title: '', prompt: '' });
  let form = $state(blank());
  const custom = $derived(!CRON_PRESETS.some((p) => p.cron === form.cron));

  async function load() {
    try {
      list = await get(`/api/routines${agentId ? `?agent=${agentId}` : ''}`);
    } catch (e: any) {
      error = e.message;
    }
  }
  $effect(() => {
    agentId;
    load();
    return onEvent((t) => {
      if (t === 'routines') load();
    });
  });

  function startEdit(r: Routine) {
    editing = r.id;
    adding = false;
    form = { agent_id: r.agent_id, name: r.name, cron: r.cron, title: r.title, prompt: r.prompt };
  }
  async function save() {
    error = '';
    try {
      if (editing) await patch(`/api/routines/${editing}`, form);
      else await post('/api/routines', form);
      editing = null;
      adding = false;
      form = blank();
      await load();
    } catch (e: any) {
      error = e.message;
    }
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
    error = '';
    try {
      const o = await post(`/api/routines/${r.id}/run`);
      note = `Запущено: ${o.title}`;
      setTimeout(() => (note = ''), 4000);
    } catch (e: any) {
      error = e.message;
    }
  }
  const when = (r: Routine, who?: string) =>
    [who ?? '—', r.enabled && r.next_run_at ? `следующий запуск ${dateTime(r.next_run_at)}` : '', r.last_run_at ? `прошлый ${dateTime(r.last_run_at)}` : ''].filter(Boolean).join(' · ');
  async function remove(r: Routine) {
    if (!confirm(`Удалить регулярную задачу «${r.name}»?`)) return;
    await del(`/api/routines/${r.id}`);
    load();
  }
</script>

<section class="rl">
  <header>
    <h2>Постоянные задачи · {list.length}</h2>
    <button class="btn primary sm" onclick={() => { adding = !adding; editing = null; form = blank(); }}><Icon name="plus" size={13} />Добавить</button>
  </header>
  <p class="muted intro">Работа, которую специалист делает сам по расписанию: обходы, отчёты, проверки. В срок для него появляется задача, а результат вы видите в «Задачах».</p>
  {#if error}<p class="bad" role="alert">{error}</p>{/if}
  {#if note}<p class="ok" role="status">{note}</p>{/if}

  {#if adding && !editing}<div class="card pad">{@render formView()}</div>{/if}

  <ul class="rows">
    {#each list as r (r.id)}
      {@const ag = agentById(r.agent_id)}
      <li class:off={!r.enabled}>
        <div class="line">
          <Avatar name={ag?.name} color={ag?.accent} size={28} />
          <div class="grow">
            <div class="ttl"><b>{r.name}</b>
              <span class="chip">{describeCron(r.cron)}</span>
              {#if r.source.startsWith('skill:')}<span class="chip" title="Объявлена навыком. Время и включение можно менять, перезапуск этого не сбрасывает.">навык {r.source.slice(6)}</span>{/if}
              {#if !r.enabled}<span class="chip">выключена</span>{/if}</div>
            <div class="meta">{when(r, ag?.name)}</div>
            {#if r.prompt}<div class="meta clip">{r.prompt}</div>{/if}
          </div>
          <div class="side">
            <Switch on={r.enabled} label="Включить {r.name}" onchange={(v) => toggle(r, v)} />
            <button class="btn sm" onclick={() => runNow(r)} title="Создать задачу сейчас, расписание не меняется">Запустить сейчас</button>
            <button class="btn sm" onclick={() => startEdit(r)}>Изменить</button>
            <button class="icon-btn" onclick={() => remove(r)} aria-label="Удалить {r.name}" title="Удалить"><Icon name="trash" size={15} /></button>
          </div>
        </div>
        {#if editing === r.id}{@render formView()}{/if}
      </li>
    {:else}
      <li class="empty">Постоянных задач пока нет. Добавьте вручную или положите в папку навыка специалиста файл <code>routines.json</code>: расписание появится само.</li>
    {/each}
  </ul>
</section>

{#snippet formView()}
  <form class="form" onsubmit={(e) => { e.preventDefault(); save(); }}>
    <div class="two">
      <label>Специалист
        <select class="field" bind:value={form.agent_id} required disabled={!!agentId || !!editing}><option value="" disabled>Выберите…</option>{#each app.agents as a}<option value={a.id}>{a.name}</option>{/each}</select></label>
      <label>Название<input class="field" bind:value={form.name} required placeholder="Утренний обход" /></label>
    </div>
    <div class="two">
      <label>Когда
        <select class="field" value={custom ? '' : form.cron} onchange={(e) => { if (e.currentTarget.value) form.cron = e.currentTarget.value; }}>
          {#each CRON_PRESETS as p}<option value={p.cron}>{p.label}</option>{/each}
          <option value="">Своё расписание…</option>
        </select></label>
      <label>Расписание (cron) <small>минуты часы число месяц день_недели · по времени этого компьютера</small>
        <input class="field mono" bind:value={form.cron} required placeholder="0 9 * * 1" /></label>
    </div>
    <p class="faint hint">Сейчас: {describeCron(form.cron)}</p>
    <label>Заголовок задачи <small>к нему добавится дата</small><input class="field" bind:value={form.title} placeholder="по умолчанию — название" /></label>
    <label>Что сделать и каким должен быть результат<textarea class="field" rows="4" bind:value={form.prompt} placeholder="Пройди по главным страницам сайта, проверь, что открываются, и коротко опиши, что сломано."></textarea></label>
    <div class="row"><button class="btn primary" type="submit">{editing ? 'Сохранить' : 'Добавить'}</button><button class="btn" type="button" onclick={() => { editing = null; adding = false; }}>Отмена</button></div>
  </form>
{/snippet}

<style>
  .rl { display: flex; flex-direction: column; gap: 10px; }
  header { display: flex; align-items: center; justify-content: space-between; gap: 10px; }
  h2 { margin: 0; font-size: 14.5px; font-weight: 650; }
  .intro { margin: 0; font-size: 13px; line-height: 1.5; max-width: 720px; }
  .bad { color: var(--bad); margin: 0; font-size: 13px; }
  .ok { color: var(--ok); margin: 0; font-size: 13px; }
  .rows { list-style: none; margin: 0; padding: 0; background: var(--bg-raised); border-radius: var(--radius); box-shadow: var(--shadow-1); overflow: hidden; }
  .rows li { padding: 12px 16px; border-top: 1px solid var(--line); }
  .rows li:first-child { border-top: 0; }
  .rows li.off :global(.av) { opacity: 0.55; }
  .rows li.off .grow > .ttl > b { color: var(--fg-2); }
  .rows li.empty { color: var(--fg-3); font-size: 13.5px; }
  .line { display: flex; gap: 12px; align-items: flex-start; }
  .grow { flex: 1; min-width: 0; }
  .ttl { display: flex; align-items: center; gap: 6px; flex-wrap: wrap; }
  .meta { color: var(--fg-2); font-size: 12.5px; margin-top: 2px; }
  .meta.clip { overflow: hidden; display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; color: var(--fg-3); }
  .side { display: flex; align-items: center; gap: 8px; flex: none; flex-wrap: wrap; justify-content: flex-end; }
  .form { display: flex; flex-direction: column; gap: 12px; padding: 14px 0 4px; }
  .card.pad .form { padding: 0; }
  .form label { display: flex; flex-direction: column; gap: 4px; font-size: 13px; font-weight: 600; }
  .form label small { font-weight: 400; color: var(--fg-3); }
  .two { display: grid; grid-template-columns: 1fr 1fr; gap: 12px; }
  .hint { margin: -4px 0 0; font-size: 12.5px; }
  code { font: 12px var(--mono); padding: 1px 5px; border-radius: 5px; background: var(--bg-hover); }
  @media (max-width: 760px) { .two { grid-template-columns: 1fr; } .line { flex-wrap: wrap; } }
</style>
