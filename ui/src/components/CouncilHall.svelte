<script lang="ts">
  // A council as a small senate chamber: tiers of seats on arcs, the convener at the rostrum, a wave going out
  // from the rostrum while the council is still working. 16:9. Click: open or close the discussion below it.
  import { agentById } from '../lib/store.svelte';
  import Avatar from './Avatar.svelte';
  import Icon from './Icon.svelte';
  type Seat = 'wait' | 'live' | 'ok' | 'bad';
  let { who, members, stateOf, working, done, called, decided, verdict, time, shut, ontoggle }: {
    who: string; members: string[]; stateOf: (id: string) => Seat; working: boolean; done: number; called: number;
    decided: boolean; verdict: string; time: string; shut: boolean; ontoggle: () => void;
  } = $props();
  const convener = $derived(agentById(who));
  // the chamber is drawn on a 160 x 90 plane; the rostrum sits at the bottom centre
  const CX = 80, CY = 80;
  const TIERS = [70, 54, 38];
  const seats = $derived.by(() => {
    const n = members.length;
    const rings = n <= 5 ? 1 : n <= 12 ? 2 : 3;
    const radii = rings === 1 ? [52] : rings === 2 ? [62, 42] : [68, 52, 36];
    const per = radii.map((_, i) => Math.floor(n / rings) + (i < n % rings ? 1 : 0));
    const out: { id: string; x: number; y: number }[] = [];
    let k = 0;
    radii.forEach((r, i) => {
      for (let j = 0; j < per[i]; j++) {
        const th = Math.PI - ((j + 0.5) / per[i]) * Math.PI;
        out.push({ id: members[k++], x: CX + r * Math.cos(th), y: CY - r * Math.sin(th) });
      }
    });
    return out;
  });
  const arc = (r: number) => `M${CX - r} ${CY} A${r} ${r} 0 0 1 ${CX + r} ${CY}`;
</script>

<div class="hall" class:working>
  <button class="surface" onclick={ontoggle} aria-expanded={!shut} aria-label={shut ? 'Развернуть обсуждение совета' : 'Свернуть обсуждение совета'}>
    <svg viewBox="0 0 160 90" preserveAspectRatio="xMidYMid meet" aria-hidden="true">
      {#each TIERS as r (r)}<path d={arc(r)} class="tier" />{/each}
      <path d={arc(10)} class="tier rostrum" />
      {#if working}
        <path d={arc(70)} class="wave w1" /><path d={arc(70)} class="wave w2" /><path d={arc(70)} class="wave w3" />
      {/if}
      <line x1="6" y1={CY} x2="154" y2={CY} class="floor" />
    </svg>
    <span class="head">
      <span class="chev" class:open={!shut}><Icon name="right" size={11} /></span>
      <span class="title"><b>{convener?.name ?? 'Специалист'}</b> {called > 1 ? 'собрал совет' : 'позвал коллегу'}</span>
    </span>
    <span class="state" class:live={working}>
      {#if working}<span class="loader"></span>ответили {done} из {called}{:else if decided}<Icon name="check" size={12} />решение готово{:else}ответили {done} из {called}{/if}
    </span>
    {#each seats as s (s.id)}
      {@const a = agentById(s.id)}
      <span class="seat {stateOf(s.id)}" style="left:{s.x / 160 * 100}%;top:{s.y / 90 * 100}%" title={a?.name ?? 'Участник'}>
        <Avatar name={a?.name} color={a?.accent} size={26} />
      </span>
    {/each}
    <span class="podium" style="left:{CX / 160 * 100}%;top:{CY / 90 * 100}%" title={convener?.name}><Avatar name={convener?.name} color={convener?.accent} size={34} /></span>
    <time class="time">{time}</time>
    {#if shut && verdict}<span class="verdict"><span class="vl">Решение</span>{verdict}</span>{/if}
  </button>
</div>

<style>
  .hall { margin: 8px auto 12px; width: min(100%, 40rem); }
  .surface { position: relative; display: block; width: 100%; aspect-ratio: 16 / 9; overflow: hidden; border: 1px solid var(--line); border-radius: 14px; background: var(--bg-raised); text-align: left; font-size: 12px; }
  .surface:hover { border-color: var(--line-strong); }
  svg { position: absolute; inset: 0; width: 100%; height: 100%; }
  .tier { fill: none; stroke: var(--line); stroke-width: .5; }
  .rostrum { stroke: var(--line-strong); }
  .floor { stroke: var(--line); stroke-width: .4; }
  .wave { fill: none; stroke: var(--fg-3); stroke-width: .5; opacity: 0; transform-origin: 80px 80px; animation: hall-wave 3.6s ease-out infinite; }
  .w2 { animation-delay: 1.2s; }
  .w3 { animation-delay: 2.4s; }
  @keyframes hall-wave { 0% { transform: scale(.15); opacity: .55; } 100% { transform: scale(1); opacity: 0; } }
  .head { position: absolute; top: 10px; left: 12px; right: 150px; display: flex; align-items: center; gap: 6px; color: var(--fg-2); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .head b { color: var(--fg); font-weight: 600; }
  .chev { display: inline-flex; color: var(--fg-3); transition: transform .15s var(--ease); }
  .chev.open { transform: rotate(90deg); }
  .state { position: absolute; top: 10px; right: 12px; display: inline-flex; align-items: center; gap: 5px; color: var(--fg-3); font-size: 11px; font-variant-numeric: tabular-nums; }
  .state.live { color: var(--fg-2); }
  .loader { width: 10px; height: 10px; border-radius: 50%; border: 1.5px solid var(--line-strong); border-top-color: var(--fg); animation: hall-spin .8s linear infinite; }
  @keyframes hall-spin { to { transform: rotate(360deg); } }
  .seat, .podium { position: absolute; transform: translate(-50%, -50%); display: inline-flex; border-radius: 50%; box-shadow: 0 0 0 3px var(--bg-raised); }
  .seat.wait { opacity: .4; }
  .seat.live::after { content: ''; position: absolute; inset: -3px; border-radius: 50%; border: 2px solid transparent; border-top-color: var(--fg-2); animation: hall-spin .9s linear infinite; }
  .seat.ok::after, .seat.bad::after { content: ''; position: absolute; right: -3px; bottom: -3px; width: 9px; height: 9px; border-radius: 50%; background: var(--ok); box-shadow: 0 0 0 2px var(--bg-raised); }
  .seat.bad::after { background: var(--bad); }
  .time { position: absolute; right: 12px; bottom: 8px; color: var(--fg-3); font-size: 11px; }
  .verdict { position: absolute; left: 12px; right: 56px; bottom: 8px; overflow: hidden; white-space: nowrap; text-overflow: ellipsis; color: var(--fg); font-size: 12px; }
  .vl { margin-right: 6px; color: var(--fg-3); font-weight: 600; }
  @media (prefers-reduced-motion: reduce) { .wave { animation: none; } .loader, .seat.live::after { animation-duration: 2.4s; } }
  @media (max-width: 520px) { .state { display: none; } .head { right: 12px; } }
</style>
