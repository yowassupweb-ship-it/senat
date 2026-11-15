<script lang="ts">
  import type { Agent, Obligation } from '../lib/types';
  import { agentById, go } from '../lib/store.svelte';
  import { STATE_LABEL, timeAgo } from '../lib/format';
  import Avatar from './Avatar.svelte';

  let { obligations, members }: { obligations: Obligation[]; members: Agent[] } = $props();
  const open = $derived(obligations.filter((o) => !['done', 'cancelled'].includes(o.state)));
  const ordered = $derived([...open].sort((a, b) => {
    const rank = (o: Obligation) => o.state === 'blocked' ? 0 : o.state === 'review' ? 1 : o.state === 'waiting' ? 2 : 3;
    return rank(a) - rank(b) || a.created_at - b.created_at;
  }));
  const needsPerson = $derived(open.filter((o) => o.state === 'blocked' || (o.state === 'review' && (!o.reviewer_id || !!o.blocker)) || o.wait_condition?.type === 'user').length);
  const owner = (id?: string | null) => agentById(id)?.name ?? 'Без исполнителя';
  const dependencies = (o: Obligation) => (o.dependencies ?? []).map((id) => obligations.find((dep) => dep.id === id)).filter((dep): dep is Obligation => !!dep && dep.state !== 'done');
  const downstream = (id: string) => open.filter((o) => (o.dependencies ?? []).includes(id)).length;
</script>

<section class="section" aria-label="Работа команды">
  <h2>Работа команды <span class="count">{open.length}</span></h2>
  <div class="row">
    <span class="chip">В работе: {open.filter((o) => o.state === 'running').length}</span>
    <span class="chip">Ждут: {open.filter((o) => o.state === 'waiting').length}</span>
    <span class="chip">На проверке: {open.filter((o) => o.state === 'review').length}</span>
    <span class="chip" class:warn={needsPerson > 0}>Нужно ваше решение: {needsPerson}</span>
  </div>
  <ul class="list">
    {#each ordered as o (o.id)}
      <li>
        <button class="item" onclick={() => go({ kind: 'obligations', id: o.id })}>
          <Avatar name={owner(o.owner_id)} size={26} />
          <span class="grow">
            <span class="title">{o.title}</span>
            <span class="sub">{owner(o.owner_id)} · {o.result || 'Результат ещё не описан'}</span>
            {#if o.blocker}<span class="sub bad">{o.blocker}</span>{/if}
            {#if dependencies(o).length}<span class="sub">Ждёт: {dependencies(o).map((dep) => `${dep.title} — ${owner(dep.owner_id)}`).join('; ')}</span>{/if}
            {#if o.state === 'review'}<span class="sub">Проверяет: {o.reviewer_id ? owner(o.reviewer_id) : 'вы'} · {o.evidence || 'Основания проверки не приложены'}</span>{/if}
            {#if downstream(o.id)}<span class="sub">От результата зависят задач: {downstream(o.id)}</span>{/if}
            <span class="meta"><span class="chip">{STATE_LABEL[o.state]}</span><span class="faint">{timeAgo(o.updated_at)}</span></span>
          </span>
        </button>
      </li>
    {:else}<li class="empty-note">Открытых задач нет. Новые поручения появятся здесь вместе с исполнителями и зависимостями.</li>{/each}
  </ul>
  <details>
    <summary>Загрузка специалистов</summary>
    <ul class="list">
      {#each members as member (member.id)}
        <li class="item"><span class="grow">{member.name}</span><span class="sub">Задач: {open.filter((o) => o.owner_id === member.id).length} · На проверке: {open.filter((o) => o.reviewer_id === member.id && o.state === 'review').length}</span></li>
      {/each}
    </ul>
  </details>
</section>

<style>
  .item { align-items: flex-start; }
  .item .title, .item .sub { display: block; white-space: normal; overflow-wrap: anywhere; }
  .meta { display: flex; flex-wrap: wrap; align-items: center; gap: 0.5em; margin-top: 0.5em; }
</style>
