<script lang="ts">
  import { tick, untrack } from 'svelte';
  import { get, patch, post } from '../lib/api';
  import ForumDialog from './ForumDialog.svelte';
  import { agentById, app, go, loadGit, loadMessages, projectById, threadById, upsertMessage } from '../lib/store.svelte';
  import type { Message } from '../lib/types';
  import { MODE_LABEL } from '../lib/format';
  import Avatar from './Avatar.svelte';
  import ChangesPanel from './ChangesPanel.svelte';
  import Composer, { type Settings } from './Composer.svelte';
  import Icon from './Icon.svelte';
  import MessageView from './Message.svelte';

  let { threadId, projectId }: { threadId?: string; projectId?: string } = $props();

  const thread = $derived(threadById(threadId));
  const project = $derived(projectById(thread?.project_id ?? projectId));
  const messages = $derived(threadId ? (app.messages[threadId] ?? []) : []);
  const lastAgent = $derived([...messages].reverse().find((m) => m.author_kind === 'agent'));
  const running = $derived(!!lastAgent && (lastAgent.status === 'working' || lastAgent.status === 'queued'));
  let loading = $state(false);
  let forumDialog = $state(false);
  const isForum = $derived(thread?.kind === 'forum');
  const meta = $derived(thread?.meta ?? {});
  const forumStatus = $derived(({ open: 'идёт обсуждение', summarizing: 'подводит итог', concluded: 'завершён', stopped: 'остановлен' } as Record<string, string>)[meta.status ?? ''] ?? '');
  async function forumAct(what: 'stop' | 'conclude') {
    await post(`/api/forums/${threadId}/${what}`).catch((e) => (error = e.message));
  }
  let error = $state('');

  // Settings of a chat that does not exist yet live here; for existing chats they come from the thread.
  let draftSettings = $state<Settings>({ agent: '', model: '', mode: '', effort: '' });
  const settings = $derived<Settings>(thread ? { agent: thread.agent_id ?? '', model: thread.model, mode: thread.mode, effort: thread.effort } : draftSettings);

  async function changeSettings(s: Settings) {
    if (!thread) {
      draftSettings = s;
      return;
    }
    try {
      await patch(`/api/threads/${thread.id}`, { agent_id: s.agent, model: s.model, mode: s.mode, effort: s.effort });
      const i = app.threads.findIndex((t) => t.id === thread.id);
      if (i >= 0) app.threads[i] = { ...app.threads[i], agent_id: s.agent || null, model: s.model, mode: s.mode, effort: s.effort };
    } catch (e: any) {
      error = e.message;
    }
  }

  // load messages when the chat changes
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
          toBottom(true);
        });
    });
    const pid = thread?.project_id;
    if (pid) untrack(() => loadGit(pid));
  });

  // ---------- scrolling ----------
  let scroller: HTMLElement | undefined = $state();
  let stick = $state(true);
  function onScroll() {
    if (!scroller) return;
    stick = scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight < 80;
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
  async function send(text: string) {
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
        model: settings.model || undefined,
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
    }
  }

  async function answer(m: Message, allow: boolean) {
    try {
      await post(`/api/messages/${m.id}/answer`, { decision: allow ? 'allow' : 'deny' });
    } catch (e: any) {
      error = e.message;
    }
  }
  async function stop(m?: Message) {
    const target = m ?? lastAgent;
    if (!target) return;
    try {
      await post(`/api/messages/${target.id}/cancel`);
    } catch (e: any) {
      error = e.message;
    }
  }
  function retry(m: Message) {
    const i = messages.findIndex((x) => x.id === m.id);
    const prev = [...messages.slice(0, i)].reverse().find((x) => x.author_kind === 'user');
    if (prev) send(prev.body);
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
      {#if project?.root_path}
        <button class="chip git" class:on={app.panel} onclick={() => (app.panel = !app.panel)} title="Изменения в папке проекта">
          <Icon name="branch" size={12} />{git?.available ? git.branch : 'нет git'}
          {#if changeCount}<b>{changeCount}</b>{/if}
        </button>
      {/if}
      {#if thread}
        <button class="icon-btn" onclick={togglePin} aria-label={thread.pinned ? 'Открепить' : 'Закрепить'} title={thread.pinned ? 'Открепить' : 'Закрепить'}><Icon name="pin" size={15} /></button>
        {#if thread.kind !== 'lounge'}<button class="icon-btn" onclick={archive} aria-label="В архив" title="В архив"><Icon name="archive" size={15} /></button>{/if}
      {/if}
    </header>

    {#if !threadId}
      <div class="empty">
        <div class="mark"><Icon name="sparkle" size={22} /></div>
        <h1>{greeting}</h1>
        <div class="who">
          {#each app.agents.slice(0, 6) as a (a.id)}
            <button class="who-item" class:on={draftSettings.agent === a.id} onclick={() => (draftSettings = { ...draftSettings, agent: draftSettings.agent === a.id ? '' : a.id })}>
              <Avatar name={a.name} color={a.accent} size={22} />{a.name}
            </button>
          {/each}
        </div>
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
            <MessageView {m} onanswer={answer} onstop={stop} onretry={retry} />
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
      placeholder={isForum ? 'Добавьте мысль в обсуждение…' : undefined}
    />
  </section>

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
  .chip.proj,
  .chip.git {
    cursor: pointer;
  }
  .chip.proj:hover,
  .chip.git:hover,
  .chip.git.on {
    background: var(--bg-active);
    color: var(--fg);
  }
  .chip.git b {
    font-weight: 600;
    color: var(--accent);
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
    padding: 20px 20px 60px;
    text-align: center;
  }
  .mark {
    width: 46px;
    height: 46px;
    border-radius: 14px;
    display: grid;
    place-items: center;
    background: var(--accent-soft);
    color: var(--accent);
    margin-bottom: 8px;
  }
  h1 {
    margin: 0;
    font-size: 24px;
    font-weight: 650;
    letter-spacing: -0.01em;
  }
  .empty p {
    margin: 0;
    max-width: 460px;
  }
  .who {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
    justify-content: center;
    margin-top: 16px;
  }
  .who-item {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 5px 12px 5px 6px;
    border-radius: 20px;
    background: var(--bg-sunken);
    box-shadow: inset 0 0 0 1px var(--line);
  }
  .who-item:hover {
    background: var(--bg-hover);
  }
  .who-item.on {
    box-shadow: inset 0 0 0 1.5px var(--accent);
    background: var(--accent-soft);
  }
</style>
