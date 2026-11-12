<script lang="ts">
  import { app } from '../lib/store.svelte';
  import { fileUrl } from '../lib/api';

  let { name = '', color = '', size = 24, src, progress }: { name?: string; color?: string; size?: number; src?: string; progress?: number } = $props();

  // A specialist has a colour of their own, worn quietly: it tints the disc and its edge, while the initial and
  // every label around it stay in the interface's own ink. Identity without a carnival.
  const agent = $derived(app.agents.find((a) => a.name === name));
  const photo = $derived(src ?? agent?.avatar ?? '');
  const working = $derived(!!agent && app.live.some((r) => r.agent_id === agent.id && r.status === 'running'));
  let broken = $state(false);
  $effect(() => {
    photo;
    broken = false;
  });
  const deg = $derived(Math.max(0, Math.min(1, progress ?? 0)) * 360);
  const c = $derived(color || agent?.accent || 'var(--fg-3)');
</script>

<span class="av" class:working class:photo={!!photo && !broken} class:xp={progress !== undefined} style="--c:{c};--deg:{deg}deg;width:{size}px;height:{size}px;font-size:{Math.round(size * 0.44)}px" aria-hidden="true">
  <span class="face">{#if photo && !broken}<img src={fileUrl(photo)} alt="" draggable="false" onerror={() => (broken = true)} />{:else}{(name || '?').trim().charAt(0).toUpperCase()}{/if}</span>
</span>

<style>
  .av {
    position: relative;
    display: inline-block;
    flex: none;
    border-radius: 50%;
  }
  .face {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    border-radius: 50%;
    overflow: hidden;
    background: color-mix(in srgb, var(--c) 26%, var(--bg-active));
    color: var(--fg);
    font-weight: 600;
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--c) 45%, var(--line-strong));
  }
  img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }

  /* working: a thin ring turns in the specialist's own colour */
  .working::after {
    content: '';
    position: absolute;
    inset: -2.5px;
    border-radius: 50%;
    border: 1.5px solid var(--line);
    border-top-color: var(--c);
    animation: runlight 1.1s linear infinite;
  }
  @keyframes runlight {
    to {
      transform: rotate(360deg);
    }
  }

  /* experience: how far to the next level */
  .xp::before {
    content: '';
    position: absolute;
    inset: -4px;
    border-radius: 50%;
    background: conic-gradient(from -90deg, var(--c) var(--deg), var(--line) 0);
    -webkit-mask: radial-gradient(farthest-side, transparent calc(100% - 2.5px), #000 calc(100% - 2px));
    mask: radial-gradient(farthest-side, transparent calc(100% - 2.5px), #000 calc(100% - 2px));
  }
  @media (prefers-reduced-motion: reduce) {
    .working::after {
      animation: none;
    }
  }
</style>
