<script lang="ts">
  import { app } from '../lib/store.svelte';
  import { roleLabel } from '../lib/format';
  import type { Agent } from '../lib/types';
  import Avatar from './Avatar.svelte';

  // The council before the work starts: every specialist on an equal seat. An arc looked right with four names and
  // collided with eight, so the layout is one that cannot collide — fixed cells, centred, wrapping. Pick a seat to
  // address that specialist; pick none and Senate chooses by the subject.
  let { selected = '', onpick, title }: { selected?: string; onpick: (id: string) => void; title: string; project?: string } = $props();

  const agents = $derived(app.agents);
  const working = (a: Agent) => app.live.find((r) => r.agent_id === a.id && r.status === 'running');
  const busyCount = $derived(new Set(app.live.filter((r) => r.status === 'running').map((r) => r.agent_id)).size);
  const hour = new Date().getHours();
  const hello = hour < 5 ? 'Доброй ночи' : hour < 12 ? 'Доброе утро' : hour < 18 ? 'Добрый день' : 'Добрый вечер';
  const chosen = $derived(agents.find((a) => a.id === selected));
</script>

<div class="hall">
  <p class="eyebrow">Зал совета</p>
  <h1>{hello}</h1>
  <p class="sub">{title}</p>

  <div class="seats" role="group" aria-label="Специалисты">
    {#each agents as a, i (a.id)}
      {@const w = working(a)}
      <button
        class="pod"
        class:on={selected === a.id}
        style="--i:{i}"
        aria-pressed={selected === a.id}
        title={w ? `${a.name}: ${w.hint || 'работает'}` : roleLabel(a) || a.name}
        onclick={() => onpick(selected === a.id ? '' : a.id)}
      >
        <Avatar name={a.name} color={a.accent} size={40} />
        <span class="nm">{a.name}</span>
        <span class="st">{w ? 'работает' : roleLabel(a) || 'свободен'}</span>
      </button>
    {/each}
  </div>

  <p class="hint faint">
    {#if chosen}
      Обращение к: <b>{chosen.name}</b>. Нажмите ещё раз, чтобы Senate выбирал сам.
    {:else}
      Выберите, к кому обратиться, или просто напишите: Senate подберёт специалиста по теме.
    {/if}
    {#if busyCount}<span class="live"> Сейчас работают: {busyCount}</span>{/if}
  </p>
</div>

<style>
  .hall {
    width: min(900px, 100%);
    margin: 0 auto;
    display: flex;
    flex-direction: column;
    align-items: center;
    text-align: center;
    padding: 2vh 12px 0;
  }
  .eyebrow {
    margin: 0 0 10px;
    font-size: 10.5px;
    font-weight: 600;
    letter-spacing: 0.18em;
    text-transform: uppercase;
    color: var(--fg-3);
  }
  h1 {
    margin: 0;
    font-size: clamp(28px, 3.4vw, 38px);
    line-height: 1.08;
    font-weight: 650;
    letter-spacing: -0.035em;
  }
  .sub {
    margin: 8px 0 0;
    font-size: 14px;
    color: var(--fg-2);
  }

  /* fixed cells: the row wraps, nothing ever lands on a neighbour */
  .seats {
    display: grid;
    grid-template-columns: repeat(auto-fit, 112px);
    justify-content: center;
    gap: 6px 4px;
    margin: 26px 0 20px;
    width: 100%;
  }
  .pod {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 7px;
    padding: 12px 6px 10px;
    border-radius: var(--radius);
    animation: seat 0.24s var(--ease-out) both;
    animation-delay: calc(var(--i) * 30ms);
    transition: background 0.12s var(--ease), box-shadow 0.12s var(--ease);
  }
  @keyframes seat {
    from {
      opacity: 0;
    }
    to {
      opacity: 1;
    }
  }
  .pod:hover {
    background: var(--bg-hover);
  }
  .pod:focus-visible {
    outline: none;
    box-shadow: var(--focus-ring);
  }
  .pod.on {
    background: var(--bg-active);
    box-shadow: inset 0 0 0 1px var(--line-strong);
  }
  .nm {
    font-size: 12.5px;
    font-weight: 600;
    line-height: 1.25;
    /* two lines of room whatever the name: the status below keeps one baseline across the row */
    min-height: calc(2 * 1.25em);
    max-width: 100%;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
    text-wrap: balance;
  }
  .st {
    font-size: 11px;
    color: var(--fg-3);
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .pod.on .st {
    color: var(--fg-2);
  }

  .hint {
    margin: 0;
    font-size: 13px;
    max-width: 560px;
    line-height: 1.5;
  }
  .hint b {
    color: var(--fg);
  }
  .live {
    color: var(--fg-2);
  }

  @media (max-width: 520px) {
    .seats {
      grid-template-columns: repeat(auto-fit, 96px);
      margin: 18px 0 16px;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .pod {
      animation: none;
    }
  }
</style>
