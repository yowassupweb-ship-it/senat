<script lang="ts">
  import { onMount } from 'svelte';
  import { get } from '../lib/api';
  import { app } from '../lib/store.svelte';
  import { roleLabel } from '../lib/format';
  import type { Agent } from '../lib/types';
  import Avatar from './Avatar.svelte';

  // The Senate chamber: the specialists sit in pods on a ring around a central podium, and the person speaks from the
  // podium ("I am the Senate"). Pick a pod to address that specialist; pick none and Senate chooses by the subject.
  // On a narrow window the ring becomes a plain wrapped row.
  let { selected = [], onpick, title }: { selected?: string[]; onpick: (id: string) => void; title: string; project?: string } = $props();

  let name = $state('');
  onMount(() => {
    get('/api/settings')
      .then((s) => (name = (s.user_name ?? '').trim()))
      .catch(() => {});
  });

  const agents = $derived(app.agents);
  const working = (a: Agent) => app.live.find((r) => r.agent_id === a.id && r.status === 'running');
  const busyCount = $derived(new Set(app.live.filter((r) => r.status === 'running').map((r) => r.agent_id)).size);
  const hour = new Date().getHours();
  const hello = hour < 5 ? 'Доброй ночи' : hour < 12 ? 'Доброе утро' : hour < 18 ? 'Добрый день' : 'Добрый вечер';
  const isOn = (id: string) => selected.includes(id);
  /** How far a specialist is from the next level, 0..1 (a level starts at 6 * (level-1)^2 experience). */
  const xpFraction = (a: Agent) => {
    const lv = a.level ?? 1;
    const from = (lv - 1) ** 2 * 6;
    const to = a.next_level_xp ?? from + 6;
    return Math.max(0, Math.min(1, ((a.xp ?? 0) - from) / Math.max(1, to - from)));
  };

  const CARD_W = 92;
  const CARD_H = 104;
  let width = $state(0);
  // The ring is switched on and off with a margin: its own height can bring a scrollbar, which narrows the page, which
  // must not flip the layout back and forth (it did: two flickering captions).
  let wide = $state(false);
  $effect(() => {
    if (width >= 720) wide = true;
    else if (width < 600) wide = false;
  });

  // One ring of pods on a big circle whose centre lies down behind the composer: the person speaks from there. The
  // arc is as shallow as the pods allow (a whole pod apart in one direction at every bend); concentric rings go on
  // down and fade out behind the composer. Too narrow for that: a plain wrapped row.
  const ring = $derived.by(() => {
    const n = agents.length;
    if (!wide || n < 1) return null;
    const usable = width - CARD_W - 8;
    for (let deg = n === 1 ? 1 : 36; deg <= 68; deg++) {
      const half = (deg * Math.PI) / 180;
      const R = usable / (2 * Math.sin(half));
      const pts = agents.map((_, i) => {
        const th = n === 1 ? 0 : -half + (2 * half * i) / (n - 1);
        return { x: R * Math.sin(th), y: R * (1 - Math.cos(th)) };
      });
      const clear = pts.every((p, i) => pts.every((q, j) => j <= i || Math.abs(p.x - q.x) >= CARD_W + 6 || Math.abs(p.y - q.y) >= CARD_H + 6));
      if (!clear) continue;
      const drop = R * (1 - Math.cos(half));
      const tail = 150; // rings continue below the last pod, fading out
      return { cx: width / 2, R, pts, top: CARD_H / 2 + 4, height: Math.ceil(drop + CARD_H + 10 + tail), tail, rings: [R - 150, R - 270, R - 390].filter((r) => r > 60) };
    }
    return null;
  });

  // Without the ring the pods wrap in rows, and the rows are made even: nine specialists sit as 5 + 4, not 8 + 1 with
  // one left alone underneath. The width is measured on the outer box, so the rows can widen again with the window.
  const GAP = 8;
  const gridWidth = $derived.by(() => {
    const n = agents.length;
    if (!width || n < 1) return 0;
    const perRow = Math.max(1, Math.floor((width + GAP) / (CARD_W + GAP)));
    const rows = Math.ceil(n / perRow);
    const cols = Math.ceil(n / rows);
    return cols * (CARD_W + GAP) - GAP;
  });
</script>

{#snippet pod(a: Agent, i: number)}
  {@const w = working(a)}
  <button
    class="pod"
    class:on={isOn(a.id)}
    style={(ring ? `left:${ring.cx + ring.pts[i].x - CARD_W / 2}px;top:${ring.top + ring.pts[i].y - CARD_H / 2}px;width:${CARD_W}px;height:${CARD_H}px;--i:${i}` : `--i:${i}`) + `;--glow:${a.accent || 'var(--fg)'}`}
    aria-pressed={isOn(a.id)}
    title={w ? `${a.name}: ${w.hint || 'работает'}` : `${a.name}${roleLabel(a) ? ' · ' + roleLabel(a) : ''} · уровень ${a.level ?? 1}`}
    onclick={() => onpick(a.id)}
  >
    <span class="av"><Avatar name={a.name} color={a.accent} size={52} progress={xpFraction(a)} /><b class="lv" aria-label="Уровень">{a.level ?? 1}</b>{#if w}<i class="dot" aria-label="работает"></i>{/if}</span>
    <span class="nm">{a.name}</span>
  </button>
{/snippet}

<div class="hall">
  <h1>{hello}{name ? `, ${name}` : ''}</h1>
  <p class="sub">Сенат в сборе. {title}</p>

  <!-- The width is measured on this full-width stage, never on the seats: in rows the seats are narrowed to even
       them out, and a narrowed box must not be what decides how wide the rows may be. One list of pods for both
       layouts, so switching between the ring and the rows does not rebuild them (their photos would blink). -->
  <div class="stage" bind:clientWidth={width}>
    <div class="seats" class:ring={!!ring} style={ring ? `height:${ring.height}px` : gridWidth ? `max-width:${gridWidth}px` : ''} role="group" aria-label="Специалисты">
      {#if ring}
        <svg class="tiers" width={width} height={ring.height} aria-hidden="true" style="--tail:{ring.tail}px">
          {#each ring.rings as r, k}
            <circle cx={ring.cx} cy={ring.top + ring.R} {r} fill="none" stroke="currentColor" stroke-opacity={0.12} stroke-linecap="round" />
          {/each}
        </svg>
      {/if}
      {#each agents as a, i (a.id)}{@render pod(a, i)}{/each}
    </div>
  </div>
</div>

<style>
  .hall { width: min(1600px, 100%); margin: 0 auto; padding: 5vh 16px 0; }
  h1 { margin: 0; font-size: clamp(26px, 3vw, 34px); line-height: 1.1; font-weight: 650; letter-spacing: -0.03em; text-align: center; }
  .sub { margin: 6px 0 26px; font-size: 15px; color: var(--fg-2); text-align: center; }

  .stage { width: 100%; }
  .seats { position: relative; color: var(--fg); display: flex; flex-wrap: wrap; justify-content: center; gap: 14px 8px; margin: 0 auto; }
  .seats.ring { display: block; max-width: none; }
  .tiers { position: absolute; left: 0; top: 0; pointer-events: none; -webkit-mask-image: linear-gradient(to bottom, #000 calc(100% - var(--tail)), transparent); mask-image: linear-gradient(to bottom, #000 calc(100% - var(--tail)), transparent); }
  .pod {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: flex-start;
    gap: 7px;
    min-width: 0;
    width: 92px;
    padding: 4px 2px;
    border-radius: 14px;
    text-align: center;
    color: var(--fg-2);
    animation: seat 0.28s var(--ease-out) both;
    animation-delay: calc(var(--i) * 30ms);
    transition: color 0.12s var(--ease), transform 0.12s var(--ease);
  }
  .ring .pod { position: absolute; }
  .pod:hover { color: var(--fg); }
  .pod:hover .av { transform: translateY(-2px); }
  .pod:focus-visible { outline: none; }
  .pod:focus-visible .av::after { content: ''; position: absolute; inset: -6px; border-radius: 50%; box-shadow: 0 0 0 1.5px var(--fg-3); pointer-events: none; z-index: 0; }
  .pod.on { color: var(--fg); }
  /* chosen: the avatar glows in its own colour, softly; no ring, no underline */
  .pod.on .av::after { content: ''; position: absolute; inset: -4px; border-radius: 50%; box-shadow: 0 0 18px 4px color-mix(in srgb, var(--glow) 55%, transparent), 0 0 0 1px color-mix(in srgb, var(--glow) 60%, transparent); pointer-events: none; z-index: 0; transition: box-shadow 0.2s var(--ease); }
  .av { position: relative; display: grid; isolation: isolate; transition: transform 0.12s var(--ease); }
  .av :global(.av) { z-index: 1; }
  .lv { position: absolute; z-index: 3; right: -5px; top: -4px; min-width: 19px; height: 19px; padding: 0 5px; border-radius: 10px; display: grid; place-items: center; font-size: 11px; font-weight: 700; font-variant-numeric: tabular-nums; color: var(--bg); background: var(--fg); box-shadow: 0 0 0 2px var(--bg); }
  .dot { position: absolute; z-index: 3; right: -1px; bottom: -1px; width: 9px; height: 9px; border-radius: 50%; background: var(--warn, #d29922); box-shadow: 0 0 0 2px var(--bg); }
  .nm {
    font-size: 11.5px;
    font-weight: 600;
    line-height: 1.2;
    max-width: 100%;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
    text-wrap: balance;
  }
  @keyframes seat { from { opacity: 0; } to { opacity: 1; } }
  @media (prefers-reduced-motion: reduce) { .pod { animation: none; } }
</style>
