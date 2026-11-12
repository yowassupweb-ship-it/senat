<script lang="ts">
  import { post } from '../lib/api';
  import { app, go } from '../lib/store.svelte';
  import AgentChips from './AgentChips.svelte';
  import Icon from './Icon.svelte';

  let { onclose, topic = '', project = '', fromThread = '' }: { onclose: () => void; topic?: string; project?: string; fromThread?: string } = $props();

  let q = $state(topic);
  let outcome = $state<'plan' | 'decision' | 'analysis'>('plan');
  let people = $state<string[]>(app.agents.slice(0, 3).map((a) => a.id));
  let rounds = $state(2);
  let projectId = $state(project);
  let error = $state('');
  let busy = $state(false);
  const outcomes: [string, string][] = [['plan', 'План работы'], ['decision', 'Решение'], ['analysis', 'Разбор вопроса']];

  async function open() {
    error = '';
    busy = true;
    try {
      const proj = app.projects.find((p) => p.id === projectId);
      const t = await post('/api/forums', { topic: q, outcome, participants: people, rounds, project_id: projectId || null, company_id: proj?.company_id ?? null, from_thread: fromThread || null });
      app.threads.unshift(t);
      app.messages[t.id] = [];
      onclose();
      go({ kind: 'chat', id: t.id });
    } catch (e: any) {
      error = e.message;
    } finally {
      busy = false;
    }
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="scrim" onclick={onclose}>
  <div class="dlg" role="dialog" tabindex="-1" aria-modal="true" aria-label="Новый форум" onclick={(e) => e.stopPropagation()}>
    <header><b>Новый форум</b><button class="icon-btn" onclick={onclose} aria-label="Закрыть"><Icon name="x" size={15} /></button></header>
    <form onsubmit={(e) => { e.preventDefault(); open(); }}>
      <label>Вопрос<textarea class="field" rows="3" bind:value={q} placeholder="Что обсуждаем?" required></textarea></label>
      <div class="two">
        <div><span class="lbl">Итог</span><div class="tabs">{#each outcomes as [k, l]}<button type="button" class:on={outcome === k} onclick={() => (outcome = k as typeof outcome)}>{l}</button>{/each}</div></div>
        <div><span class="lbl">Раундов</span><div class="tabs">{#each [1, 2, 3, 4, 5] as n}<button type="button" class:on={rounds === n} onclick={() => (rounds = n)}>{n}</button>{/each}</div></div>
      </div>
      <div><span class="lbl">Участники · {people.length}</span><AgentChips selected={people} onchange={(ids) => (people = ids)} /></div>
      <label>Проект
        <select class="field" bind:value={projectId}><option value="">Без проекта</option>{#each app.projects as p}<option value={p.id}>{p.name}</option>{/each}</select></label>
      {#if error}<p class="bad">{error}</p>{/if}
      <div class="row"><button class="btn primary" type="submit" disabled={busy || people.length < 2 || !q.trim()}>Открыть форум</button><button class="btn" type="button" onclick={onclose}>Отмена</button></div>
    </form>
  </div>
</div>

<style>
  .scrim {
    position: fixed;
    inset: 0;
    z-index: 110;
    background: rgba(0, 0, 0, 0.3);
    display: grid;
    place-items: center;
  }
  .dlg {
    width: 560px;
    max-width: calc(100vw - 32px);
    max-height: 88vh;
    overflow-y: auto;
    background: var(--bg-raised);
    border-radius: var(--radius-l);
    box-shadow: var(--shadow-2);
    animation: fadeIn 0.14s var(--ease);
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 14px 14px 4px 20px;
    font-size: 15px;
  }
  form {
    padding: 8px 20px 20px;
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
  label {
    display: flex;
    flex-direction: column;
    gap: 4px;
    font-weight: 500;
    font-size: 13px;
  }
  .lbl {
    display: block;
    font-size: 12px;
    font-weight: 600;
    color: var(--fg-3);
    text-transform: uppercase;
    letter-spacing: 0.03em;
    margin-bottom: 5px;
  }
  .two {
    display: flex;
    gap: 20px;
    flex-wrap: wrap;
  }
  .bad {
    color: var(--bad);
    margin: 0;
  }
  .row {
    display: flex;
    gap: 8px;
  }
</style>
