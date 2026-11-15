<script lang="ts">
  import { onMount } from 'svelte';
  import { get, post } from '../lib/api';
  import Icon from './Icon.svelte';
  import { onEvent } from '../lib/store.svelte';
  let { threadId }: { threadId: string } = $props();
  type Participant = { id: string; name: string; tokens: number | null; window: number | null; compacted: boolean };
  let context = $state<{ auto: boolean; pending: number; participants: Participant[] } | null>(null);
  let error = $state('');
  let busy = $state(false);
  let expanded = $state(false);
  let instructions = $state('');
  const percent = (p: Participant) => p.tokens != null && p.window && p.window > 0 ? Math.round(p.tokens / p.window * 100) : null;
  const level = (v: number | null) => v === null ? '' : v >= 95 ? 'bad' : v >= 80 ? 'warn' : '';
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
    <span class="label">Контекст</span>
    <span class="value {level(peak)}">{peak >= 0 ? `${peak}%` : '—'}</span>
    <span class="track {level(peak)}" aria-hidden="true"><span class="fill" style:width={`${Math.min(100, Math.max(0,peak))}%`}></span><i class="mark"></i></span>
    <span class="status" class:busy={!!context?.pending}>{#if context?.pending}<Icon name="refresh" size={12} />Сжатие идёт{:else if context?.auto}Автосжатие · 80%{:else}Автосжатие выключено{/if}</span>
    <span class="chev" class:open={expanded}><Icon name="down" size={12} /></span>
  </button>
  {#if expanded}
    <div class="details">
      <div class="people">
        {#each context?.participants ?? [] as p (p.id)}
          {@const v = percent(p)}
          <div class="person">
            <span class="name">{p.name}</span>
            <span class="track small {level(v)}" aria-hidden="true"><span class="fill" style:width={`${Math.min(100, v ?? 0)}%`}></span><i class="mark"></i></span>
            <span class="meta">{p.compacted ? 'Сжат, ждёт следующего ответа' : v === null ? 'Объём ещё не сообщён' : `${v}% · ${p.tokens?.toLocaleString('ru-RU')} / ${p.window?.toLocaleString('ru-RU')}`}</span>
          </div>
        {:else}<p class="faint">Данные появятся после первого ответа.</p>{/each}
      </div>
      <label class="auto">
        <input class="switch" type="checkbox" checked={context?.auto ?? true} disabled={busy || !context} onchange={(e) => change({ auto: e.currentTarget.checked })} />
        <span>Сжимать автоматически при 80%</span>
      </label>
      <div class="manual">
        <input class="field" aria-label="Что сохранить в сводке" bind:value={instructions} placeholder="Что сохранить в сводке: решения, текущий план" />
        <button class="btn sm" disabled={busy || !!context?.pending || !context?.participants.length} onclick={() => change({ compact: true, instructions })}>Сжать сейчас</button>
      </div>
      <p class="faint hint">Шкала показывает наибольшее заполнение среди участников. История переписки при сжатии сохраняется.</p>
    </div>
  {/if}
  {#if error}<p class="err" role="alert">{error}</p>{/if}
</section>
<style>
  .context { flex: none; max-height: 50vh; overflow-y: auto; border-bottom: 1px solid var(--line); padding: 0 20px; }
  .overview { display: flex; align-items: center; gap: 10px; width: 100%; height: 32px; text-align: left; font-size: 12px; border-radius: var(--radius-s); }
  .overview:focus-visible { outline-offset: -2px; }
  .label { color: var(--fg-3); }
  .value { min-width: 30px; font-variant-numeric: tabular-nums; font-weight: 600; color: var(--fg); }
  .value.warn { color: var(--warn); }
  .value.bad { color: var(--bad); }
  .track { position: relative; height: 4px; flex: 1; min-width: 60px; max-width: 280px; background: var(--bg-active); border-radius: 4px; overflow: hidden; }
  .track.small { flex: none; width: 120px; height: 3px; }
  .fill { display: block; height: 100%; background: var(--fg-2); border-radius: 4px; transition: width .3s var(--ease); }
  .track.warn .fill { background: var(--warn); }
  .track.bad .fill { background: var(--bad); }
  .mark { position: absolute; top: 0; bottom: 0; left: 80%; width: 1px; background: var(--line-strong); }
  .status { margin-left: auto; display: inline-flex; align-items: center; gap: 5px; color: var(--fg-3); white-space: nowrap; }
  .status.busy { color: var(--fg-2); }
  .chev { display: inline-flex; color: var(--fg-3); transition: transform .15s var(--ease); }
  .chev.open { transform: rotate(180deg); }
  .overview:hover .chev, .overview:hover .label { color: var(--fg-2); }
  .details { max-width: 760px; padding: 4px 0 12px; font-size: 13px; display: grid; gap: 10px; }
  .people { display: grid; gap: 2px; }
  .person { display: grid; grid-template-columns: minmax(80px, 160px) auto 1fr; align-items: center; gap: 12px; padding: 4px 0; }
  .name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .meta { color: var(--fg-3); font-size: 12px; font-variant-numeric: tabular-nums; }
  .auto { display: flex; align-items: center; gap: 10px; cursor: pointer; }
  .switch { appearance: none; flex: none; width: 28px; height: 16px; margin: 0; border-radius: 8px; background: var(--bg-active); position: relative; cursor: pointer; transition: background .15s var(--ease); }
  .switch::after { content: ''; position: absolute; top: 2px; left: 2px; width: 12px; height: 12px; border-radius: 50%; background: var(--fg); transition: transform .15s var(--ease); }
  .switch:checked { background: var(--fg-3); }
  .switch:checked::after { transform: translateX(12px); }
  .switch:disabled { opacity: .5; cursor: default; }
  .manual { display: flex; gap: 8px; align-items: center; }
  .manual .field { flex: 1; min-width: 0; }
  .hint { margin: 0; font-size: 12px; }
  .err { margin: 0 0 8px; color: var(--bad); font-size: 12px; }
  p { margin: 0; }
</style>
