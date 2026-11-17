<script lang="ts">
  // One place that answers "is anyone waiting for me?", and lets the person settle it without leaving the chat:
  // a stopped task is restarted, answered or dropped here; a permission and a proposed decision are decided here.
  // Only a question asked inside another chat needs that chat opened, because the question is there.
  import { get, post } from '../lib/api';
  import { agentById, app, go, projectById } from '../lib/store.svelte';
  import Icon from './Icon.svelte';

  let { threadId }: { threadId?: string } = $props();

  type Act = { label: string; primary?: boolean; danger?: boolean; run: () => Promise<unknown> | void };
  type Row = { key: string; what: string; where: string; acts: Act[]; answer?: (text: string) => Promise<unknown> };

  const move = (id: string, to: string, extra: Record<string, unknown>) => post(`/api/obligations/${id}/transition`, { to, ...extra });

  const rows = $derived.by(() => {
    const out: Row[] = [];
    for (const a of app.attention.approvals) {
      const who = agentById(a.agent_id)?.name ?? 'Специалист';
      out.push({
        key: 'a' + a.id,
        what: `${who} просит разрешения`,
        where: a.detail?.args?.title ?? a.detail?.args?.body ?? a.action,
        acts: [
          { label: 'Разрешить', primary: true, run: () => post(`/api/approvals/${a.id}/decide`, { approve: true }) },
          { label: 'Отклонить', run: () => post(`/api/approvals/${a.id}/decide`, { approve: false }) },
        ],
      });
    }
    for (const t of app.threads) {
      if (!t.awaiting || t.id === threadId || t.archived) continue;
      const who = agentById(t.agent_id)?.name ?? 'Специалист';
      out.push({ key: 't' + t.id, what: `${who} ждёт вашего ответа в чате`, where: t.title || 'Новый чат', acts: [{ label: 'Открыть чат', primary: true, run: () => go({ kind: 'chat', id: t.id }) }] });
    }
    for (const o of app.attention.needs_user) {
      const who = agentById(o.owner_id)?.name ?? 'Специалист';
      out.push({
        key: 'o' + o.id,
        what: `${who}: ${o.blocker || 'остановился и ждёт вашего решения'}`,
        where: o.title,
        answer: (text) => post(`/api/obligations/${o.id}/answer`, { answer: text }),
        acts: [
          { label: 'Запустить снова', primary: true, run: () => move(o.id, 'accepted', { note: 'повторный запуск' }) },
          { label: 'Снять задачу', danger: true, run: () => move(o.id, 'cancelled', { outcome: 'снята человеком' }) },
        ],
      });
    }
    for (const d of app.attention.proposed_decisions) {
      out.push({
        key: 'd' + d.id,
        what: `Предложено решение: ${d.title}`,
        where: projectById(d.project_id)?.name ?? 'общее',
        acts: [
          { label: 'Принять', primary: true, run: () => post(`/api/decisions/${d.id}/status`, { status: 'accepted' }) },
          { label: 'Отклонить', run: () => post(`/api/decisions/${d.id}/status`, { status: 'rejected' }) },
        ],
      });
    }
    return out;
  });

  let expanded = $state(false);
  let busy = $state<string | null>(null);
  let failed = $state<Record<string, string>>({});
  let replyTo = $state<string | null>(null);
  let reply = $state('');

  async function act(row: Row, fn: () => Promise<unknown> | void) {
    busy = row.key;
    delete failed[row.key];
    try {
      await fn();
      // the list is rebuilt from the node's own answer, not guessed
      app.attention = await get('/api/attention');
      if (replyTo === row.key) {
        replyTo = null;
        reply = '';
      }
    } catch (e: any) {
      failed[row.key] = e?.message || 'Не получилось. Повторите.';
    } finally {
      busy = null;
    }
  }
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
          <li aria-busy={busy === r.key}>
            <div class="line">
              <div class="text">
                <span class="what">{r.what}</span>
                <span class="where" title={r.where}>{r.where}</span>
              </div>
              <div class="acts">
                {#if r.answer}<button class="btn sm" disabled={busy === r.key} aria-expanded={replyTo === r.key} onclick={() => { replyTo = replyTo === r.key ? null : r.key; reply = ''; }}>Ответить</button>{/if}
                {#each r.acts as a (a.label)}
                  <button class="btn sm" class:primary={a.primary} class:danger={a.danger} disabled={busy === r.key} onclick={() => act(r, a.run)}>{a.label}</button>
                {/each}
              </div>
            </div>
            {#if replyTo === r.key && r.answer}
              <form class="reply" onsubmit={(e) => { e.preventDefault(); if (reply.trim()) act(r, () => r.answer!(reply.trim())); }}>
                <!-- svelte-ignore a11y_autofocus -->
                <input class="field" bind:value={reply} placeholder="Ответ, уточнение или решение: специалист продолжит с ним" aria-label="Ответ специалисту" autofocus onkeydown={(e) => e.key === 'Escape' && (replyTo = null)} />
                <button type="submit" class="btn sm primary" disabled={!reply.trim() || busy === r.key}>{busy === r.key ? 'Отправляю…' : 'Продолжить'}</button>
              </form>
            {/if}
            {#if failed[r.key]}<p class="err" role="alert">{failed[r.key]}</p>{/if}
          </li>
        {/each}
      </ul>
    {/if}
  </section>
{/if}

<style>
  .needs { margin: 0 auto 0.375rem; width: 100%; max-width: 46rem; box-sizing: border-box; padding: 0 8px 2px; border: 1px solid var(--line); border-left: 3px solid var(--warn); border-radius: 10px; background: var(--bg-raised); font-size: 12px; }
  .head { display: flex; align-items: center; gap: 6px; width: 100%; min-height: 26px; min-width: 0; color: var(--fg-2); font-size: 12px; border-radius: var(--radius-s); }
  .head:hover { color: var(--fg); }
  .head:focus-visible { outline: 2px solid var(--accent); outline-offset: 1px; }
  .title { font-weight: 600; white-space: nowrap; }
  .chev { display: inline-flex; color: var(--fg-3); transition: transform 0.15s var(--ease); }
  .chev.open { transform: rotate(90deg); }
  .count { min-width: 18px; padding: 0 6px; border-radius: 9px; background: var(--warn-soft); color: var(--warn); font-size: 11px; font-weight: 600; line-height: 18px; text-align: center; font-variant-numeric: tabular-nums; }
  .first { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; text-align: left; color: var(--fg-3); }
  ul { list-style: none; margin: 0; padding: 0; max-height: 15rem; overflow-y: auto; }
  li { padding: 6px 4px; border-top: 1px solid var(--line); }
  li[aria-busy='true'] { opacity: 0.6; }
  .line { display: flex; align-items: center; gap: 8px; }
  .text { flex: 1; min-width: 0; display: grid; gap: 1px; }
  .what { color: var(--fg); display: -webkit-box; -webkit-box-orient: vertical; -webkit-line-clamp: 2; line-clamp: 2; overflow: hidden; overflow-wrap: anywhere; }
  .where { color: var(--fg-3); font-size: 11px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .acts { display: flex; gap: 4px; flex: none; }
  .btn.danger { color: var(--bad); }
  .reply { display: flex; gap: 6px; margin-top: 6px; }
  .reply .field { flex: 1; min-width: 0; }
  .err { margin: 4px 0 0; color: var(--bad); }
  @media (prefers-reduced-motion: reduce) { .chev { transition: none; } }
  @media (max-width: 600px) {
    .needs { margin-inline: 0.5rem; width: auto; max-width: none; }
    .line { flex-direction: column; align-items: stretch; }
    .acts { flex-wrap: wrap; }
  }
</style>
