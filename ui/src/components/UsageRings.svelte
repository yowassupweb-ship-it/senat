<script lang="ts">
  // Subscription usage of each logged-in agent, compact: the logo in a circle, the inner ring is the five-hour
  // window, the outer ring the long one (week, or month for Kimi) in its own colour. Hover or focus: the details.
  import { app } from '../lib/store.svelte';
  import type { Account } from '../lib/types';
  import { dateTime, timeAgo } from '../lib/format';
  import BrandMark from './BrandMark.svelte';

  type Win = { used: number; resets_at: number };
  const LONG = ['seven_day', 'month'];
  const LABEL: Record<string, string> = { five_hour: '5 часов', seven_day: 'Неделя', month: 'Месяц', seven_day_opus: 'Неделя · Opus', seven_day_sonnet: 'Неделя · Sonnet' };
  const PLAN: Record<string, string> = { plus: 'Plus', pro: 'Pro', max: 'Max', team: 'Team', business: 'Business', enterprise: 'Enterprise', free: 'Free' };

  const shown = $derived(app.accounts.filter((a) => a.state === 'ready'));

  function longOf(a: Account): [string, Win] | null {
    const w = a.limits?.windows ?? {};
    for (const k of LONG) if (w[k]) return [k, w[k]];
    return null;
  }

  // Ring geometry: two concentric arcs around a 20px disc.
  const SIZE = 30;
  const C = SIZE / 2;
  const R_IN = 11.5;
  const R_OUT = 14;
  const arc = (r: number, used: number) => {
    const len = 2 * Math.PI * r;
    return `${(Math.min(100, Math.max(0, used)) / 100) * len} ${len}`;
  };
  // A window that is nearly spent shows it in the warning colours, whichever window it is.
  const tone = (used: number, base: string) => (used >= 95 ? 'var(--bad)' : used >= 80 ? 'var(--warn)' : base);

  function left(sec: number): string {
    if (!sec) return '';
    const s = sec - Date.now() / 1000;
    if (s <= 0) return 'уже обновилось';
    const d = Math.floor(s / 86400);
    const h = Math.floor((s % 86400) / 3600);
    const m = Math.floor((s % 3600) / 60);
    const span = d ? `${d} дн ${h} ч` : h ? `${h} ч ${m} мин` : `${Math.max(1, m)} мин`;
    return `через ${span} · ${dateTime(sec * 1000)}`;
  }

  let open = $state<string | null>(null);
  let timer: ReturnType<typeof setTimeout> | undefined;
  const show = (p: string) => { clearTimeout(timer); open = p; };
  const hide = () => { clearTimeout(timer); timer = setTimeout(() => (open = null), 120); };
</script>

{#if shown.length}
  <div class="rings" role="group" aria-label="Лимиты подписок">
    {#each shown as a (a.provider)}
      {@const w = a.limits?.windows ?? {}}
      {@const short = w.five_hour}
      {@const long = longOf(a)}
      {@const longKey = long?.[0] ?? 'seven_day'}
      {@const longColor = longKey === 'month' ? 'var(--ring-month)' : 'var(--ring-week)'}
      <div class="ring" onmouseenter={() => show(a.provider)} onmouseleave={hide} role="presentation">
        <button
          class="disc"
          class:nodata={!short && !long}
          aria-label={`${a.label}: ${short ? `5 часов ${short.used}%` : ''}${long ? `, ${LABEL[long[0]].toLowerCase()} ${long[1].used}%` : ''}${!short && !long ? 'нет данных о лимитах' : ''}`}
          aria-expanded={open === a.provider}
          onfocus={() => show(a.provider)}
          onblur={hide}
        >
          <svg width={SIZE} height={SIZE} viewBox="0 0 {SIZE} {SIZE}" aria-hidden="true">
            <circle cx={C} cy={C} r={R_OUT} class="track" />
            <circle cx={C} cy={C} r={R_IN} class="track" />
            {#if long}<circle cx={C} cy={C} r={R_OUT} class="val" stroke={tone(long[1].used, longColor)} stroke-dasharray={arc(R_OUT, long[1].used)} />{/if}
            {#if short}<circle cx={C} cy={C} r={R_IN} class="val" stroke={tone(short.used, 'var(--ring-short)')} stroke-dasharray={arc(R_IN, short.used)} />{/if}
          </svg>
          <span class="logo"><BrandMark id={a.provider} size={12} /></span>
        </button>
        {#if open === a.provider}
          <div class="card" role="tooltip">
            <div class="head">
              <BrandMark id={a.provider} size={14} />
              <b>{a.label}</b>
              {#if a.limits?.plan}<span class="plan">{PLAN[a.limits.plan] ?? a.limits.plan}</span>{/if}
            </div>
            {#if short || long}
              {#each Object.entries(w) as [k, win] (k)}
                {@const c = k === 'five_hour' ? 'var(--ring-short)' : k === 'month' ? 'var(--ring-month)' : 'var(--ring-week)'}
                <div class="row">
                  <span class="lbl"><i class="sw" style="background:{c}"></i>{LABEL[k] ?? k}</span>
                  <span class="pct" class:hot={win.used >= 80}>{win.used}%</span>
                  <div class="bar"><i style="width:{Math.min(100, win.used)}%; background:{tone(win.used, c)}"></i></div>
                  {#if win.resets_at}<span class="when">Обновится {left(win.resets_at)}</span>{/if}
                </div>
              {/each}
              {#if a.limits?.at}<p class="foot">Данные {timeAgo(a.limits.at) === 'сейчас' ? 'только что' : `${timeAgo(a.limits.at)} назад`}</p>{/if}
            {:else}
              <p class="foot">Сервис ещё не сообщил лимиты. Они появятся после первой задачи этого агента.</p>
            {/if}
          </div>
        {/if}
      </div>
    {/each}
  </div>
{/if}

<style>
  .rings {
    --ring-short: var(--fg-2);
    --ring-week: #7fa38c; /* design-ok: muted sage, the weekly window */
    --ring-month: #b59a6a; /* design-ok: muted ochre, the monthly window */
    display: flex;
    align-items: center;
    gap: 2px;
  }
  .ring {
    position: relative;
  }
  .disc {
    position: relative;
    display: grid;
    place-items: center;
    width: 32px;
    height: 32px;
    padding: 0;
    border: 0;
    border-radius: 50%;
    background: none;
    color: var(--fg-2);
    cursor: default;
  }
  .disc:hover,
  .disc:focus-visible {
    background: var(--accent-soft);
    color: var(--fg);
  }
  .disc svg {
    position: absolute;
    inset: 1px;
    transform: rotate(-90deg);
  }
  .nodata {
    opacity: 0.55;
  }
  .track {
    fill: none;
    stroke: var(--line);
    stroke-width: 2;
  }
  .val {
    fill: none;
    stroke-width: 2;
    stroke-linecap: round;
    transition: stroke-dasharray 0.4s ease;
  }
  .logo {
    position: relative;
    display: grid;
    place-items: center;
  }
  .card {
    position: absolute;
    top: calc(100% + 6px);
    right: 0;
    z-index: 50;
    width: 260px;
    padding: 10px 12px;
    border: 1px solid var(--line);
    border-radius: 10px;
    background: var(--bg-elev, var(--bg));
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.28);
    color: var(--fg);
    font-size: 12px;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-bottom: 8px;
    font-size: 13px;
  }
  .plan {
    margin-left: auto;
    padding: 1px 6px;
    border-radius: 5px;
    background: var(--accent-soft);
    color: var(--fg-2);
    font-size: 11px;
  }
  .row {
    display: grid;
    grid-template-columns: 1fr auto;
    gap: 3px 8px;
    margin-top: 8px;
  }
  .lbl {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--fg-2);
  }
  .sw {
    width: 8px;
    height: 8px;
    border-radius: 50%;
  }
  .pct {
    font-variant-numeric: tabular-nums;
    font-weight: 600;
  }
  .pct.hot {
    color: var(--warn);
  }
  .bar {
    grid-column: 1 / -1;
    height: 4px;
    border-radius: 2px;
    background: var(--line);
    overflow: hidden;
  }
  .bar i {
    display: block;
    height: 100%;
    border-radius: 2px;
  }
  .when {
    grid-column: 1 / -1;
    color: var(--fg-3);
    font-size: 11px;
  }
  .foot {
    margin: 10px 0 0;
    color: var(--fg-3);
    font-size: 11px;
  }
</style>
