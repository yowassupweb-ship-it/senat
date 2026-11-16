<script lang="ts">
  // One place that answers "is anyone waiting for me?": questions and permissions in other chats, tasks that stopped
  // and wait for a decision, decisions offered for approval. Shown above the composer only when there is something.
  import { agentById, app, go } from '../lib/store.svelte';
  import Icon from './Icon.svelte';

  let { threadId }: { threadId?: string } = $props();

  type Row = { key: string; what: string; where: string; act: string; open: () => void };
  const rows = $derived.by(() => {
    const out: Row[] = [];
    for (const t of app.threads) {
      if (!t.awaiting || t.id === threadId || t.archived) continue;
      const who = agentById(t.agent_id)?.name ?? 'Специалист';
      out.push({ key: 't' + t.id, what: `${who} ждёт вашего ответа`, where: t.title || 'Новый чат', act: 'Открыть чат', open: () => go({ kind: 'chat', id: t.id }) });
    }
    for (const o of app.attention.needs_user) {
      const who = agentById(o.owner_id)?.name ?? 'Специалист';
      out.push({ key: 'o' + o.id, what: `${who} остановился: ${o.blocker || 'нужно ваше решение'}`, where: o.title, act: 'К задаче', open: () => go({ kind: 'obligations' }) });
    }
    const ap = app.attention.approvals.length;
    if (ap) out.push({ key: 'ap', what: `Ждут разрешения: ${ap}`, where: 'действия специалистов', act: 'Посмотреть', open: () => go({ kind: 'obligations' }) });
    const pd = app.attention.proposed_decisions.length;
    if (pd) out.push({ key: 'pd', what: `Предложены решения: ${pd}`, where: 'ждут вашего согласия', act: 'Посмотреть', open: () => go({ kind: 'projects' }) });
    return out;
  });
  let expanded = $state(false);
</script>

{#if rows.length}
  <section class="needs" aria-label="Нужно от вас">
    <button class="head" onclick={() => (expanded = !expanded)} aria-expanded={expanded}>
      <span class="chev" class:open={expanded} aria-hidden="true"><Icon name="right" size={11} /></span>
      <span class="title">Нужно от вас</span>
      <span class="count">{rows.length}</span>
      {#if !expanded}<span class="first">{rows[0].what}</span>{/if}
    </button>
    {#if expanded}
      <ul>
        {#each rows as r (r.key)}
          <li>
            <div class="text">
              <span class="what" title={r.what}>{r.what}</span>
              <span class="where" title={r.where}>{r.where}</span>
            </div>
            <button class="btn sm" onclick={r.open}>{r.act}</button>
          </li>
        {/each}
      </ul>
    {/if}
  </section>
{/if}

<style>
  .needs { margin: 0 auto 0.375rem; width: 100%; max-width: 38rem; box-sizing: border-box; padding: 0 8px 2px; border: 1px solid var(--line); border-left: 3px solid var(--warn); border-radius: 10px; background: var(--bg-raised); font-size: 12px; }
  .head { display: flex; align-items: center; gap: 6px; width: 100%; min-height: 26px; min-width: 0; color: var(--fg-2); font-size: 12px; border-radius: var(--radius-s); }
  .head:hover { color: var(--fg); }
  .head:focus-visible { outline: 2px solid var(--accent); outline-offset: 1px; }
  .title { font-weight: 600; white-space: nowrap; }
  .chev { display: inline-flex; color: var(--fg-3); transition: transform 0.15s var(--ease); }
  .chev.open { transform: rotate(90deg); }
  .count { min-width: 18px; padding: 0 6px; border-radius: 9px; background: var(--warn-soft); color: var(--warn); font-size: 11px; font-weight: 600; line-height: 18px; text-align: center; font-variant-numeric: tabular-nums; }
  .first { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; text-align: left; color: var(--fg-3); }
  ul { list-style: none; margin: 0; padding: 0; max-height: 11rem; overflow-y: auto; }
  li { display: flex; align-items: center; gap: 8px; padding: 5px 4px; border-top: 1px solid var(--line); }
  .text { flex: 1; min-width: 0; display: grid; }
  .what { color: var(--fg); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .where { color: var(--fg-3); font-size: 11px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  @media (prefers-reduced-motion: reduce) { .chev { transition: none; } }
  @media (max-width: 600px) { .needs { margin-inline: 0.5rem; width: auto; max-width: none; } }
</style>
