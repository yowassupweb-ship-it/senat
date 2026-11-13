<script lang="ts">
  import { agentById, app, go, refreshLive } from '../lib/store.svelte';
  import { post } from '../lib/api';
  import type { LiveRun } from '../lib/types';
  import Avatar from './Avatar.svelte';
  import Icon from './Icon.svelte';
  import Splitter from './Splitter.svelte';

  // The right-hand panel: who is working right now, on what, and a way to stop them.
  // It takes room only when there is something to show: while somebody works it opens by itself, when the last one
  // finishes it folds back into a narrow strip and the chat gets the width. The person can still decide:
  //   * opening it by hand while nobody works pins it open (remembered);
  //   * folding it by hand while somebody works keeps it folded until that work is over.
  // On a narrow window it never opens by itself.
  const KEY = 'senate.live.pin';
  const readPin = () => {
    try {
      return localStorage.getItem(KEY) === '1';
    } catch {
      return false;
    }
  };
  const savePin = (v: boolean) => {
    try {
      if (v) localStorage.setItem(KEY, '1');
      else localStorage.removeItem(KEY);
    } catch {
      /* storage can be unavailable: the panel still works */
    }
  };
  let pinned = $state(readPin());
  let dismissed = $state(false);
  let wide = $state(window.innerWidth >= 1280);
  const runs = $derived(app.live);
  // One card per specialist: what they do now, and below it what waits for them — not a card per queued turn.
  const rank = (r: LiveRun) => (r.awaiting ? 0 : r.status === 'running' ? 1 : 2);
  // A council session: colleagues called into a chat by one specialist, with that specialist's own turns there — one
  // card, every member with their state, instead of a card per member.
  const councils = $derived.by(() => {
    const by = new Map<string, { key: string; caller: string; thread_id: string | null; title: string; members: LiveRun[] }>();
    for (const r of runs) {
      const caller = r.asked_by;
      if (!caller) continue;
      const key = `${r.thread_id}:${caller}`;
      const c = by.get(key) ?? { key, caller, thread_id: r.thread_id, title: r.title, members: [] };
      c.members.push(r);
      by.set(key, c);
    }
    // the caller's own turns in that chat belong to the session too
    for (const c of by.values()) for (const r of runs) if (r.agent_id === c.caller && r.thread_id === c.thread_id && !r.asked_by) c.members.push(r);
    for (const c of by.values()) c.members.sort((a, b) => rank(a) - rank(b) || a.since - b.since);
    return [...by.values()];
  });
  const inCouncil = $derived(new Set(councils.flatMap((c) => c.members.map((m) => m.session_id))));
  const groups = $derived.by(() => {
    const by = new Map<string, LiveRun[]>();
    for (const r of runs) if (!inCouncil.has(r.session_id)) by.set(r.agent_id, [...(by.get(r.agent_id) ?? []), r]);
    return [...by.entries()]
      .map(([agent_id, list]) => {
        const sorted = [...list].sort((a, b) => rank(a) - rank(b) || a.since - b.since);
        return { agent_id, lead: sorted[0], rest: sorted.slice(1) };
      })
      .sort((a, b) => rank(a.lead) - rank(b.lead) || a.lead.since - b.lead.since);
  });
  // The folder of the chat on screen: the strip offers it, so the files are one click away from anywhere in the chat.
  const viewThread = $derived(app.view.kind === 'chat' ? app.threads.find((t) => t.id === (app.view as any).id) : undefined);
  // every chat has files to show: its project's folder, or the specialists' private folders it works in
  const viewFolder = $derived(!!viewThread);
  const busy = $derived(runs.some((r) => r.status === 'running' || r.status === 'queued' || r.awaiting));
  // someone waiting for the person's answer opens the panel even when it was folded: the answer is right there
  const waiting = $derived(runs.some((r) => r.ask || r.question));
  const open = $derived(pinned || waiting || (busy && !dismissed && wide));
  $effect(() => {
    if (!busy) dismissed = false;
  });
  function toggle() {
    if (open) {
      pinned = false;
      savePin(false);
      if (busy) dismissed = true;
    } else if (busy) {
      dismissed = false;
    } else {
      pinned = true;
      savePin(true);
    }
  }
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
  // A permission answered right here: no hunting for the card in a long chat.
  let answering = $state<string | null>(null);
  async function decide(r: LiveRun, allow: boolean, scope?: 'always' | 'dir') {
    if (!r.message_id) return;
    answering = r.session_id;
    try {
      await post(`/api/messages/${r.message_id}/answer`, { decision: allow ? 'allow' : 'deny', ...(allow && scope === 'always' ? { always: true } : {}), ...(allow && scope === 'dir' ? { scope: 'dir' } : {}) });
    } catch {
      /* already answered elsewhere: the next refresh shows it */
    } finally {
      answering = null;
    }
    soon();
  }
  async function reply(r: LiveRun, text: string) {
    if (!r.message_id || !text.trim()) return;
    try {
      await post(`/api/messages/${r.message_id}/question`, { answer: text.trim() });
    } catch {
      /* closed already */
    }
    soon();
  }
  let replies = $state<Record<string, string>>({});
  const dirShort = (p: string) => p.replace(/[\\/]+$/, '').split(/[\\/]/).slice(-2).join('/');

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

<svelte:window onresize={() => (wide = window.innerWidth >= 1280)} />

<aside class="live" class:rail={!open} aria-label="Сейчас работают">
  {#if open}
    <Splitter pane="live" edge="left" label="Ширина панели «Сейчас работают»" />
    <header>
      <b>Сейчас работают</b>
      {#if groups.length}<span class="count" aria-label={`${groups.length} в работе`}>{groups.length}</span>{/if}
      <span class="grow"></span>
      {#if viewFolder}<button class="icon-btn" aria-pressed={app.panel} onclick={() => (app.panel = !app.panel)} aria-label="Файлы и изменения проекта" title="Файлы и изменения проекта"><Icon name="folder" size={15} /></button>{/if}
      <button class="icon-btn" onclick={toggle} aria-label="Свернуть панель" title={busy ? 'Свернуть до конца работы' : 'Свернуть'}><Icon name="right" size={15} /></button>
    </header>
    <div class="list">
      {#each councils as c (c.key)}
        {@const caller = agentById(c.caller)}
        <article class="run council" class:wait={c.members.some((m) => m.awaiting)}>
          <div class="ch">
            <span class="ct"><Icon name="users" size={13} />Заседание совета</span>
            <button class="btn sm" onclick={() => c.members.forEach(stopRun)} aria-label="Остановить всё заседание"><Icon name="stop" size={10} filled />Всех</button>
          </div>
          <button class="where" onclick={() => c.members[0] && openRun(c.members[0])} title={c.title || 'Открыть'}>{c.title || 'Чат'}</button>
          <div class="faint cs">созвал {caller?.name ?? 'специалист'}</div>
          <ul class="members">
            {#each c.members as m (m.session_id)}
              {@const ag = agentById(m.agent_id)}
              <li class:wait={m.awaiting}>
                <Avatar name={ag?.name} color={ag?.accent} size={20} />
                <span class="mn">{ag?.name ?? 'Специалист'}</span>
                <span class="ms">{#if m.status === 'running'}<i class="pulse" aria-hidden="true"></i>{/if}{stateOf(m)} · {dur(m.since)}</span>
                <button class="icon-btn qs" onclick={() => stopRun(m)} aria-label={`Остановить: ${ag?.name ?? 'специалиста'}`} title="Остановить"><Icon name="x" size={12} /></button>
              </li>
            {/each}
          </ul>
        </article>
      {/each}
      {#each groups as g (g.agent_id)}
        {@const ag = agentById(g.agent_id)}
        {@const r = g.lead}
        <article class="run" class:wait={r.awaiting} class:queued={r.status === 'queued'}>
          <div class="who">
            <Avatar name={ag?.name} color={ag?.accent} size={26} />
            <div class="nm"><b>{ag?.name ?? 'Специалист'}</b><span class="st">{#if r.status === 'running'}<i class="pulse" aria-hidden="true"></i>{/if}{stateOf(r)} · {dur(r.since)}</span></div>
          </div>
          {#if r.ask}
            <div class="ask" role="group" aria-label="Запрос разрешения">
              <span class="aq">{r.ask.question || 'Нужно разрешение'}</span>
              {#if r.ask.command}<code class="ac">{r.ask.command}</code>{:else if r.ask.file}<code class="ac">{r.ask.file}</code>{/if}
              <div class="aa">
                <button class="btn sm primary" disabled={answering === r.session_id} onclick={() => decide(r, true)}>Разрешить</button>
                {#if r.ask.dir}<button class="btn sm" disabled={answering === r.session_id} onclick={() => decide(r, true, 'dir')} title={r.ask.dir.label}>В папке {dirShort(r.ask.dir.path)}</button>
                {:else if r.ask.always}<button class="btn sm" disabled={answering === r.session_id} onclick={() => decide(r, true, 'always')} title={r.ask.always}>Всегда</button>{/if}
                <button class="btn sm" disabled={answering === r.session_id} onclick={() => decide(r, false)}>Отклонить</button>
              </div>
            </div>
          {:else if r.question}
            <div class="ask" role="group" aria-label="Вопрос к вам">
              <span class="aq">{r.question.text}</span>
              {#if r.question.options?.length}<div class="aa">{#each r.question.options as o}<button class="btn sm" onclick={() => reply(r, o)}>{o}</button>{/each}</div>{/if}
              <form class="aa" onsubmit={(e) => { e.preventDefault(); reply(r, replies[r.session_id] ?? ''); replies[r.session_id] = ''; }}>
                <input class="field" bind:value={replies[r.session_id]} placeholder="Ответ" aria-label="Ответ специалисту" />
                <button class="btn sm primary" type="submit">Ответить</button>
              </form>
            </div>
          {:else if r.hint}<div class="hint">{r.hint}</div>{/if}
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
          {#if g.rest.length}
            <ul class="queue" aria-label="Дальше у специалиста">
              {#each g.rest as q (q.session_id)}
                <li>
                  <button class="qw" onclick={() => openRun(q)} title={q.title || 'Открыть'}><span class="qt">{q.title || (q.kind === 'task' ? 'Задача' : 'Чат')}</span><span class="faint">{stateOf(q)} · {dur(q.since)}</span></button>
                  <button class="icon-btn qs" onclick={() => stopRun(q)} aria-label={`Убрать из очереди: ${q.title || 'ход'}`} title="Убрать из очереди"><Icon name="x" size={12} /></button>
                </li>
              {/each}
            </ul>
          {/if}
        </article>
      {:else}
        <p class="empty muted">Никто не работает. Панель закреплена: сверните её, и она будет открываться сама, когда кто-то возьмётся за дело.</p>
      {/each}

    </div>
  {:else}
    {#if viewFolder}<button class="rail-folder" aria-pressed={app.panel} onclick={() => (app.panel = !app.panel)} aria-label="Файлы и изменения проекта" title="Файлы и изменения проекта"><Icon name="folder" size={16} /></button>{/if}
    <button class="rail-btn" onclick={toggle} aria-label={busy ? `Сейчас работают: ${groups.length}. Развернуть панель` : 'Никто не работает. Закрепить панель открытой'} title={busy ? 'Сейчас работают' : 'Никто не работает'}>
      <Icon name="bolt" size={16} />
      {#if groups.length}<span class="count">{groups.length}</span>{/if}
      {#each groups.slice(0, 6) as g (g.agent_id)}
        {@const ag = agentById(g.agent_id)}
        <span class="rav" class:on={g.lead.status === 'running'}><Avatar name={ag?.name} color={ag?.accent} size={26} />{#if g.rest.length}<span class="qn">+{g.rest.length}</span>{/if}</span>
      {/each}
    </button>
  {/if}
</aside>

<style>
  .live {
    flex: none;
    width: var(--live-w, 300px);
    position: relative;
    display: flex;
    flex-direction: column;
    min-height: 0;
    border-left: 1px solid var(--line-strong);
    background: var(--bg);
    transition: width 0.22s var(--ease-out, ease);
    overflow: hidden;
  }
  .live.rail { width: 56px; }
  @media (prefers-reduced-motion: reduce) {
    .live { transition: none; }
  }
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

  .council .ch { display: flex; align-items: center; justify-content: space-between; gap: 8px; }
  .ct { display: inline-flex; align-items: center; gap: 6px; font-size: 12.5px; font-weight: 600; }
  .council .where { flex: none; }
  .cs { font-size: 11.5px; margin-top: -4px; }
  .members { list-style: none; margin: 0; padding: 6px 0 0; border-top: 1px solid var(--line); display: flex; flex-direction: column; gap: 4px; }
  .members li { display: flex; align-items: center; gap: 7px; min-width: 0; font-size: 12.5px; }
  .members li.wait .ms { color: var(--warn); }
  .mn { font-weight: 600; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; min-width: 0; }
  .ms { display: inline-flex; align-items: center; gap: 5px; margin-left: auto; color: var(--fg-3); font-size: 11.5px; white-space: nowrap; font-variant-numeric: tabular-nums; }
  .ask { display: flex; flex-direction: column; gap: 6px; min-width: 0; }
  .aq { font-size: 12.5px; line-height: 1.4; overflow-wrap: anywhere; display: -webkit-box; -webkit-line-clamp: 3; line-clamp: 3; -webkit-box-orient: vertical; overflow: hidden; }
  .ac { font-size: 11.5px; padding: 4px 6px; border-radius: var(--radius-s); background: var(--bg-sunken); overflow-wrap: anywhere; max-height: 72px; overflow: auto; }
  .aa { display: flex; flex-wrap: wrap; gap: 6px; }
  .aa .field { flex: 1; min-width: 0; height: 28px; }
  .queue { list-style: none; margin: 0; padding: 6px 0 0; border-top: 1px solid var(--line); display: flex; flex-direction: column; gap: 2px; }
  .queue li { display: flex; align-items: center; gap: 4px; min-width: 0; }
  .qw { flex: 1; min-width: 0; display: flex; align-items: baseline; gap: 6px; padding: 3px 4px; border-radius: var(--radius-s); text-align: left; font-size: 12px; }
  .qw:hover { background: var(--bg-hover); }
  .qt { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .qw .faint { flex: none; font-size: 11px; font-variant-numeric: tabular-nums; }
  .qs { width: 22px; height: 22px; }
  .qn { position: absolute; left: -6px; top: -6px; min-width: 16px; padding: 0 3px; border-radius: 8px; background: var(--bg-raised); box-shadow: 0 0 0 1px var(--line-strong); font-size: 10px; line-height: 16px; text-align: center; color: var(--fg-2); font-variant-numeric: tabular-nums; }
  .rail-folder { display: grid; place-items: center; width: 100%; height: 44px; color: var(--fg-2); border-bottom: 1px solid var(--line); }
  .rail-folder:hover, .rail-folder[aria-pressed='true'] { background: var(--bg-hover); color: var(--fg); }
  .rail-btn { display: flex; flex-direction: column; align-items: center; gap: 12px; width: 100%; padding: 16px 0; color: var(--fg-2, var(--fg)); }
  .rail-btn:hover { background: var(--bg-hover); }
  .rail-btn .count { margin-top: -6px; }
  .rav { position: relative; display: block; line-height: 0; }
  .rav.on::after { content: ''; position: absolute; right: -2px; bottom: -2px; width: 9px; height: 9px; border-radius: 50%; background: var(--accent); box-shadow: 0 0 0 2px var(--bg); animation: livepulse 1.4s ease-in-out infinite; }

  /* narrow windows: the panel would crowd the chat out */
  @media (max-width: 1100px) { .live { display: none; } }
</style>
