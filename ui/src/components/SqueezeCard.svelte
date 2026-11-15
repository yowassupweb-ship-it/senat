<script lang="ts">
  // Context compaction in the chat: one card for everyone who was compacted together. Each row is a specialist,
  // the summary they wrote hides behind a button.
  import type { Message } from '../lib/types';
  import { agentById } from '../lib/store.svelte';
  import { md } from '../lib/format';
  import Avatar from './Avatar.svelte';
  import Icon from './Icon.svelte';
  let { items, onstop }: { items: Message[]; onstop: (m: Message) => void } = $props();
  const live = (m: Message) => m.status === 'working' || m.status === 'queued';
  const working = $derived(items.filter(live).length);
  const failed = $derived(items.filter((m) => m.status === 'failed').length);
  const done = $derived(items.filter((m) => m.status === 'done').length);
  const title = $derived(working ? 'Сжимаю контекст' : failed && !done ? 'Сжать не удалось' : 'Контекст сжат');
  let open = $state<string | null>(null);
  let now = $state(Date.now());
  $effect(() => {
    if (!working) return;
    const t = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(t);
  });
  const sec = (m: Message) => Math.max(0, Math.round((now - m.created_at) / 1000));
</script>

<div class="squeeze" class:bad={!working && failed > 0 && !done} role="status">
  <div class="top">
    <span class="lead" aria-hidden="true">{#if working}<span class="loader"></span>{:else}<Icon name={failed && !done ? 'alert' : 'check'} size={14} />{/if}</span>
    <b>{title}</b>
    <span class="sum">{#if working}{done + failed} из {items.length}{:else if failed || done < items.length}{done} из {items.length}{#if failed}, {failed} не вышло{/if}{:else if items.length > 1}{items.length} участников{/if}</span>
  </div>
  <ul>
    {#each items as m (m.id)}
      {@const a = agentById(m.author_id)}
      <li class:err={m.status === 'failed'}>
        <Avatar name={a?.name} size={20} />
        <span class="who">{a?.name ?? 'Специалист'}</span>
        <span class="st">
          {#if m.status === 'queued'}в очереди
          {:else if m.status === 'working'}<span class="loader sm"></span>{sec(m)} с
          {:else if m.status === 'failed'}не вышло, контекст сохранён
          {:else if m.status === 'cancelled'}остановлено
          {:else}<Icon name="check" size={12} />продолжит со сводкой{/if}
        </span>
        {#if m.status === 'done' && m.body}<button class="more" aria-expanded={open === m.id} onclick={() => (open = open === m.id ? null : m.id)}>{open === m.id ? 'Скрыть' : 'Сводка'}</button>{/if}
        {#if live(m)}<button class="icon-btn" aria-label="Остановить сжатие" title="Остановить" onclick={() => onstop(m)}><Icon name="x" size={12} /></button>{/if}
      </li>
      {#if open === m.id && m.body}<li class="body">{@html md(m.body)}</li>{/if}
    {/each}
  </ul>
</div>

<style>
  .squeeze { width: fit-content; min-width: 18rem; max-width: min(100%, 34rem); margin: 8px auto; padding: 8px 12px 6px; border: 1px solid var(--line); border-radius: 12px; background: var(--bg-raised); font-size: 12px; }
  .squeeze.bad { background: var(--bad-soft); }
  .top { display: flex; align-items: center; gap: 8px; padding-bottom: 6px; font-size: 13px; }
  .top b { font-weight: 600; }
  .sum { margin-left: auto; color: var(--fg-3); font-size: 11px; font-variant-numeric: tabular-nums; }
  .lead { display: grid; place-items: center; width: 16px; height: 16px; color: var(--fg-2); }
  .bad .lead { color: var(--bad); }
  ul { list-style: none; margin: 0; padding: 0; }
  li { display: flex; align-items: center; gap: 8px; padding: 5px 0; border-top: 1px solid var(--line); }
  li.body { display: block; padding: 8px 0 8px 28px; color: var(--fg-2); max-height: 16rem; overflow-y: auto; }
  .who { font-weight: 500; }
  .st { margin-left: auto; display: inline-flex; align-items: center; gap: 5px; color: var(--fg-3); font-variant-numeric: tabular-nums; }
  li.err .st { color: var(--bad); }
  .more { padding: 1px 8px; border-radius: 6px; color: var(--fg-2); font-size: 11px; }
  .more:hover { background: var(--bg-hover); color: var(--fg); }
  .loader { width: 14px; height: 14px; border-radius: 50%; border: 2px solid var(--line-strong); border-top-color: var(--fg); animation: sq-spin .8s linear infinite; }
  .loader.sm { width: 10px; height: 10px; border-width: 1.5px; }
  @keyframes sq-spin { to { transform: rotate(360deg); } }
  @media (prefers-reduced-motion: reduce) { .loader { animation-duration: 2.4s; } }
</style>
