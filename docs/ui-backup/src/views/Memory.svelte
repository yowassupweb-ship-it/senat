<script lang="ts">
  import { get, post } from '../lib/api';
  import { agentById, app, projectById } from '../lib/store.svelte';
  import { timeAgo } from '../lib/format';
  import type { Memory } from '../lib/types';
  import Page from '../components/Page.svelte';

  let level = $state<'all' | 'personal' | 'project' | 'collective'>('all');
  let q = $state('');
  let items = $state<Memory[]>([]);
  let adding = $state(false);
  let draft = $state({ body: '', level: 'collective', kind: 'preference', source: 'user', project_id: '', agent_id: '' });
  let error = $state('');

  async function load() {
    try {
      const params = new URLSearchParams();
      if (q.trim()) params.set('q', q.trim());
      if (level !== 'all') params.set('level', level);
      items = await get(`/api/memory?${params}`);
    } catch (e: any) {
      error = e.message;
    }
  }
  $effect(() => {
    level;
    q;
    const t = setTimeout(load, q ? 200 : 0);
    return () => clearTimeout(t);
  });

  const kindLabel: Record<string, string> = { observation: 'наблюдение', hypothesis: 'гипотеза', verified: 'проверено', decision: 'решение', preference: 'предпочтение', error: 'ошибка' };
  const levelLabel: Record<string, string> = { personal: 'личная', project: 'проект', collective: 'общая' };

  async function act(m: Memory, what: 'verify' | 'promote' | 'correct' | 'archive') {
    try {
      if (what === 'verify') {
        const basis = prompt('Основание проверки: данные, воспроизводимый результат, независимая оценка или ваше решение');
        if (!basis) return;
        await post(`/api/memory/${m.id}/verify`, { basis });
      } else if (what === 'correct') {
        const body = prompt('Исправленная формулировка', m.body);
        if (!body || body === m.body) return;
        await post(`/api/memory/${m.id}/correct`, { body });
      } else if (what === 'archive') {
        if (!confirm('Убрать запись из активного использования?')) return;
        await post(`/api/memory/${m.id}/archive`);
      } else await post(`/api/memory/${m.id}/promote`);
      error = '';
      load();
    } catch (e: any) {
      error = e.message;
    }
  }
  async function add() {
    try {
      const level = draft.level;
      await post('/api/memory', {
        body: draft.body,
        level,
        kind: draft.kind,
        source: 'user',
        verification: draft.kind === 'verified' ? 'подтверждено пользователем' : '',
        project_id: level === 'project' ? draft.project_id || null : null,
        agent_id: level === 'personal' ? draft.agent_id || null : null,
      });
      draft.body = '';
      adding = false;
      load();
    } catch (e: any) {
      error = e.message;
    }
  }
</script>

<Page title="Память">
  {#snippet actions()}<button class="btn primary" onclick={() => (adding = !adding)}>Добавить запись</button>{/snippet}

  {#if adding}
    <form class="card pad form" style="margin-bottom:18px" onsubmit={(e) => { e.preventDefault(); add(); }}>
      <label>Что запомнить<textarea class="field" rows="2" bind:value={draft.body} required></textarea></label>
      <div class="row">
        <label style="flex:1">Уровень
          <select class="field" bind:value={draft.level}><option value="collective">Общая (всем)</option><option value="project">Проект</option><option value="personal">Личная</option></select></label>
        <label style="flex:1">Вид
          <select class="field" bind:value={draft.kind}>{#each Object.entries(kindLabel) as [k, v]}<option value={k}>{v}</option>{/each}</select></label>
        {#if draft.level === 'project'}<label style="flex:1">Проект<select class="field" bind:value={draft.project_id} required><option value="">—</option>{#each app.projects as p}<option value={p.id}>{p.name}</option>{/each}</select></label>{/if}
        {#if draft.level === 'personal'}<label style="flex:1">Специалист<select class="field" bind:value={draft.agent_id} required><option value="">—</option>{#each app.agents as a}<option value={a.id}>{a.name}</option>{/each}</select></label>{/if}
      </div>
      <div class="row"><button class="btn primary" type="submit">Сохранить</button><button class="btn" type="button" onclick={() => (adding = false)}>Отмена</button></div>
    </form>
  {/if}

  <div class="row" style="margin-bottom:14px">
    <div class="tabs" role="tablist">
      {#each [['all', 'Все'], ['personal', 'Личная'], ['project', 'Проектов'], ['collective', 'Общая']] as [k, l]}
        <button role="tab" aria-selected={level === k} class:on={level === k} onclick={() => (level = k as typeof level)}>{l}</button>
      {/each}
    </div>
    <input class="field" style="max-width:320px" bind:value={q} placeholder="Поиск по памяти" aria-label="Поиск по памяти" />
  </div>
  {#if error}<p style="color:var(--bad)">{error}</p>{/if}

  <ul class="list">
    {#each items as m (m.id)}
      <li class="item" style="align-items:flex-start">
        <span class="grow">
          <div style="white-space:pre-wrap">{m.body}</div>
          <div class="sub" style="margin-top:3px">
            {levelLabel[m.level]}{#if m.agent_id} · {agentById(m.agent_id)?.name}{/if}{#if m.project_id} · {projectById(m.project_id)?.name}{/if}
            {#if m.source} · источник: {m.source.length > 24 ? m.source.slice(0, 8) : m.source}{/if}{#if m.verification} · основание: {m.verification}{/if} · {timeAgo(m.updated_at)}
          </div>
        </span>
        <span class="chip {m.kind === 'verified' ? 'ok' : m.kind === 'error' ? 'bad' : m.kind === 'hypothesis' ? 'warn' : ''}">{kindLabel[m.kind]}</span>
        <span class="row" style="gap:2px">
          {#if m.kind !== 'verified'}<button class="btn sm" onclick={() => act(m, 'verify')}>Проверено</button>{/if}
          {#if m.kind === 'verified' && m.level !== 'collective'}<button class="btn sm" onclick={() => act(m, 'promote')}>В общую</button>{/if}
          <button class="btn sm" onclick={() => act(m, 'correct')}>Исправить</button>
          <button class="btn sm danger" onclick={() => act(m, 'archive')}>Убрать</button>
        </span>
      </li>
    {:else}
      <li class="empty-note">{q ? 'Ничего не нашлось.' : 'Пока ничего не сохранено. Специалисты записывают сюда опыт по ходу работы, а вы можете добавить своё.'}</li>
    {/each}
  </ul>
</Page>
