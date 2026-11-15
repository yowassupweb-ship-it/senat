<script lang="ts">
  import { outbox, retryOutgoing, removeOutgoing } from '../lib/outbox.svelte';
  import { agentById, app } from '../lib/store.svelte';
  import type { Message } from '../lib/types';
  import Avatar from './Avatar.svelte';
  import Icon from './Icon.svelte';
  let { threadId, projectId, queued = [], oncancel }: { threadId?: string; projectId?: string; queued?: Message[]; oncancel: (m: Message) => void } = $props();
  const pending = $derived(outbox.items.filter(i => threadId ? i.threadId === threadId : !i.threadId && i.projectId === projectId));
  let expanded = $state(true);
  const failedCount = $derived(pending.filter(i => i.state === 'failed').length);
</script>

{#if pending.length || queued.length}
  <section class="outbox" aria-label="Очередь сообщений">
    <button class="queue-title" onclick={() => expanded = !expanded} aria-expanded={expanded}>
      <span class="chev" class:open={expanded}><Icon name="right" size={11} /></span>
      <span>Очередь</span>
      <span class="count">{pending.length + queued.length}</span>
      {#if !expanded && failedCount}<span class="warnline">{failedCount} не отправлено</span>{/if}
    </button>
    {#if expanded}
      <ol>
        {#each pending as item (item.id)}
          <li class:failed={item.state === 'failed'}>
            <span class="lead" class:spin={item.state === 'sending'} aria-hidden="true"><Icon name={item.state === 'failed' ? 'alert' : item.state === 'sending' ? 'refresh' : 'clock'} size={13} /></span>
            <div class="pending-text">
              <span class="preview">{item.text || 'Вложения'}</span>
              <small role={item.state === 'failed' ? 'alert' : 'status'}>{item.state === 'sending' ? 'Отправляется' : item.state === 'failed' ? item.error : 'Ожидает отправки'}</small>
            </div>
            {#if item.state === 'failed'}<button class="btn sm" onclick={() => retryOutgoing(item.id)}>Повторить</button>{/if}
            {#if item.state !== 'sending'}<button class="icon-btn rm" aria-label="Убрать сообщение из очереди" title="Убрать из очереди" onclick={() => removeOutgoing(item.id)}><Icon name="x" size={12}/></button>{/if}
          </li>
        {/each}
        {#each queued as m, i (m.id)}
          {@const agent = agentById(m.author_id)}
          {@const request = app.messages[m.thread_id]?.find(x => x.id === m.props.request_message_id)}
          <li>
            <span class="lead"><Avatar name={agent?.name} size={20}/></span>
            <div class="pending-text">
              <span class="preview"><b>{agent?.name ?? 'Специалист'}</b>{request?.body ? ` · ${request.body}` : ''}</span>
              <small>Ждёт запуска{queued.length > 1 ? ` · ${i + 1} из ${queued.length}` : ''}</small>
            </div>
            <button class="icon-btn rm" aria-label={`Отменить ожидающий ответ: ${agent?.name ?? 'специалист'}`} title="Отменить ожидающий ответ" onclick={() => oncancel(m)}><Icon name="x" size={12}/></button>
          </li>
        {/each}
      </ol>
    {/if}
  </section>
{/if}

<style>
  .outbox { margin: 0 auto 0.5rem; max-width: 53rem; padding: 2px 8px 4px; border: 1px solid var(--line); border-radius: 10px; background: var(--bg-raised); font-size: 12px; }
  .queue-title { display: flex; align-items: center; gap: 6px; width: 100%; min-height: 26px; color: var(--fg-2); font-size: 12px; font-weight: 600; border-radius: var(--radius-s); }
  .queue-title:hover { color: var(--fg); }
  .chev { display: inline-flex; color: var(--fg-3); transition: transform .15s var(--ease); }
  .chev.open { transform: rotate(90deg); }
  .count { min-width: 18px; padding: 0 6px; border-radius: 9px; background: var(--bg-active); color: var(--fg-2); font-size: 11px; font-weight: 600; line-height: 18px; text-align: center; font-variant-numeric: tabular-nums; }
  .warnline { margin-left: auto; color: var(--bad); font-weight: 500; }
  ol { list-style: none; margin: 0; padding: 0; max-height: 9.5rem; overflow-y: auto; }
  li { display: flex; align-items: center; gap: 8px; padding: 5px 4px; border-top: 1px solid var(--line); border-radius: 0; }
  li:first-child { border-top: 0; }
  li.failed { background: var(--bad-soft); border-radius: var(--radius-s); border-top-color: transparent; }
  .lead { flex: none; display: grid; place-items: center; width: 20px; height: 20px; color: var(--fg-3); }
  li.failed .lead { color: var(--bad); }
  .spin :global(svg) { animation: spin 1s linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }
  @media (prefers-reduced-motion: reduce) { .spin :global(svg) { animation: none; } }
  .pending-text { min-width: 0; flex: 1; }
  .preview { display: block; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; color: var(--fg); }
  .preview b { font-weight: 600; }
  small { display: block; color: var(--fg-3); font-size: 11px; }
  li.failed small { color: var(--bad); }
  .rm { opacity: 0; transition: opacity .12s var(--ease); }
  li:hover .rm, .rm:focus-visible { opacity: 1; }
  @media (hover: none) { .rm { opacity: 1; } }
  @media (max-width: 600px) { .outbox { margin-inline: 0.5rem; } }
</style>
