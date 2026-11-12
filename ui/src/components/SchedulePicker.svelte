<script lang="ts">
  import { describeCron } from '../lib/cron';
  import Icon from './Icon.svelte';

  // Builds a cron line from plain choices: how often, which days, what time. Anything unusual stays editable as text.
  let { value, onchange, block = false }: { value: string; onchange: (cron: string) => void; block?: boolean } = $props();

  type Mode = 'hourly' | 'daily' | 'weekdays' | 'weekly' | 'monthly' | 'custom';
  const DAYS = [
    { n: 1, s: 'Пн' },
    { n: 2, s: 'Вт' },
    { n: 3, s: 'Ср' },
    { n: 4, s: 'Чт' },
    { n: 5, s: 'Пт' },
    { n: 6, s: 'Сб' },
    { n: 0, s: 'Вс' },
  ];
  const MODES: { id: Mode; label: string }[] = [
    { id: 'daily', label: 'Каждый день' },
    { id: 'weekdays', label: 'По будням' },
    { id: 'weekly', label: 'По дням недели' },
    { id: 'monthly', label: 'Раз в месяц' },
    { id: 'hourly', label: 'Каждый час' },
    { id: 'custom', label: 'Своё (cron)' },
  ];

  let open = $state(false);
  let root: HTMLElement;
  let mode = $state<Mode>('daily');
  let time = $state('09:00');
  let days = $state<number[]>([1]);
  let dom = $state(1);
  let minute = $state(0);
  let raw = $state('');

  const pad = (n: number) => String(n).padStart(2, '0');

  function parse(cron: string) {
    raw = cron;
    const f = cron.trim().split(/\s+/);
    if (f.length !== 5) { mode = 'custom'; return; }
    const [mi, h, d, mo, w] = f;
    if (!/^\d+$/.test(mi) || mo !== '*') { mode = 'custom'; return; }
    minute = +mi;
    if (h === '*' && d === '*' && w === '*') { mode = 'hourly'; return; }
    if (!/^\d+$/.test(h)) { mode = 'custom'; return; }
    time = `${pad(+h)}:${pad(+mi)}`;
    if (d === '*' && w === '*') { mode = 'daily'; return; }
    if (d === '*' && w === '1-5') { mode = 'weekdays'; return; }
    if (d === '*' && /^[0-7](,[0-7])*$/.test(w)) { mode = 'weekly'; days = w.split(',').map((x) => +x % 7); return; }
    if (w === '*' && /^\d+$/.test(d) && +d >= 1 && +d <= 31) { mode = 'monthly'; dom = +d; return; }
    mode = 'custom';
  }

  function build(): string {
    const [h, m] = time.split(':').map((x) => +x || 0);
    switch (mode) {
      case 'hourly': return `${minute} * * * *`;
      case 'daily': return `${m} ${h} * * *`;
      case 'weekdays': return `${m} ${h} * * 1-5`;
      case 'weekly': return `${m} ${h} * * ${[...(days.length ? days : [1])].sort((a, b) => ((a + 6) % 7) - ((b + 6) % 7)).join(',')}`;
      case 'monthly': return `${m} ${h} ${Math.min(31, Math.max(1, dom || 1))} * *`;
      default: return raw.trim() || '0 9 * * *';
    }
  }
  function commit() {
    const c = build();
    if (c !== value) onchange(c);
  }
  function toggleOpen() {
    if (!open) parse(value);
    open = !open;
  }
  function toggleDay(n: number) {
    days = days.includes(n) ? days.filter((x) => x !== n) : [...days, n];
    commit();
  }
  function onDoc(e: MouseEvent) {
    if (open && !root.contains(e.target as Node)) open = false;
  }
</script>

<svelte:window onmousedown={onDoc} onkeydown={(e) => { if (e.key === 'Escape') open = false; }} />

<div class="sp" class:block bind:this={root}>
  <button type="button" class="trigger" class:block aria-haspopup="dialog" aria-expanded={open} onclick={toggleOpen}>
    <span class="tl">{describeCron(value)}</span><Icon name="down" size={12} />
  </button>
  {#if open}
    <div class="pop" role="dialog" aria-label="Расписание">
      <div class="modes" role="radiogroup" aria-label="Как часто">
        {#each MODES as m}
          <button type="button" role="radio" aria-checked={mode === m.id} class:on={mode === m.id} onclick={() => { if (m.id === 'custom') raw = build(); mode = m.id; commit(); }}>{m.label}</button>
        {/each}
      </div>

      {#if mode === 'weekly'}
        <div class="days" role="group" aria-label="Дни недели">
          {#each DAYS as d}<button type="button" class:on={days.includes(d.n)} aria-pressed={days.includes(d.n)} onclick={() => toggleDay(d.n)}>{d.s}</button>{/each}
        </div>
      {/if}
      {#if mode === 'monthly'}
        <label class="row">Число месяца<input type="number" min="1" max="31" bind:value={dom} onchange={commit} /></label>
      {/if}
      {#if mode === 'hourly'}
        <label class="row">Минута часа<input type="number" min="0" max="59" bind:value={minute} onchange={commit} /></label>
      {:else if mode !== 'custom'}
        <label class="row">Время<input type="time" bind:value={time} onchange={commit} /></label>
      {/if}
      {#if mode === 'custom'}
        <label class="row col">Расписание cron <small>минуты часы число месяц день_недели</small>
          <input class="mono" bind:value={raw} onchange={commit} placeholder="0 9 * * 1" /></label>
      {/if}
      <p class="sum">{describeCron(build())}. Время по этому компьютеру.</p>
    </div>
  {/if}
</div>

<style>
  .sp { position: relative; display: inline-block; }
  .sp.block { display: block; }
  .trigger { display: inline-flex; align-items: center; gap: 6px; height: 32px; padding: 0 12px; border-radius: 999px; background: var(--bg-hover); color: var(--fg-2); font-size: 12.5px; }
  .trigger:hover, .trigger[aria-expanded='true'] { color: var(--fg); background: var(--bg-active); }
  .trigger:focus-visible { outline: none; box-shadow: inset 0 0 0 1px var(--fg-3); }
  .trigger.block { width: 100%; height: 34px; border-radius: var(--radius-s); border: 1px solid var(--line-strong); background: var(--bg-raised); color: var(--fg); justify-content: space-between; padding: 0 10px; font-size: 13px; }
  .tl { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; max-width: 230px; }
  .pop { position: absolute; z-index: 40; right: 0; top: calc(100% + 8px); width: 316px; padding: 12px; border-radius: 14px; background: var(--bg-raised); box-shadow: var(--shadow-2), inset 0 0 0 1px var(--line); display: flex; flex-direction: column; gap: 12px; text-align: left; }
  .modes { display: flex; flex-wrap: wrap; gap: 6px; }
  .modes button, .days button { height: 30px; padding: 0 11px; border-radius: 999px; background: var(--bg-hover); color: var(--fg-2); font-size: 12.5px; }
  .modes button:hover, .days button:hover { color: var(--fg); }
  .modes button.on, .days button.on { background: var(--fg); color: var(--bg); font-weight: 600; }
  .days { display: grid; grid-template-columns: repeat(7, 1fr); gap: 4px; }
  .days button { padding: 0; border-radius: 8px; }
  .row { display: flex; align-items: center; justify-content: space-between; gap: 12px; font-size: 13px; color: var(--fg-2); }
  .row.col { flex-direction: column; align-items: stretch; gap: 6px; }
  .row small { color: var(--fg-3); }
  input[type='time'], input[type='number'], .row input.mono { height: 32px; border-radius: 8px; padding: 0 10px; background: var(--bg); color: var(--fg); border: 0; box-shadow: inset 0 0 0 1px var(--line-strong); font: inherit; font-size: 13px; color-scheme: dark; }
  input[type='number'] { width: 80px; }
  input:focus { outline: none; box-shadow: inset 0 0 0 1px var(--fg-3); }
  .sum { margin: 0; font-size: 12.5px; color: var(--fg-3); }
  @media (prefers-color-scheme: light) { input[type='time'], input[type='number'] { color-scheme: light; } }
</style>
