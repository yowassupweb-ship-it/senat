<script lang="ts">
  import Dropdown from '../components/Dropdown.svelte';
  import { get, post } from '../lib/api';
  import { agentById, app, projectById } from '../lib/store.svelte';
  import { dots, timeAgo } from '../lib/format';
  import type { Memory } from '../lib/types';
  import Page from '../components/Page.svelte';

  // Where a fact came from, in words: an automatic note from a run, a chat, or the person.
  function sourceLabel(src?: string | null): string {
    if (!src) return '';
    if (src.startsWith('auto')) return 'записано само';
    if (src.startsWith('chat') || src.startsWith('thread')) return 'из чата';
    if (src === 'user' || src === 'manual') return 'вручную';
    return src.length > 24 ? '' : 'источник: ' + src;
  }

  let level = $state<'all' | 'personal' | 'project' | 'collective'>('all');
  let q = $state('');
  let items = $state<Memory[]>([]);
  // long facts are folded to a few lines; a click unfolds one
  let open = $state<Record<string, boolean>>({});
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
          <Dropdown block value={draft.level} options={[{ id: 'collective', label: 'Общая', hint: 'видят все специалисты' }, { id: 'project', label: 'Проекта', hint: 'видят те, кто работает в проекте' }, { id: 'personal', label: 'Личная', hint: 'только у одного специалиста' }]} label="Чья" onpick={(v) => (draft.level = v)} /></label>
        <label style="flex:1">Вид
          <Dropdown block value={draft.kind} options={Object.entries(kindLabel).map(([k, v]) => ({ id: k, label: v as string }))} label="Вид" onpick={(v) => (draft.kind = v)} /></label>
        {#if draft.level === 'project'}<label style="flex:1">Проект<Dropdown block value={draft.project_id} options={[{ id: '', label: 'Выберите проект', disabled: true }, ...app.projects.map((p) => ({ id: p.id, label: p.name }))]} label="Проект" onpick={(v) => (draft.project_id = v)} /></label>{/if}
        {#if draft.level === 'personal'}<label style="flex:1">Специалист<Dropdown block value={draft.agent_id} options={[{ id: '', label: 'Выберите специалиста', disabled: true }, ...app.agents.map((a) => ({ id: a.id, label: a.name }))]} label="Специалист" onpick={(v) => (draft.agent_id = v)} /></label>{/if}
      </div>
      <div class="row"><button class="btn primary" type="submit" disabled={!draft.body.trim() || (draft.level === 'project' && !draft.project_id) || (draft.level === 'personal' && !draft.agent_id)}>Сохранить</button><button class="btn" type="button" onclick={() => (adding = false)}>Отмена</button></div>
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
      <li class="item mem">
        <div class="top">
          <span class="grow">
            {#if m.body.length > 320 && !open[m.id]}
              <button class="body clamp" onclick={() => (open[m.id] = true)} title="Показать полностью" aria-expanded="false">{m.body}</button>
            {:else}
              <div class="body">{m.body}</div>
              {#if m.body.length > 320}<button class="more" onclick={() => (open[m.id] = false)} aria-expanded="true">Свернуть</button>{/if}
            {/if}
          </span>
          <span class="chip {m.kind === 'verified' ? 'ok' : m.kind === 'error' ? 'bad' : m.kind === 'hypothesis' ? 'warn' : ''}">{kindLabel[m.kind]}</span>
        </div>
        <div class="bottom">
          <span class="sub grow">
            {dots(levelLabel[m.level], m.agent_id && agentById(m.agent_id)?.name, m.project_id && projectById(m.project_id)?.name, sourceLabel(m.source), m.verification && 'основание: ' + m.verification, timeAgo(m.updated_at))}
          </span>
        <span class="row acts" style="gap:2px">
          {#if m.kind !== 'verified'}<button class="btn sm" onclick={() => act(m, 'verify')}>Проверено</button>{/if}
          {#if m.kind === 'verified' && m.level !== 'collective'}<button class="btn sm" onclick={() => act(m, 'promote')}>В общую</button>{/if}
          <button class="btn sm" onclick={() => act(m, 'correct')}>Исправить</button>
          <button class="btn sm danger" onclick={() => act(m, 'archive')}>Убрать</button>
        </span>
        </div>
      </li>
    {:else}
      <li class="empty-note">{q ? 'Ничего не нашлось.' : 'Пока ничего не сохранено. Специалисты записывают сюда опыт по ходу работы, а вы можете добавить своё.'}</li>
    {/each}
  </ul>
</Page>

<style>
  /* the text takes the whole row; the verdict sits at the right edge; actions share the quiet meta line */
  .mem { flex-direction: column; align-items: stretch; gap: 6px; padding: 12px 16px; }
  .top, .bottom { display: flex; gap: 14px; min-width: 0; }
  .top { align-items: flex-start; }
  .top .chip { margin-top: 1px; flex: none; }
  .bottom { align-items: center; min-height: 24px; }
  .bottom .sub { min-width: 0; }
  .acts { flex: none; flex-wrap: nowrap; }
  @media (max-width: 640px) { .bottom { flex-wrap: wrap; } .acts { flex-wrap: wrap; } .bottom .sub { flex-basis: 100%; } }
  .body { white-space: pre-wrap; text-align: left; font: inherit; color: inherit; line-height: 1.5; }
  .clamp { display: -webkit-box; -webkit-line-clamp: 4; line-clamp: 4; -webkit-box-orient: vertical; overflow: hidden; width: 100%; cursor: pointer; border-radius: 4px; }
  .clamp:hover { color: var(--fg); }
  .clamp:focus-visible, .more:focus-visible { outline: none; box-shadow: var(--focus-ring); }
  .more { margin-top: 2px; font-size: 12px; color: var(--fg-3); }
  .more:hover { color: var(--fg); }
  /* actions show up where the pointer is, as in macOS lists; on touch they stay visible */
  @media (hover: hover) {
    .mem .acts { opacity: 0; transition: opacity 0.12s var(--ease); }
    .mem:hover .acts, .mem:focus-within .acts { opacity: 1; }
  }
</style>
