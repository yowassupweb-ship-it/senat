<script lang="ts">
  import { get } from '../lib/api';
  import { agentById, go, onEvent } from '../lib/store.svelte';
  import { STATE_LABEL } from '../lib/format';
  import type { CalendarItem } from '../lib/types';
  import Avatar from './Avatar.svelte';
  import Icon from './Icon.svelte';

  // A month of work: what the schedules will do (outlined), what they already started (filled, with the state of the
  // task), and the tasks that have a deadline. Click a day for the list.
  const DAY = 86_400_000;
  const weekdays = ['Пн', 'Вт', 'Ср', 'Чт', 'Пт', 'Сб', 'Вс'];
  const startOfDay = (d: Date) => new Date(d.getFullYear(), d.getMonth(), d.getDate());

  let cursor = $state(new Date(new Date().getFullYear(), new Date().getMonth(), 1));
  let items = $state<CalendarItem[]>([]);
  let selected = $state<string>(key(new Date()));
  let error = $state('');

  function key(d: Date) {
    return `${d.getFullYear()}-${d.getMonth() + 1}-${d.getDate()}`;
  }
  // six weeks from the Monday on or before the 1st
  const gridStart = $derived.by(() => {
    const first = new Date(cursor.getFullYear(), cursor.getMonth(), 1);
    const back = (first.getDay() + 6) % 7;
    return new Date(first.getFullYear(), first.getMonth(), 1 - back);
  });
  const days = $derived(Array.from({ length: 42 }, (_, i) => new Date(gridStart.getFullYear(), gridStart.getMonth(), gridStart.getDate() + i)));

  async function load() {
    try {
      const from = gridStart.getTime();
      const r = await get(`/api/calendar?from=${from}&to=${from + 42 * DAY}`);
      items = r.items;
      error = '';
    } catch (e: any) {
      error = e.message;
    }
  }
  $effect(() => {
    gridStart;
    load();
    return onEvent((t) => {
      if (t === 'routines' || t === 'attention') load();
    });
  });

  const byDay = $derived.by(() => {
    const m = new Map<string, CalendarItem[]>();
    for (const it of items) {
      const k = key(new Date(it.at));
      (m.get(k) ?? m.set(k, []).get(k)!).push(it);
    }
    return m;
  });
  const upper = (s: string) => s.charAt(0).toUpperCase() + s.slice(1);
  const monthTitle = $derived(upper(cursor.toLocaleString('ru-RU', { month: 'long', year: 'numeric' })));
  const todayKey = key(new Date());
  const time = (ms: number) => new Date(ms).toLocaleTimeString('ru-RU', { hour: '2-digit', minute: '2-digit' });
  const shift = (n: number) => (cursor = new Date(cursor.getFullYear(), cursor.getMonth() + n, 1));
  function today() {
    cursor = new Date(new Date().getFullYear(), new Date().getMonth(), 1);
    selected = todayKey;
  }
  const agentColor = (id?: string | null) => agentById(id)?.accent ?? 'var(--fg-3)';
  const kindLabel = (i: CalendarItem) => (i.kind === 'planned' ? 'по расписанию' : i.kind === 'run' ? STATE_LABEL[i.state ?? ''] ?? 'запущена' : 'срок');
  const dayItems = $derived(byDay.get(selected) ?? []);
  const selectedDate = $derived.by(() => {
    const d = days.find((x) => key(x) === selected);
    return d ? upper(d.toLocaleDateString('ru-RU', { weekday: 'long', day: 'numeric', month: 'long' })) : '';
  });
  function open(i: CalendarItem) {
    if (i.obligation_id) go({ kind: 'obligations', id: i.obligation_id });
  }
</script>

<section class="cal" aria-label="Календарь задач">
  <header>
    <button class="icon-btn" onclick={() => shift(-1)} aria-label="Прошлый месяц"><Icon name="right" size={15} /></button>
    <h2>{monthTitle}</h2>
    <button class="icon-btn" onclick={() => shift(1)} aria-label="Следующий месяц"><Icon name="right" size={15} /></button>
    <button class="btn sm" onclick={today}>Сегодня</button>
    <span class="legend faint"><i class="sw planned"></i>по расписанию <i class="sw run"></i>запущено <i class="sw due"></i>срок</span>
  </header>
  {#if error}<p class="bad">{error}</p>{/if}

  <div class="grid" role="group" aria-label="Дни месяца">
    {#each weekdays as w}<div class="wd" aria-hidden="true">{w}</div>{/each}
    {#each days as d (d.getTime())}
      {@const k = key(d)}
      {@const list = byDay.get(k) ?? []}
      <button class="day" class:other={d.getMonth() !== cursor.getMonth()} class:today={k === todayKey} class:sel={k === selected} onclick={() => (selected = k)} aria-pressed={k === selected} aria-label={`${d.toLocaleDateString('ru-RU', { day: 'numeric', month: 'long' })}: ${list.length ? list.length + ' событий' : 'пусто'}`}>
        <span class="n">{d.getDate()}</span>
        {#each list.slice(0, 3) as it}
          <span class="chip {it.kind}" style="--c:{agentColor(it.agent_id)}" title={`${time(it.at)} ${it.title}`}>{#if it.kind === 'due'}<Icon name="flag" size={9} />{/if}<b>{time(it.at)}</b> {it.title}</span>
        {/each}
        {#if list.length > 3}<span class="more faint">ещё {list.length - 3}</span>{/if}
      </button>
    {/each}
  </div>

  <div class="dayview">
    <h3>{selectedDate}</h3>
    <ul>
      {#each dayItems as it, i (i)}
        {@const ag = agentById(it.agent_id)}
        <li>
          <time class="mono">{time(it.at)}</time>
          <Avatar name={ag?.name} color={ag?.accent} size={20} />
          <span class="grow"><b>{it.title}</b><small class="faint">{ag?.name ?? 'без исполнителя'} · {kindLabel(it)}</small></span>
          {#if it.obligation_id}<button class="btn sm" onclick={() => open(it)}>Открыть</button>{/if}
        </li>
      {:else}
        <li class="faint">В этот день ничего нет</li>
      {/each}
    </ul>
  </div>
</section>

<style>
  .cal { display: flex; flex-direction: column; gap: 14px; }
  header { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  h2 { margin: 0 6px; font-size: 16px; font-weight: 650; min-width: 150px; text-align: center; }
  header .icon-btn:first-child :global(svg) { transform: scaleX(-1); }
  .legend { margin-left: auto; display: inline-flex; align-items: center; gap: 6px; font-size: 12px; }
  .sw { display: inline-block; width: 12px; height: 8px; border-radius: 3px; margin-left: 8px; }
  .sw.planned { box-shadow: inset 0 0 0 1.5px var(--accent); }
  .sw.run { background: var(--accent); }
  .sw.due { background: var(--warn); }
  .bad { color: var(--bad); margin: 0; }

  .grid { display: grid; grid-template-columns: repeat(7, minmax(0, 1fr)); gap: 4px; }
  .wd { padding: 4px 6px; font-size: 11.5px; font-weight: 600; color: var(--fg-3); text-transform: uppercase; letter-spacing: 0.03em; }
  .day { min-height: 104px; padding: 6px; display: flex; flex-direction: column; gap: 3px; align-items: stretch; text-align: left; border-radius: var(--radius-s); background: var(--bg-raised); box-shadow: 0 0 0 1px var(--line); overflow: hidden; }
  .day:hover { background: var(--bg-hover); }
  .day.other { background: transparent; }
  .day.other .n { color: var(--fg-3); }
  .day.other .chip { opacity: 0.75; }
  .day.today .n { background: var(--accent); color: var(--accent-fg); }
  .day.sel { box-shadow: 0 0 0 2px var(--accent); }
  .day:focus-visible { outline: none; box-shadow: var(--focus-ring); }
  .n { align-self: flex-start; min-width: 20px; padding: 0 5px; border-radius: 10px; font-size: 12px; line-height: 20px; text-align: center; font-variant-numeric: tabular-nums; }
  .chip { display: block; padding: 1px 6px; border-radius: 5px; font-size: 11px; line-height: 17px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; color: var(--fg); }
  .chip b { font-weight: 600; font-variant-numeric: tabular-nums; }
  .chip.planned { box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--c) 70%, transparent); background: transparent; }
  .chip.run { background: color-mix(in srgb, var(--c) 28%, transparent); }
  .chip.due { background: var(--warn-soft); color: var(--fg); }
  .more { font-size: 11px; padding-left: 4px; }

  .dayview { padding: 4px 2px; }
  h3 { margin: 0 0 8px; font-size: 14px; font-weight: 650; }
  ul { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 6px; }
  li { display: flex; align-items: center; gap: 10px; padding: 8px 12px; border-radius: var(--radius-s); background: var(--bg-raised); box-shadow: 0 0 0 1px var(--line); font-size: 13.5px; }
  li time { flex: none; width: 44px; color: var(--fg-3); font-size: 12.5px; }
  li .grow { flex: 1; min-width: 0; display: flex; flex-direction: column; }
  li small { font-size: 12px; }
  @media (max-width: 760px) { .day { min-height: 64px; } .chip b { display: none; } .legend { display: none; } }
</style>
