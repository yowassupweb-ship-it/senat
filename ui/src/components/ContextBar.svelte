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
  let root = $state<HTMLElement | undefined>();
  const SIZE = 30, C = 15, R = 12.5;
  const arc = (v: number) => { const len = 2 * Math.PI * R; return `${Math.min(100, Math.max(0, v)) / 100 * len} ${len}`; };
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
    const away = (e: MouseEvent) => { if (expanded && root && !root.contains(e.target as Node)) expanded = false; };
    window.addEventListener('mousedown', away);
    return () => { off(); clearInterval(timer); window.removeEventListener('mousedown', away); };
  });
</script>
<div class="ctx" bind:this={root} onkeydown={(e) => { if (e.key === 'Escape') expanded = false; }} role="presentation">
  <button class="disc" class:nodata={peak < 0} aria-expanded={expanded} aria-label={`Контекст чата: ${peak >= 0 ? `${peak}%` : 'нет данных'}`} title="Контекст" onclick={() => expanded = !expanded}>
    <svg width={SIZE} height={SIZE} viewBox="0 0 {SIZE} {SIZE}" aria-hidden="true">
      <circle cx={C} cy={C} r={R} class="track" />
      {#if peak >= 0}<circle cx={C} cy={C} r={R} class="val {level(peak)}" stroke-dasharray={arc(peak)} />{/if}
    </svg>
    <span class="num {level(peak)}">{peak >= 0 ? peak : '–'}</span>
  </button>
  {#if expanded}
    <div class="card" role="dialog" aria-label="Контекст чата">
      <div class="head"><b>Контекст</b><span class="state" class:busy={!!context?.pending}>{context?.pending ? 'Сжатие идёт' : context?.auto ? 'Автосжатие · 80%' : 'Автосжатие выключено'}</span></div>
      {#each context?.participants ?? [] as p (p.id)}
        {@const v = percent(p)}
        <div class="row">
          <span class="name">{p.name}</span>
          <span class="pct {level(v)}">{v === null ? '–' : `${v}%`}</span>
          <span class="bar" aria-hidden="true"><i class={level(v)} style:width={`${Math.min(100, v ?? 0)}%`}></i><u></u></span>
          <span class="meta">{p.compacted ? 'Сжат, ждёт следующего ответа' : v === null ? 'Объём ещё не сообщён' : `${p.tokens?.toLocaleString('ru-RU')} из ${p.window?.toLocaleString('ru-RU')} токенов`}</span>
        </div>
      {:else}<p class="meta">Данные появятся после первого ответа.</p>{/each}
      <label class="auto">
        <span>Сжимать автоматически</span>
        <input class="switch" type="checkbox" checked={context?.auto ?? true} disabled={busy || !context} onchange={(e) => change({ auto: e.currentTarget.checked })} />
      </label>
      <input class="field" aria-label="Что сохранить в сводке" bind:value={instructions} placeholder="Что сохранить в сводке" />
      <button class="btn sm go" disabled={busy || !!context?.pending || !context?.participants.length} onclick={() => change({ compact: true, instructions })}>Сжать сейчас</button>
      {#if error}<p class="err" role="alert">{error}</p>{/if}
    </div>
  {/if}
</div>
<style>
  .ctx { position: relative; }
  .disc { position: relative; display: grid; place-items: center; width: 32px; height: 32px; border-radius: 50%; color: var(--fg-2); }
  .disc:hover, .disc[aria-expanded='true'], .disc:focus-visible { background: var(--accent-soft); color: var(--fg); }
  .disc svg { position: absolute; inset: 1px; transform: rotate(-90deg); }
  .nodata { opacity: .55; }
  .track { fill: none; stroke: var(--line); stroke-width: 2.5; }
  .val { fill: none; stroke: var(--fg-2); stroke-width: 2.5; stroke-linecap: round; transition: stroke-dasharray .4s var(--ease); }
  .val.warn { stroke: var(--warn); }
  .val.bad { stroke: var(--bad); }
  .num { position: relative; font-size: 10px; font-weight: 600; font-variant-numeric: tabular-nums; letter-spacing: -.02em; }
  .num.warn { color: var(--warn); }
  .num.bad { color: var(--bad); }
  .card { position: absolute; top: calc(100% + 6px); right: 0; z-index: 50; width: 290px; padding: 12px; display: grid; gap: 10px; border: 1px solid var(--line); border-radius: 10px; background: var(--bg-elev, var(--bg)); box-shadow: 0 8px 24px rgba(0,0,0,.28); color: var(--fg); font-size: 12px; }
  .head { display: flex; align-items: baseline; justify-content: space-between; font-size: 13px; }
  .state { color: var(--fg-3); font-size: 11px; }
  .state.busy { color: var(--fg-2); }
  .row { display: grid; grid-template-columns: 1fr auto; gap: 4px 8px; }
  .name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--fg-2); }
  .pct { font-weight: 600; font-variant-numeric: tabular-nums; }
  .pct.warn { color: var(--warn); }
  .pct.bad { color: var(--bad); }
  .bar { grid-column: 1 / -1; position: relative; height: 4px; border-radius: 2px; background: var(--line); overflow: hidden; }
  .bar i { display: block; height: 100%; border-radius: 2px; background: var(--fg-2); transition: width .3s var(--ease); }
  .bar i.warn { background: var(--warn); }
  .bar i.bad { background: var(--bad); }
  .bar u { position: absolute; top: 0; bottom: 0; left: 80%; width: 1px; background: var(--line-strong); }
  .meta { grid-column: 1 / -1; color: var(--fg-3); font-size: 11px; font-variant-numeric: tabular-nums; margin: 0; }
  .auto { display: flex; align-items: center; justify-content: space-between; gap: 10px; padding-top: 10px; border-top: 1px solid var(--line); cursor: pointer; }
  .switch { appearance: none; flex: none; width: 28px; height: 16px; margin: 0; border-radius: 8px; background: var(--bg-active); position: relative; cursor: pointer; transition: background .15s var(--ease); }
  .switch::after { content: ''; position: absolute; top: 2px; left: 2px; width: 12px; height: 12px; border-radius: 50%; background: var(--fg); transition: transform .15s var(--ease); }
  .switch:checked { background: var(--fg-3); }
  .switch:checked::after { transform: translateX(12px); }
  .switch:disabled { opacity: .5; cursor: default; }
  .go { justify-self: stretch; }
  .err { margin: 0; color: var(--bad); font-size: 11px; }
</style>
