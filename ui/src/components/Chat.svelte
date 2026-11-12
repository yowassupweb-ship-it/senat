<script lang="ts">
  import { tick, untrack } from 'svelte';
  import { fileUrl, get, patch, post } from '../lib/api';
  import ForumDialog from './ForumDialog.svelte';
  import { agentById, app, go, loadGit, loadMessages, projectById, refreshThreads, threadById, upsertMessage } from '../lib/store.svelte';
  import type { Message } from '../lib/types';
  import { MODE_LABEL } from '../lib/format';
  import Avatar from './Avatar.svelte';
  import ChangesPanel from './ChangesPanel.svelte';
  import Chamber from './Chamber.svelte';
  import Composer, { type Settings, type Todo } from './Composer.svelte';
  import Icon from './Icon.svelte';
  import MessageView from './Message.svelte';

  let { threadId, projectId }: { threadId?: string; projectId?: string } = $props();

  const thread = $derived(threadById(threadId));
  const project = $derived(projectById(thread?.project_id ?? projectId));
  const messages = $derived(threadId ? (app.messages[threadId] ?? []) : []);
  const lastAgent = $derived([...messages].reverse().find((m) => m.author_kind === 'agent'));
  const isLive = (m: Message) => m.author_kind === 'agent' && (m.status === 'working' || m.status === 'queued');
  // Anyone still working in this chat (not only the last reply): several specialists can run at once.
  const running = $derived(messages.some(isLive));
  let loading = $state(false);
  // a picture from a message, shown large
  let lightbox = $state<{ url: string; name: string } | null>(null);
  let forumDialog = $state(false);
  const isForum = $derived(thread?.kind === 'forum');
  const meta = $derived(thread?.meta ?? {});
  const forumStatus = $derived(({ open: 'идёт обсуждение', summarizing: 'подводит итог', concluded: 'завершён', stopped: 'остановлен' } as Record<string, string>)[meta.status ?? ''] ?? '');
  async function forumAct(what: 'stop' | 'conclude') {
    await post(`/api/forums/${threadId}/${what}`).catch((e) => (error = e.message));
  }
  let error = $state('');

  // Settings of a chat that does not exist yet live here; for existing chats they come from the thread.
  let draftSettings = $state<Settings>({ agent: '', mode: '', effort: '' });
  const settings = $derived<Settings>(thread ? { agent: thread.agent_id ?? '', mode: thread.mode, effort: thread.effort } : draftSettings);

  async function changeSettings(s: Settings) {
    if (!thread) {
      draftSettings = s;
      return;
    }
    try {
      await patch(`/api/threads/${thread.id}`, { agent_id: s.agent, mode: s.mode, effort: s.effort });
      const i = app.threads.findIndex((t) => t.id === thread.id);
      if (i >= 0) app.threads[i] = { ...app.threads[i], agent_id: s.agent || null, mode: s.mode, effort: s.effort };
    } catch (e: any) {
      error = e.message;
    }
  }

  // Load messages when the chat changes — and only then. This effect must depend on `threadId` alone: the list of
  // chats is replaced on every event while a specialist works, and anything read from it here would reload the
  // messages and throw the page to the bottom each time, under the reader's hands.
  $effect(() => {
    const id = threadId;
    if (!id) return;
    untrack(() => {
      if (!app.messages[id]) loading = true;
      loadMessages(id)
        .catch((e) => (error = e.message))
        .finally(async () => {
          loading = false;
          await tick();
          stick = true;
          toBottom(true);
        });
    });
  });

  // The project's git state: a plain value, so it is refetched when the project changes, not on every refresh.
  const threadProject = $derived(thread?.project_id ?? null);
  $effect(() => {
    const pid = threadProject;
    if (pid) untrack(() => loadGit(pid));
  });

  // ---------- scrolling ----------
  let scroller: HTMLElement | undefined = $state();
  let stick = $state(true);
  // Following the output is the reader's choice, made by scrolling: up means "I am reading, leave me alone",
  // all the way down means "follow again". New content growing under them changes neither (it does not move
  // the scroll position), so a long answer arriving never makes it let go, and never snaps it back.
  let lastTop = 0;
  function onScroll() {
    if (!scroller) return;
    const top = scroller.scrollTop;
    const gap = scroller.scrollHeight - top - scroller.clientHeight;
    if (gap < 24) stick = true;
    else if (top < lastTop) stick = false;
    lastTop = top;
  }
  function toBottom(force = false) {
    if (!scroller) return;
    if (force || stick) scroller.scrollTop = scroller.scrollHeight;
  }
  $effect(() => {
    // follow streaming output while the user is at the bottom
    messages.length;
    lastAgent?.body;
    lastAgent?.props?.steps?.length;
    lastAgent?.props?.ask?.state;
    tick().then(() => toBottom());
  });

  // ---------- actions ----------
  async function send(text: string, attachments: { id?: string }[] = []) {
    error = '';
    try {
      let id = threadId;
      if (!id) {
        const t = await post('/api/threads', { project_id: projectId ?? null, agent_id: draftSettings.agent || null });
        id = t.id as string;
        app.messages[id] = [];
        app.threads.unshift({ ...t, last_text: '', working: true });
      }
      const d = await post(`/api/threads/${id}/messages`, {
        text,
        // files go by id only: the node knows where it stored them
        attachments: attachments.filter((a) => a.id).map((a) => ({ id: a.id })),
        mode: settings.mode || undefined,
        effort: settings.effort || undefined,
      });
      upsertMessage(d.user_message);
      d.agent_messages.forEach(upsertMessage);
      if (!threadId) {
        // carry a pre-chat choice into the chat
        go({ kind: 'chat', id });
      } else stick = true;
    } catch (e: any) {
      error = e.message;
      return false;
    }
    return true;
  }

  async function answer(m: Message, allow: boolean, always = false) {
    try {
      await post(`/api/messages/${m.id}/answer`, { decision: allow ? 'allow' : 'deny', ...(allow && always ? { always: true } : {}) });
    } catch (e: any) {
      error = e.message;
    }
  }
  async function stop(m?: Message) {
    // The composer's stop button halts everyone working here; a message's own button halts just that one.
    const targets = m ? [m] : messages.filter(isLive);
    if (!targets.length) return;
    try {
      await Promise.all(targets.map((t) => post(`/api/messages/${t.id}/cancel`)));
    } catch (e: any) {
      error = e.message;
    }
  }
  // "I do not agree with this plan": stop everyone, keep the plan, wait for the person's objection.
  async function pausePlan() {
    if (!threadId) return;
    error = '';
    try {
      await post(`/api/threads/${threadId}/plan/pause`, { todos: planTodos });
      refreshThreads();
    } catch (e: any) {
      error = e.message;
    }
  }
  // Changed their mind: the plan goes on as it was.
  async function resumePlan() {
    if (!threadId) return;
    error = '';
    try {
      await post(`/api/threads/${threadId}/plan/resume`);
      await send('Продолжай по прежнему плану.');
    } catch (e: any) {
      error = e.message;
    }
  }
  function retry(m: Message) {
    const i = messages.findIndex((x) => x.id === m.id);
    const prev = [...messages.slice(0, i)].reverse().find((x) => x.author_kind === 'user');
    if (prev) send(prev.body, prev.props?.attachments ?? []);
  }

  async function rename() {
    if (!thread) return;
    const t = prompt('Название чата', thread.title);
    if (t === null) return;
    await patch(`/api/threads/${thread.id}`, { title: t }).catch(() => {});
    const i = app.threads.findIndex((x) => x.id === thread.id);
    if (i >= 0) app.threads[i] = { ...app.threads[i], title: t };
  }
  async function togglePin() {
    if (!thread) return;
    await patch(`/api/threads/${thread.id}`, { pinned: !thread.pinned });
    const i = app.threads.findIndex((x) => x.id === thread.id);
    if (i >= 0) app.threads[i] = { ...app.threads[i], pinned: !thread.pinned };
  }
  async function archive() {
    if (!thread) return;
    await patch(`/api/threads/${thread.id}`, { archived: true });
    app.threads = app.threads.filter((x) => x.id !== thread.id);
    go({ kind: 'new', project: thread.project_id ?? undefined });
  }

  const git = $derived(project ? app.git[project.id] : undefined);
  const changeCount = $derived(git?.available ? (git.changes?.length ?? 0) : 0);

  // The specialist's current plan: the latest TodoWrite of the latest run that made one. Shown while it
  // is being worked on; once everything is done and nobody is working, it goes away.
  const gitChip = $derived(project?.root_path ? { label: git?.available ? (git.branch ?? 'git') : 'нет git', count: changeCount, active: app.panel } : undefined);

  let planSig = '';
  let planKept: Todo[] = [];
  const planTodos = $derived.by((): Todo[] => {
    let found: Todo[] = [];
    outer: for (let i = messages.length - 1; i >= 0; i--) {
      const m = messages[i];
      if (m.author_kind !== 'agent') continue;
      const steps = m.props?.steps ?? [];
      for (let j = steps.length - 1; j >= 0; j--) {
        const list = steps[j].name === 'TodoWrite' ? steps[j].input?.todos : null;
        if (Array.isArray(list) && list.length) {
          const todos = list.filter((t: any) => t && typeof t.content === 'string') as Todo[];
          found = running || todos.some((t) => t.status !== 'completed') ? todos : [];
          break outer;
        }
      }
    }
    // Streaming rebuilds the steps on every token. Hand the panel the same array until the plan itself
    // changes, so it is not re-rendered (and does not flicker) while the specialist is writing.
    const sig = JSON.stringify(found.map((t) => [t.content, t.status, t.activeForm ?? '']));
    if (sig !== planSig) {
      planSig = sig;
      planKept = found;
    }
    return planKept;
  });
  const effMode = $derived(settings.mode || project?.mode || 'edit');
  const greeting = $derived(project ? `Над чем работаем в «${project.name}»?` : 'Над чем работаем?');
</script>

<div class="wrap">
  <section class="main">
    <header>
      {#if thread}
        <button class="title" onclick={rename} title="Переименовать">{thread.title || 'Новый чат'}</button>
      {:else}
        <span class="title static">Новый чат</span>
      {/if}
      {#if isForum}
        <span class="chip accent"><Icon name="forum" size={12} />Форум{#if meta.round} · раунд {meta.round}/{meta.rounds}{/if}</span>
        {#if forumStatus}<span class="chip {meta.status === 'open' || meta.status === 'summarizing' ? 'warn' : ''}">{forumStatus}</span>{/if}
      {/if}
      {#if project}
        <button class="chip proj" onclick={() => go({ kind: 'projects', id: project.id })}><Icon name="folder" size={12} />{project.name}</button>
      {/if}
      {#if !isForum}<span class="chip" title={MODE_LABEL[effMode]?.hint}><Icon name="shield" size={12} />{MODE_LABEL[effMode]?.label}</span>{/if}
      <span class="grow"></span>
      {#if isForum && (meta.status === 'open' || meta.status === 'summarizing')}
        <button class="btn sm" onclick={() => forumAct('conclude')}>Подвести итог</button>
        <button class="btn sm" onclick={() => forumAct('stop')}>Остановить</button>
      {:else if thread && thread.kind === 'chat'}
        <button class="icon-btn" onclick={() => (forumDialog = true)} aria-label="Вынести на форум" title="Вынести на форум"><Icon name="forum" size={15} /></button>
      {/if}
      {#if thread}
        <button class="icon-btn" onclick={togglePin} aria-label={thread.pinned ? 'Открепить' : 'Закрепить'} title={thread.pinned ? 'Открепить' : 'Закрепить'}><Icon name="pin" size={15} /></button>
        {#if thread.kind !== 'lounge'}<button class="icon-btn" onclick={archive} aria-label="В архив" title="В архив"><Icon name="archive" size={15} /></button>{/if}
      {/if}
    </header>

    {#if !threadId}
      <div class="empty">
        <Chamber title={greeting} selected={draftSettings.agent} onpick={(id) => (draftSettings = { ...draftSettings, agent: id })} />
      </div>
    {:else}
      <div class="scroll" bind:this={scroller} onscroll={onScroll}>
        <div class="col">
          {#if app.hasMore[threadId]}
            <button class="older" onclick={() => loadMessages(threadId!, messages[0]?.id)}>Показать раньше</button>
          {/if}
          {#if loading}
            <p class="faint center">Загрузка…</p>
          {/if}
          {#each messages as m (m.id)}
            <MessageView {m} onanswer={answer} onstop={stop} onretry={retry} onimage={(a) => (lightbox = a)} />
          {/each}
          {#if !loading && !messages.length}
            <p class="faint center">Здесь пока пусто. Напишите первое сообщение.</p>
          {/if}
        </div>
      </div>
      {#if !stick}
        <button class="jump" onclick={() => { stick = true; toBottom(true); }} aria-label="К последнему сообщению"><Icon name="down" size={15} /></button>
      {/if}
    {/if}

    {#if error}<div class="error" role="alert">{error}<button class="icon-btn" onclick={() => (error = '')} aria-label="Закрыть"><Icon name="x" size={13} /></button></div>{/if}

    <Composer
      {settings}
      onchange={changeSettings}
      onsend={send}
      onstop={() => stop()}
      {running}
      draftKey={threadId ?? `new:${projectId ?? ''}`}
      projectMode={project?.mode ?? 'edit'}
      compact={isForum}
      preview={meta.preview}
      {threadId}
      git={gitChip}
      ongit={() => (app.panel = !app.panel)}
      todos={planTodos}
      paused={!!meta.plan_hold}
      onpause={pausePlan}
      onresume={resumePlan}
      placeholder={isForum ? 'Добавьте мысль в обсуждение…' : meta.plan_hold ? 'План приостановлен. Что в нём не так?' : undefined}
    />
  </section>

  {#if lightbox}
    <div class="lightbox" role="dialog" aria-modal="true" aria-label={lightbox.name} tabindex="-1" onclick={() => (lightbox = null)} onkeydown={(e) => e.key === 'Escape' && (lightbox = null)}>
      <img src={fileUrl(lightbox.url)} alt={lightbox.name} />
      <button class="icon-btn close" onclick={() => (lightbox = null)} aria-label="Закрыть"><Icon name="x" size={18} /></button>
    </div>
  {/if}

  {#if forumDialog}<ForumDialog onclose={() => (forumDialog = false)} topic={[...messages].reverse().find((m) => m.author_kind === 'user')?.body ?? ''} project={project?.id ?? ''} fromThread={threadId ?? ''} />{/if}
  {#if app.panel && project?.root_path}
    <ChangesPanel projectId={project.id} />
  {/if}
</div>

<style>
  .wrap {
    display: flex;
    height: 100%;
    min-width: 0;
  }
  .main {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    position: relative;
  }
  header {
    height: 46px;
    flex: none;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 16px;
    -webkit-app-region: drag;
  }
  header > * {
    -webkit-app-region: no-drag;
  }
  .title {
    font-weight: 600;
    font-size: 14.5px;
    max-width: 38vw;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    padding: 3px 6px;
    margin-left: -6px;
    border-radius: var(--radius-s);
  }
  .title:not(.static):hover {
    background: var(--bg-hover);
  }
  .grow {
    flex: 1;
  }
  .lightbox { position: fixed; inset: 0; z-index: 80; display: grid; place-items: center; padding: 32px; background: var(--scrim, rgba(0, 0, 0, 0.7)); backdrop-filter: blur(6px); cursor: zoom-out; }
  .lightbox img { max-width: 100%; max-height: 100%; object-fit: contain; border-radius: var(--radius); box-shadow: var(--shadow-2); }
  .lightbox .close { position: absolute; top: 16px; right: 16px; color: #fff; }
  .chip.proj {
    cursor: pointer;
  }
  .chip.proj:hover {
    background: var(--bg-active);
    color: var(--fg);
  }
  .scroll {
    flex: 1;
    overflow-y: auto;
    padding: 0 20px;
  }
  .col {
    max-width: 820px;
    margin: 0 auto;
    padding: 6px 0 26px;
  }
  .center {
    text-align: center;
    padding: 30px 0;
  }
  .older {
    display: block;
    margin: 0 auto 8px;
    color: var(--accent);
    font-size: 13px;
  }
  .jump {
    position: absolute;
    right: 28px;
    bottom: 150px;
    width: 32px;
    height: 32px;
    border-radius: 50%;
    background: var(--bg-raised);
    box-shadow: var(--shadow-2);
    display: grid;
    place-items: center;
    z-index: 5;
  }
  .error {
    margin: 0 auto 8px;
    max-width: 820px;
    width: calc(100% - 40px);
    background: var(--bad-soft);
    color: var(--bad);
    border-radius: var(--radius);
    padding: 7px 12px;
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .empty {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 6px;
    padding: 20px 20px 28px;
    overflow-y: auto;
    text-align: center;
  }
  header { min-height: 62px; height: auto; padding: 12px 24px; border-bottom: 1px solid var(--line); background: var(--toolbar); flex-wrap: wrap; }
  @media (max-width: 760px) { header { padding: 14px 12px 14px 52px; gap: 6px; } .empty { padding: 24px 16px; } .scroll { padding: 0 16px; } }
</style>
