<script lang="ts">
  import { onMount } from 'svelte';
  import { get, post } from '../lib/api';
  import { onEvent } from '../lib/store.svelte';
  let { threadId }: { threadId: string } = $props();
  type Participant = { id: string; name: string; tokens: number | null; window: number | null; compacted: boolean };
  let context = $state<{ auto: boolean; pending: number; participants: Participant[] } | null>(null);
  let error = $state('');
  let busy = $state(false);
  let expanded = $state(false);
  let instructions = $state('');
  const percent = (p: Participant) => p.tokens != null && p.window && p.window > 0 ? Math.round(p.tokens / p.window * 100) : null;
  const peak = $derived(context ? Math.max(-1, ...context.participants.map(p => percent(p) ?? -1)) : -1);
  async function refresh() { try { const data = await get(`/api/threads/${threadId}/context`); if (Array.isArray(data?.participants)) context = data; error = ''; } catch (e: any) { error = e.message; } }
  async function change(body: object) {
    busy = true;
    try { context = await post(`/api/threads/${threadId}/context`, body); error = ''; } catch (e: any) { error = e.message; }
    finally { busy = false; }
  }
  onMount(() => {
    refresh();
    const off = onEvent((type, data) => { if ((type === 'thread' && data.id === threadId) || (type === 'message' && data.thread_id === threadId && ['done','failed','queued'].includes(data.status))) refresh(); });
    const timer = setInterval(refresh, 30000);
    return () => { off(); clearInterval(timer); };
  });
</script>
<section class="context" aria-label="Контекст чата">
  <button class="overview" aria-expanded={expanded} onclick={() => expanded = !expanded}>
    <span>Контекст {peak >= 0 ? `${peak}%` : '—'}</span>
    <span class="track" aria-hidden="true"><span style:width={`${Math.min(100, Math.max(0,peak))}%`}></span></span>
    <span class="faint">{context?.pending ? 'Сжатие в очереди или выполняется' : context?.auto ? 'Автосжатие · 80%' : 'Автосжатие выключено'}</span>
  </button>
  {#if expanded}
    <div class="details">
      <p class="faint">Шкала показывает наибольшее заполнение среди участников. История переписки хранится отдельно и при сжатии сохраняется. Автосжатие работает, когда модель сообщает размер контекста.</p>
      {#each context?.participants ?? [] as p (p.id)}
        <div class="person"><span>{p.name}</span><span>{p.compacted ? 'Сжат · ждёт следующего ответа' : percent(p) === null ? 'Модель ещё не сообщила объём' : `${percent(p)}% · ${p.tokens?.toLocaleString('ru-RU')} / ${p.window?.toLocaleString('ru-RU')} токенов`}</span></div>
      {:else}<p class="faint">Данные появятся после первого ответа.</p>{/each}
      <label class="auto"><input type="checkbox" checked={context?.auto ?? true} disabled={busy || !context} onchange={(e) => change({ auto: e.currentTarget.checked })} />Автоматически сжимать контекст при 80%</label>
      <label class="instructions">Что сохранить в сводке<input class="field" bind:value={instructions} placeholder="Например: решения по архитектуре и текущий план" /></label>
      <button class="btn sm" disabled={busy || !!context?.pending || !context?.participants.length} onclick={() => change({ compact: true, instructions })}>Сжать контекст участников</button>
      <p class="faint">Можно написать ассистенту: «Сожми контекст, сохрани решения и незавершённые задачи».</p>
    </div>
  {/if}
  {#if error}<p role="alert">{error}</p>{/if}
</section>
<style>
  .context { flex: none; max-height: 50vh; overflow-y: auto; border-bottom: 1px solid var(--line); padding: 8px 20px; }
  .overview { display: flex; flex-wrap: wrap; align-items: center; gap: 12px; width: 100%; text-align: left; font-size: 12px; }
  .track { height: 4px; min-width: 60px; max-width: 220px; flex: 1; background: var(--line); border-radius: 4px; overflow: hidden; }
  .track > span { display: block; height: 100%; background: var(--accent); }
  .details { max-width: 760px; padding-top: 12px; font-size: 13px; }
  .person { display: flex; flex-wrap: wrap; justify-content: space-between; gap: 8px; padding: 5px 0; }
  .auto { display: flex; align-items: center; gap: 8px; margin: 12px 0; }
  .instructions { display: grid; gap: 6px; margin: 12px 0; }
  p { margin: 8px 0; }
</style>
