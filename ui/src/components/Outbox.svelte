<script lang="ts">
  import { outbox, retryOutgoing, removeOutgoing } from '../lib/outbox.svelte';
  import { agentById, app } from '../lib/store.svelte';
  import type { Message } from '../lib/types';
  import Avatar from './Avatar.svelte';
  import Icon from './Icon.svelte';
  let { threadId, projectId, queued = [], oncancel }: { threadId?: string; projectId?: string; queued?: Message[]; oncancel: (m: Message) => void } = $props();
  const pending = $derived(outbox.items.filter(i => threadId ? i.threadId === threadId : !i.threadId && i.projectId === projectId));
  let expanded = $state(true);
</script>

{#if pending.length || queued.length}
  <section class="outbox" aria-label="Очередь сообщений">
    <button class="queue-title" onclick={() => expanded = !expanded} aria-expanded={expanded}>
      <Icon name={expanded ? 'down' : 'right'} size={13} />Очередь сообщений <span class="faint">{pending.length + queued.length}</span>
    </button>
    {#if expanded}
      <ol>
        {#each pending as item (item.id)}
          <li>
            <div class="pending-text"><span class="preview">{item.text || 'Вложения'}</span><small class:failed={item.state === 'failed'} role={item.state === 'failed' ? 'alert' : 'status'}>{item.state === 'sending' ? 'Отправляется…' : item.state === 'failed' ? item.error : 'Ожидает отправки'}</small></div>
            {#if item.state === 'failed'}<button class="btn sm" onclick={() => retryOutgoing(item.id)}>Повторить</button>{/if}
            {#if item.state !== 'sending'}<button class="icon-btn" aria-label="Убрать сообщение из очереди" title="Убрать из очереди" onclick={() => removeOutgoing(item.id)}><Icon name="x" size={13}/></button>{/if}
          </li>
        {/each}
        {#each queued as m, i (m.id)}
          {@const agent = agentById(m.author_id)}
          {@const request = app.messages[m.thread_id]?.find(x => x.id === m.props.request_message_id)}
          <li>
            <Avatar name={agent?.name} size={22}/>
            <div class="pending-text"><span class="preview">{agent?.name ?? 'Специалист'}{request?.body ? ` · ${request.body}` : ''}</span><small>Ожидает запуска · {i + 1} в списке ожидания</small></div>
            <button class="icon-btn" aria-label={`Отменить ожидающий ответ: ${agent?.name ?? 'специалист'}`} title="Отменить ожидающий ответ" onclick={() => oncancel(m)}><Icon name="x" size={13}/></button>
          </li>
        {/each}
      </ol>
    {/if}
  </section>
{/if}

<style>
  .outbox { margin: 0 auto 0.5rem; max-width: 53rem; padding: 0.5rem 0.75rem; border: 1px solid var(--line); border-radius: var(--radius); background: var(--bg-raised); box-shadow: var(--shadow-1); }
  .queue-title { display: flex; align-items: center; gap: 0.5rem; font-size: inherit; font-weight: 600; min-height: 1.5rem; }
  ol { list-style: none; margin: 0.25rem 0 0; padding: 0; max-height: 10rem; overflow-y: auto; }
  li { display: flex; align-items: center; gap: 0.5rem; padding: 0.375rem 0; }
  .pending-text { min-width: 0; flex: 1; }
  .preview { display: block; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  small { display: block; color: var(--fg-2); }
  .failed { color: var(--bad); }
  @media (max-width: 600px) { .outbox { margin-inline: 0.5rem; } }
</style>
