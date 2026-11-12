<script lang="ts">
  import { agentById, app, go, refreshLive } from '../lib/store.svelte';
  import { post } from '../lib/api';
  import type { LiveRun } from '../lib/types';
  import Avatar from './Avatar.svelte';
  import Icon from './Icon.svelte';

  // The right-hand panel: who is working right now, on what, and a way to stop them.
  const KEY = 'senate.live.open';
  // Open if the person left it open (or closed, if they closed it). Never chosen: open only where there is room,
  // so on a narrow window the chat keeps the width.
  const read = () => {
    try {
      const v = localStorage.getItem(KEY);
      if (v === '1') return true;
      if (v === '0') return false;
    } catch {
      /* storage can be unavailable */
    }
    return window.innerWidth >= 1280;
  };
  let open = $state(read());
  function toggle() {
    open = !open;
    try {
      localStorage.setItem(KEY, open ? '1' : '0');
    } catch {
      /* storage can be unavailable: the panel still works */
    }
  }

  const runs = $derived(app.live);
  let now = $state(Date.now());
  const soon = () => refreshLive();

  $effect(() => {
    refreshLive(true);
    const tick = setInterval(() => (now = Date.now()), 1000);
    const safety = setInterval(() => refreshLive(true), 15000);
    return () => {
      clearInterval(tick);
      clearInterval(safety);
    };
  });

  const dur = (since: number) => {
    const s = Math.max(0, Math.round((now - since) / 1000));
    return s >= 3600 ? `${Math.floor(s / 3600)} ч ${Math.floor((s % 3600) / 60)} мин` : `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`;
  };
  const stateOf = (r: LiveRun) => (r.awaiting ? 'ждёт разрешения' : r.status === 'queued' ? 'в очереди' : r.phase || 'работает');

  function openRun(r: LiveRun) {
    if (r.thread_id) go({ kind: 'chat', id: r.thread_id });
    else if (r.obligation_id) go({ kind: 'obligations', id: r.obligation_id });
  }
  async function stopRun(r: LiveRun) {
    if (!r.message_id) return;
    try {
      await post(`/api/messages/${r.message_id}/cancel`);
    } catch {
      /* the next refresh shows what really happened */
    }
    soon();
  }
</script>

<aside class="live" class:rail={!open} aria-label="Сейчас работают">
  {#if open}
    <header>
      <b>Сейчас работают</b>
      {#if runs.length}<span class="count" aria-label={`${runs.length} в работе`}>{runs.length}</span>{/if}
      <span class="grow"></span>
      <button class="icon-btn" onclick={toggle} aria-label="Свернуть панель" title="Свернуть"><Icon name="right" size={15} /></button>
    </header>
    <div class="list">
      {#each runs as r (r.session_id)}
        {@const ag = agentById(r.agent_id)}
        <article class="run" class:wait={r.awaiting} class:queued={r.status === 'queued'}>
          <div class="who">
            <Avatar name={ag?.name} color={ag?.accent} size={26} />
            <div class="nm"><b>{ag?.name ?? 'Специалист'}</b><span class="st">{#if r.status === 'running'}<i class="pulse" aria-hidden="true"></i>{/if}{stateOf(r)} · {dur(r.since)}</span></div>
          </div>
          {#if r.hint}<div class="hint">{r.hint}</div>{/if}
          {#if r.plan}
            <div class="plan" title={r.plan.current}>
              <span class="bar" aria-hidden="true"><i style="width:{Math.round((r.plan.done / Math.max(1, r.plan.total)) * 100)}%"></i></span>
              <span class="pn">{r.plan.done}/{r.plan.total}</span>
              {#if r.plan.current}<span class="pc faint">{r.plan.current}</span>{/if}
            </div>
          {/if}
          <div class="foot">
            <button class="where" onclick={() => openRun(r)} title={r.title || 'Открыть'}>{r.title || (r.kind === 'task' ? 'Задача' : 'Чат')}</button>
            <button class="btn sm" onclick={() => stopRun(r)} aria-label={`Остановить: ${ag?.name ?? 'специалиста'}`}><Icon name="stop" size={10} filled />Стоп</button>
          </div>
        </article>
      {:else}
        <p class="empty muted">Никто не работает. Когда специалист возьмётся за дело, он появится здесь.</p>
      {/each}

    </div>
  {:else}
    <button class="rail-btn" onclick={toggle} aria-label={`Сейчас работают: ${runs.length}. Развернуть панель`} title="Сейчас работают">
      <Icon name="bolt" size={16} />
      {#if runs.length}<span class="count">{runs.length}</span>{/if}
      {#each runs.slice(0, 6) as r (r.session_id)}
        {@const ag = agentById(r.agent_id)}
        <span class="rav" class:on={r.status === 'running'}><Avatar name={ag?.name} color={ag?.accent} size={26} /></span>
      {/each}
    </button>
  {/if}
</aside>

<style>
  .live {
    flex: none;
    width: 300px;
    display: flex;
    flex-direction: column;
    min-height: 0;
    border-left: 1px solid var(--line-strong);
    background: var(--bg);
  }
  .live.rail { width: 56px; }
  header { display: flex; align-items: center; gap: 8px; padding: 14px 10px 10px 16px; font-size: 13px; }
  .grow { flex: 1; }
  .count { min-width: 18px; padding: 0 6px; border-radius: 999px; background: var(--accent); color: var(--accent-fg); font-size: 11px; font-weight: 650; line-height: 18px; text-align: center; font-variant-numeric: tabular-nums; }
  .list { flex: 1; min-height: 0; overflow-y: auto; padding: 4px 12px 16px; display: flex; flex-direction: column; gap: 10px; }
  .empty { margin: 8px 4px; font-size: 12.5px; line-height: 1.5; }

  .run { padding: 10px 12px; border-radius: var(--radius); background: var(--bg-raised); box-shadow: 0 0 0 1px var(--line-strong); display: flex; flex-direction: column; gap: 8px; min-width: 0; overflow: hidden; }
  /* what the specialist is doing: two lines at most, and a long path with no spaces breaks instead of escaping */
  .hint { font-size: 12.5px; line-height: 1.4; min-width: 0; display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; overflow-wrap: anywhere; word-break: break-word; }
  .run.wait { box-shadow: 0 0 0 1.5px var(--warn); }
  .run.queued { opacity: 0.8; }
  .who { display: flex; align-items: center; gap: 9px; min-width: 0; }
  .nm { display: flex; flex-direction: column; min-width: 0; font-size: 13px; line-height: 1.3; }
  .nm b { font-weight: 600; }
  .st { display: inline-flex; align-items: center; gap: 6px; font-size: 11.5px; color: var(--fg-3); font-variant-numeric: tabular-nums; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .run.wait .st { color: var(--warn); }
  .pulse { width: 7px; height: 7px; border-radius: 50%; background: var(--accent); flex: none; animation: livepulse 1.4s ease-in-out infinite; }
  @keyframes livepulse { 0%, 100% { opacity: 1; } 50% { opacity: 0.3; } }
  @media (prefers-reduced-motion: reduce) { .pulse, .rav.on::after { animation: none; } }

  .plan { display: flex; align-items: center; gap: 8px; font-size: 11.5px; min-width: 0; }
  .bar { flex: none; width: 54px; height: 4px; border-radius: 2px; background: var(--bg-active, var(--bg-hover)); overflow: hidden; }
  .bar i { display: block; height: 100%; background: var(--accent); transition: width 0.3s var(--ease); }
  .pn { font-variant-numeric: tabular-nums; font-weight: 600; flex: none; }
  .pc { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; }
  .foot { display: flex; align-items: center; gap: 8px; }
  .where { flex: 1; min-width: 0; text-align: left; font-size: 12px; color: var(--accent); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .where:hover { text-decoration: underline; }
  .foot .btn { display: inline-flex; align-items: center; gap: 5px; }

  .rail-btn { display: flex; flex-direction: column; align-items: center; gap: 12px; width: 100%; padding: 16px 0; color: var(--fg-2, var(--fg)); }
  .rail-btn:hover { background: var(--bg-hover); }
  .rail-btn .count { margin-top: -6px; }
  .rav { position: relative; display: block; line-height: 0; }
  .rav.on::after { content: ''; position: absolute; right: -2px; bottom: -2px; width: 9px; height: 9px; border-radius: 50%; background: var(--accent); box-shadow: 0 0 0 2px var(--bg); animation: livepulse 1.4s ease-in-out infinite; }

  /* narrow windows: the panel would crowd the chat out */
  @media (max-width: 1100px) { .live { display: none; } }
</style>
