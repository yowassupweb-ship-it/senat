<script lang="ts">
  import ContextBar from './ContextBar.svelte';
  import ChatWorkspace from './ChatWorkspace.svelte';
  import NeedsYou from './NeedsYou.svelte';
  import Spinner from './Spinner.svelte';
  import { tick, untrack } from 'svelte';
  import { fileUrl, get, patch, post } from '../lib/api';
  import ForumDialog from './ForumDialog.svelte';
  import { agentById, app, go, loadGit, loadMessages, loadThreadGit, projectById, refreshThreads, threadById, upsertMessage } from '../lib/store.svelte';
  import type { Message } from '../lib/types';
  import { MODE_LABEL } from '../lib/format';
  import Avatar from './Avatar.svelte';
  import ProjectPanel from './ProjectPanel.svelte';
  import Chamber from './Chamber.svelte';
  import Composer, { type Settings, type Todo } from './Composer.svelte';
  import Icon from './Icon.svelte';
  import MessageView from './Message.svelte';
  import UsageRings from './UsageRings.svelte';
  import Outbox from './Outbox.svelte';
  import SqueezeCard from './SqueezeCard.svelte';
  import { enqueue, initOutbox } from '../lib/outbox.svelte';
  import type { Uploaded } from '../lib/api';

  let { threadId, projectId }: { threadId?: string; projectId?: string } = $props();

  const thread = $derived(threadById(threadId));
  const project = $derived(projectById(thread?.project_id ?? projectId));
  const messages = $derived(threadId ? (app.messages[threadId] ?? []) : []);
  // A specialist calling colleagues in is a post with comments: the calls made in a row are one post, and under it —
  // the colleagues' answers (retries included), the notes to them and the caller's reactions, in order. The post
  // stands in the chat where the discussion last moved, so a live one is at the bottom.
  type Post = { key: string; author: string | null; calls: Message[]; comments: Message[]; at: number; decision?: Message };
  // the calls of a post as what was said: one request to several colleagues is one line addressed to all of them
  function asksOf(calls: Message[]): { key: string; to: string[]; text: string }[] {
    const out: { key: string; to: string[]; text: string }[] = [];
    for (const c of calls) {
      const text = c.body.replace(/^→\s*@\S+:\s*/, '').trim();
      const to = (c.props as any)?.call?.to as string;
      const same = out.find((a) => a.text === text);
      if (same) {
        if (!same.to.includes(to)) same.to.push(to);
      } else out.push({ key: c.id, to: [to], text });
    }
    return out;
  }
  type Row = { kind: 'msg'; m: Message; at: number } | { kind: 'squeeze'; ms: Message[]; at: number } | { kind: 'post'; p: Post; at: number };
  type Item = Row | { kind: 'work'; rows: Row[]; at: number };
  const rowKey = (it: Item): string => (it.kind === 'msg' ? it.m.id : it.kind === 'squeeze' ? 's' + it.ms[0].id : it.kind === 'work' ? 'w' + rowKey(it.rows[0]) : 'p' + it.p.key);
  const live = (m: Message) => m.status === 'working' || m.status === 'queued';
  /**
   * What specialists say to each other while they work: notes, a question to one colleague and its answer.
   * The person reads results, not the kitchen, so finished exchanges fold into one thin line.
   * Anything still running, anything that failed, councils and every word addressed to the person stay as they are.
   */
  function isProcess(it: Row): boolean {
    if (it.kind === 'msg') {
      const p = (it.m.props ?? {}) as any;
      return it.m.author_kind === 'agent' && it.m.status === 'done' && (!!p.note || !!p.asked_by);
    }
    if (it.kind === 'post') {
      const members = new Set(it.p.calls.map((c) => (c.props as any)?.call?.to));
      const all = [...it.p.calls, ...it.p.comments];
      return members.size <= 1 && !it.p.decision && it.p.comments.length > 0 && !all.some((m) => live(m) || m.status === 'failed');
    }
    return false;
  }
  function foldWork(items: Row[]): Item[] {
    const out: Item[] = [];
    for (const it of items) {
      const last = out[out.length - 1];
      if (isProcess(it)) {
        if (last?.kind === 'work') {
          last.rows.push(it);
          last.at = it.at;
        } else out.push({ kind: 'work', rows: [it], at: it.at });
      } else out.push(it);
    }
    return out;
  }
  const rowMessages = (r: Row): Message[] => (r.kind === 'msg' ? [r.m] : r.kind === 'squeeze' ? r.ms : [...r.p.calls, ...r.p.comments]);
  const stepsWord = (n: number) => (n % 10 === 1 && n % 100 !== 11 ? 'шаг' : n % 10 >= 2 && n % 10 <= 4 && (n % 100 < 12 || n % 100 > 14) ? 'шага' : 'шагов');
  const hm = (t: number) => new Date(t).toLocaleTimeString('ru-RU', { hour: '2-digit', minute: '2-digit' });
  function workSummary(rows: Row[]) {
    const ms = rows.flatMap(rowMessages).sort((a, b) => a.created_at - b.created_at);
    const ids = new Set<string>();
    for (const m of ms) {
      if (m.author_id) ids.add(m.author_id);
      const p = (m.props ?? {}) as any;
      for (const id of [p.note?.to, p.call?.to]) if (id) ids.add(id);
    }
    const from = ms[0]?.created_at ?? 0;
    const to = ms[ms.length - 1]?.created_at ?? from;
    const lastText = (ms[ms.length - 1]?.body ?? '').replace(/^Записка @\S+:\s*/, '').replace(/^→\s*@\S+:\s*/, '').replace(/\s+/g, ' ');
    return { people: [...ids].map((id) => agentById(id)?.name ?? 'коллега').join(' ↔ '), ids: [...ids], n: ms.length, when: hm(from) === hm(to) ? hm(from) : hm(from) + '–' + hm(to), last: lastText };
  }
  const flow = $derived.by(() => {
    const byId = new Map(messages.map((m) => [m.id, m]));
    const taken = new Set<string>();
    const items: Row[] = [];
    let open: Post | null = null;
    const posts = new Map<string, Post>();
    const P = (m: Message) => (m.props ?? {}) as any;
    for (const m of messages) {
      if (taken.has(m.id)) continue;
      const call = P(m).call;
      if (call) {
        // a call right after the same specialist's call joins their post
        if (!open || open.author !== m.author_id || m.created_at - open.calls[open.calls.length - 1].created_at > 5 * 60_000) {
          open = { key: m.id, author: m.author_id, calls: [], comments: [], at: m.created_at };
          items.push({ kind: 'post', p: open, at: 0 });
          posts.set(open.key, open);
        }
        open.calls.push(m);
        taken.add(m.id);
        // calls made before they remembered their answer: the first later answer from that colleague to this caller
        const guess = call.answer ? undefined : messages.find((x) => !taken.has(x.id) && x.created_at >= m.created_at && x.author_id === call.to && P(x).asked_by === m.author_id)?.id;
        let id: string | undefined = call.answer ?? guess;
        const seen = new Set<string>();
        while (id && byId.has(id) && !seen.has(id)) {
          seen.add(id);
          const a = byId.get(id)!;
          open.comments.push(a);
          taken.add(a.id);
          id = P(a).retried_by;
        }
        continue;
      }
      // the caller reacting to an answer, or a note between the same people, belongs to the open post
      const called = open ? new Set(open.calls.map((c) => P(c).call.to)) : null;
      if (open && called && m.author_id === open.author && ((P(m).reply_from && called.has(P(m).reply_from)) || (P(m).note && called.has(P(m).note.to)))) {
        open.comments.push(m);
        taken.add(m.id);
        continue;
      }
      // the person's word in a council, and the convener's answer to it, stay under that council
      const into = posts.get(P(m).council_note?.post ?? P(m).council_reply ?? '');
      if (into) {
        into.comments.push(m);
        taken.add(m.id);
        continue;
      }
      if (m.author_kind === 'user') open = null;
      // compactions made together are one card; the node's own "Контекст сжат" line is replaced by it
      if (m.author_kind === 'system' && /^Контекст сжат/.test(m.body)) continue;
      if (m.author_kind === 'agent' && P(m).compact) {
        const last = items[items.length - 1];
        if (last?.kind === 'squeeze') last.ms.push(m);
        else items.push({ kind: 'squeeze', ms: [m], at: m.created_at });
        continue;
      }
      items.push({ kind: 'msg', m, at: m.created_at });
    }
    for (const it of items) {
      if (it.kind === 'post') {
        it.p.comments.sort((a, b) => a.created_at - b.created_at);
        // the convener's summing-up of all the answers is the council's decision: shown above the comments
        const d = [...it.p.comments].reverse().find((c) => c.author_id === it.p.author && P(c).council && c.status !== 'failed');
        if (d) {
          it.p.decision = d;
          it.p.comments = it.p.comments.filter((c) => c !== d);
        }
        const all = d ? [...it.p.comments, d] : it.p.comments;
        const last = all.length ? Math.max(...all.map((c) => c.updated_at ?? c.created_at)) : it.p.at;
        it.at = Math.max(it.p.at, last);
      }
    }
    return foldWork(items.sort((a, b) => a.at - b.at));
  });
  const lastAgent = $derived([...messages].reverse().find((m) => m.author_kind === 'agent'));
  const isLive = (m: Message) => m.author_kind === 'agent' && (m.status === 'working' || m.status === 'queued');
  // Anyone still working in this chat (not only the last reply): several specialists can run at once.
  const running = $derived(messages.some(isLive));
  // A reply shown as queued or working is re-read now and then: an event lost on the way (the node restarted, the
  // connection dropped, a run withdrawn before it started) must not leave it "in the queue" on the screen forever.
  $effect(() => {
    const id = threadId;
    if (!id || !running) return;
    const t = setInterval(() => {
      if (document.visibilityState === 'visible') loadMessages(id).catch(() => {});
    }, 20000);
    return () => clearInterval(t);
  });
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
  let workspaceOpen = $state(false);
  const draftGroup = `new-${Date.now()}-${Math.random().toString(36).slice(2)}`;
  $effect(() => { app.nodeId; untrack(initOutbox); });
  let loadingOlder = $state(false);
  async function loadOlder() {
    if (!threadId || loadingOlder || !scroller) return;
    const id = threadId;
    const el = scroller;
    const height = el.scrollHeight;
    const top = el.scrollTop;
    loadingOlder = true;
    stick = false;
    try {
      await loadMessages(id, messages[0]?.id);
      await tick();
      if (threadId === id) el.scrollTop = top + el.scrollHeight - height;
    } catch (e: any) { error = e.message; }
    finally { loadingOlder = false; }
  }

  // Settings of a chat that does not exist yet live here; for existing chats they come from the thread.
  let draftSettings = $state<Settings>({ agent: '', mode: '', effort: '' });
  // Several specialists can be picked before the chat starts: they are all addressed with @mentions.
  // The same choice works in an open chat: the specialist menu in the composer ticks several, and each message
  // then goes to all of them. One ticked is simply that chat's specialist; none ticked leaves the choice to Senate.
  let picked = $state<string[]>(untrack(() => (thread?.agent_id ? [thread.agent_id] : [])));
  function togglePick(id: string) {
    picked = !id ? [] : picked.includes(id) ? picked.filter((x) => x !== id) : [...picked, id];
    changeSettings({ ...settings, agent: picked.length === 1 ? picked[0] : '' });
  }
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
  // height of the floating composer: the message list keeps that much room at its end
  let dockH = $state(0);
  /** Discussions the person folded to one line, by post key. */
  let folded = $state<Record<string, boolean>>({});
  // the answers the convener already folded into the decision: collapsed, the person has read them as they came
  let answersOpen = $state<Record<string, boolean>>({});
  /** An opinion being written for a council, by post key (absent: the form is closed). */
  let speak = $state<Record<string, string>>({});
  let speaking = $state('');
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
  // Reply to a specialist's message: address its author and quote the start of it, above whatever is drafted.
  async function reply(m: Message) {
    const who = app.agents.find((a) => a.id === m.author_id);
    if (!who || !threadId) return;
    const plain = m.body.replace(/```[\s\S]*?```/g, '[код]').replace(/\s+/g, ' ').trim();
    const quote = plain.length > 220 ? plain.slice(0, 220).replace(/\s\S*$/, '') + '…' : plain;
    const head = `@${who.slug}\n> ${quote}\n\n`;
    // a second reply replaces the first quote instead of stacking them
    const draft = (app.drafts[threadId] ?? '').replace(/^@\S+\n> [^\n]*\n\n/, '');
    app.drafts[threadId] = head + draft;
    await tick();
    const ta = document.querySelector<HTMLTextAreaElement>('textarea[aria-label="Сообщение"]');
    if (ta) {
      ta.focus();
      ta.setSelectionRange(ta.value.length, ta.value.length);
      ta.dispatchEvent(new Event('input', { bubbles: true }));
    }
  }

  function send(text: string, attachments: Uploaded[] = []) {
    error = '';
    try {
      if (picked.length > 1) {
        const tags = picked.map((p) => app.agents.find((a) => a.id === p)?.slug).filter((s): s is string => !!s && !text.includes('@' + s));
        if (tags.length) text = tags.map((s) => '@' + s).join(' ') + ' ' + text;
      }
      enqueue({ group: threadId ?? draftGroup, threadId, projectId, agentId: draftSettings.agent || undefined, text, attachments,
        mode: settings.mode || undefined, effort: settings.effort || undefined }, id => {
        if (!threadId && app.view.kind === 'new' && app.view.project === projectId) go({ kind: 'chat', id });
      });
      stick = true;
    } catch (e: any) {
      error = e.message;
      return false;
    }
    return true;
  }

  async function answer(m: Message, allow: boolean, always: boolean | 'dir' = false) {
    try {
      const extra = !allow ? {} : always === 'dir' ? { scope: 'dir' } : always ? { always: true } : {};
      await post(`/api/messages/${m.id}/answer`, { decision: allow ? 'allow' : 'deny', ...extra });
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
  // The same specialist's turn again — not whoever the composer has picked, and without sending the person's
  // message a second time.
  async function retry(m: Message) {
    error = '';
    try {
      const nm = await post(`/api/messages/${m.id}/retry`);
      upsertMessage(nm);
    } catch (e: any) {
      error = e.message;
    }
  }

  // the convener's summing-up in one line: their «Решение: …» line, or the first line of what they said
  function decisionLine(m: Message): string {
    const body = (m.body ?? '').trim();
    if (!body) return m.status === 'working' || m.status === 'queued' ? 'ведущий сводит ответы…' : '';
    const own = body.match(/^\s*(?:\*\*)?Решение(?:\*\*)?\s*[:：]\s*(?:\*\*)?\s*(.+)$/im);
    const line = (own ? own[1] : body.split('\n').find((l) => l.trim()) ?? '').replace(/[*_`#>]/g, '').trim();
    return line.length > 220 ? line.slice(0, 219) + '…' : line;
  }

  // the person's word in a council: to the convener and every member
  async function sendOpinion(p: { key: string; author: string | null }, members: string[]) {
    const text = speak[p.key]?.trim();
    if (!text || !threadId || !p.author) return;
    speaking = p.key;
    error = '';
    try {
      const r = await post<{ message: Message }>(`/api/threads/${threadId}/council-note`, { post: p.key, leader: p.author, members, text });
      upsertMessage(r.message);
      delete speak[p.key];
    } catch (e: any) {
      error = e.message;
    } finally {
      speaking = '';
    }
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

  const git = $derived(threadId ? app.git['t:' + threadId] : undefined);
  const changeCount = $derived(git?.available ? (git.changes?.length ?? 0) : 0);

  // The specialist's current plan: the latest TodoWrite of the latest run that made one. Shown while it
  // is being worked on; once everything is done and nobody is working, it goes away.
  // The git bar follows the folder the chat works in, project or not. It shows once a repository is there and stays
  // (a failed refresh keeps the last state), so it does not come and go between turns.
  const gitChip = $derived(
    git?.available
      ? {
          label: `${git.branch ?? 'git'}${git.ahead ? ` ↑${git.ahead}` : ''}`,
          count: changeCount,
          active: app.panel,
          title: [git.last_commit ? `Последний коммит: ${git.last_commit}` : '', git.ahead ? `Не отправлено в удалённый репозиторий: ${git.ahead}` : git.behind === 0 && git.ahead === 0 ? 'Всё отправлено' : '', changeCount ? `Неотправленных изменений в файлах: ${changeCount}` : ''].filter(Boolean).join(' · '),
        }
      : undefined,
  );

  // Keep it fresh: when the chat opens, when the window comes back, and every so often while a specialist works.
  $effect(() => {
    const id = threadId;
    if (!id) return;
    untrack(() => loadThreadGit(id));
    const tick = setInterval(() => { if (document.visibilityState === 'visible') loadThreadGit(id); }, 15000);
    const focus = () => loadThreadGit(id);
    window.addEventListener('focus', focus);
    return () => {
      clearInterval(tick);
      window.removeEventListener('focus', focus);
    };
  });

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
  <section class="main" style="--dock-h:{dockH}px">
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
      {#if threadId && !isForum}{#key threadId}<ContextBar {threadId} />{/key}{/if}
      <UsageRings />
      {#if isForum && (meta.status === 'open' || meta.status === 'summarizing')}
        <button class="btn sm" onclick={() => forumAct('conclude')}>Подвести итог</button>
        <button class="btn sm" onclick={() => forumAct('stop')}>Остановить</button>
      {:else if thread && thread.kind === 'chat'}
        <button class="icon-btn" onclick={() => (forumDialog = true)} aria-label="Вынести на форум" title="Вынести на форум"><Icon name="forum" size={15} /></button>
      {/if}
      {#if thread}
        <button class="icon-btn" onclick={() => (app.panel = !app.panel)} aria-pressed={app.panel} aria-label="Панель проекта: файлы и изменения" title="Файлы и изменения"><Icon name="panel" size={15} /></button>
      {/if}
      {#if thread}
        <button class="icon-btn" onclick={togglePin} aria-label={thread.pinned ? 'Открепить' : 'Закрепить'} title={thread.pinned ? 'Открепить' : 'Закрепить'}><Icon name="pin" size={15} /></button>
        {#if thread.kind !== 'lounge'}<button class="icon-btn" onclick={archive} aria-label="В архив" title="В архив"><Icon name="archive" size={15} /></button>{/if}
      {/if}
    </header>

    {#if !threadId}
      <div class="empty">
        <Chamber title={greeting} selected={picked} onpick={togglePick} />
      </div>
    {:else}
      <div class="scroll" bind:this={scroller} onscroll={onScroll}>
        <div class="col">
          {#if app.hasMore[threadId]}
            <button class="older" disabled={loadingOlder} onclick={loadOlder}>{loadingOlder ? 'Загружается…' : 'Показать раньше'}</button>
          {/if}
          {#if loading}
            <Spinner size={24} fill />
          {/if}
          {#each flow as it (rowKey(it))}
            {#if it.kind === 'work'}
              {@const w = workSummary(it.rows)}
              <details class="work-fold">
                <summary>
                  <span class="wf-chev" aria-hidden="true"><Icon name="right" size={11} /></span>
                  <span class="wf-faces" aria-hidden="true">{#each w.ids.slice(0, 4) as id (id)}{@const a = agentById(id)}<Avatar name={a?.name} color={a?.accent} size={18} />{/each}</span>
                  <span class="wf-who">{w.people}</span>
                  <span class="wf-n">{w.n} {stepsWord(w.n)}</span>
                  <span class="wf-last">{w.last}</span>
                  <time class="wf-when">{w.when}</time>
                </summary>
                <div class="wf-body">
                  {#each it.rows as sub (rowKey(sub))}{@render row(sub)}{/each}
                </div>
              </details>
            {:else}
              {@render row(it)}
            {/if}
          {/each}
          {#snippet row(it: Row)}
            {#if it.kind === 'squeeze'}
              <SqueezeCard items={it.ms} onstop={stop} />
            {:else if it.kind === 'msg'}
              <MessageView m={it.m} onanswer={answer} onstop={stop} onretry={retry} onimage={(a) => (lightbox = a)} onreply={isForum ? undefined : reply} />
            {:else if it.kind === 'post'}
              {@const who = agentById(it.p.author)}
              <!-- the caller's message to colleagues, as a chat message: the same request to several is said once -->
              {@const asks = asksOf(it.p.calls)}
              {@const members = [...new Set(asks.flatMap((x) => x.to))]}
              {@const called = members.length}
              {@const shut = folded[it.p.key] ?? true}
              {@const done = new Set(it.p.comments.filter((c) => c.props.asked_by && (c.status === 'done' || c.status === 'failed')).map((c) => c.author_id)).size}
              {@const verdict = it.p.decision ? decisionLine(it.p.decision) : ''}
              {@const stateOf = (id: string) => { const c = it.p.comments.find((x) => x.author_id === id && x.props.asked_by); return !c ? 'wait' : c.status === 'failed' ? 'bad' : c.status === 'done' ? 'ok' : 'live'; }}
              {@const stillWorking = !it.p.decision && (done < called || it.p.comments.some((c) => c.status === 'working' || c.status === 'queued'))}
              <section class="post" class:shut aria-label="Заседание совета">
                <button class="ph" onclick={() => (folded[it.p.key] = !shut)} aria-expanded={!shut} title={shut ? 'Развернуть' : 'Свернуть'}>
                  <span class="chev" class:open={!shut}><Icon name="right" size={11} /></span>
                  <Avatar name={who?.name} color={who?.accent} size={36} />
                  <span class="ptitle"><b>{who?.name ?? 'Специалист'}</b> {called > 1 ? 'собрал совет' : 'позвал коллегу'}</span>
                  <span class="faces" aria-hidden="true">
                    {#each members as id (id)}
                      {@const ma = agentById(id)}
                      <span class="face {stateOf(id)}" title={`${ma?.name ?? 'Коллега'}`}><Avatar name={ma?.name} color={ma?.accent} size={34} /></span>
                    {/each}
                  </span>
                  <span class="pstate" class:live={stillWorking}>{#if stillWorking}<span class="loader"></span>ответили {done} из {called}{:else if it.p.decision}<Icon name="check" size={12} />решение готово{:else}ответили {done} из {called}{/if}</span>
                  <time class="faint">{new Date(it.p.calls[0].created_at).toLocaleTimeString('ru-RU', { hour: '2-digit', minute: '2-digit' })}</time>
                </button>
                {#if shut && verdict}
                  <button class="verdict-line" onclick={() => (folded[it.p.key] = false)} title={verdict}><span class="vl">Решение</span>{verdict}</button>
                {/if}
                {#if !shut}
                {#each asks as ask (ask.key)}
                  <p class="say">
                    {#each ask.to as id (id)}{@const to = agentById(id)}<span class="mention">@{to?.name ?? 'коллега'}</span>{' '}{/each}{ask.text}
                  </p>
                {/each}
                {#if it.p.decision}
                  <div class="decision" role="group" aria-label="Решение совета">
                    <div class="dh"><Icon name="check" size={12} />Решение совета{#if it.p.decision.props.missing?.length}<span class="faint"> · не ответили: {it.p.decision.props.missing.join(', ')}</span>{/if}</div>
                    <MessageView m={it.p.decision} nested onanswer={answer} onstop={stop} onretry={retry} onimage={(a) => (lightbox = a)} />
                  </div>
                {/if}
                {#if it.p.comments.length}
                  {@const openA = !!answersOpen[it.p.key]}
                  {@const writing = it.p.comments.filter((c) => c.status === 'working' || c.status === 'queued').length}
                  {@const people = [...new Set(it.p.comments.map((c) => c.author_id))]}
                  <button class="answers-toggle" onclick={() => (answersOpen[it.p.key] = !openA)} aria-expanded={openA}>
                    <span class="chev" class:open={openA}><Icon name="right" size={11} /></span>
                    <span>Ответы участников</span>
                    <span class="count">{it.p.comments.length}</span>
                    <span class="faces" aria-hidden="true">{#each people.slice(0, 5) as id (id)}{@const pa = agentById(id)}<Avatar name={pa?.name} color={pa?.accent} size={16} />{/each}</span>
                    {#if writing}<span class="faint">ещё пишут: {writing}</span>{/if}
                  </button>
                  {#if openA}
                    <div class="comments" role="group" aria-label="Ответы и реплики">
                      {#each it.p.comments as c (c.id)}
                        <MessageView m={c} nested onanswer={answer} onstop={stop} onretry={retry} onimage={(a) => (lightbox = a)} />
                      {/each}
                    </div>
                  {/if}
                {/if}
                {#if speak[it.p.key] !== undefined}
                  <form class="opinion" onsubmit={(e) => { e.preventDefault(); sendOpinion(it.p, members); }}>
                    <textarea rows="2" bind:value={speak[it.p.key]} placeholder="Ваше мнение — уйдёт ведущему и всем участникам" aria-label="Ваше мнение для заседания" onkeydown={(e) => { if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) { e.preventDefault(); sendOpinion(it.p, members); } if (e.key === 'Escape') delete speak[it.p.key]; }}></textarea>
                    <div class="oa"><button type="button" class="btn sm" onclick={() => delete speak[it.p.key]}>Отмена</button><button type="submit" class="btn sm primary" disabled={!speak[it.p.key]?.trim() || speaking === it.p.key}>{speaking === it.p.key ? 'Отправляю…' : 'Отправить'}</button></div>
                  </form>
                {:else}
                  <button class="add-opinion" onclick={() => (speak[it.p.key] = '')}><Icon name="reply" size={12} />Добавить мнение</button>
                {/if}
                {/if}
              </section>
            {/if}
          {/snippet}
          {#if !loading && !messages.length}
            <p class="faint center">Здесь пока пусто. Напишите первое сообщение.</p>
          {/if}
        </div>
      </div>
      {#if !stick}
        <button class="jump" onclick={() => { stick = true; toBottom(true); }} aria-label="К последнему сообщению"><Icon name="down" size={15} /></button>
      {/if}
    {/if}

    <!-- the composer floats over the chat: the messages run on under it to the bottom edge -->
    <div class="dock" bind:clientHeight={dockH}>
    <NeedsYou {threadId} />
    <Outbox {threadId} {projectId} queued={messages.filter(m => m.author_kind === 'agent' && m.status === 'queued')} oncancel={stop}/>
    {#if error}<div class="error" role="alert">{error}<button class="icon-btn" onclick={() => (error = '')} aria-label="Закрыть"><Icon name="x" size={13} /></button></div>{/if}

    {#if workspaceOpen && threadId}<ChatWorkspace {threadId} projectId={thread?.project_id} onclose={() => workspaceOpen = false} />{/if}
    <Composer
      {settings}
      {picked}
      ontoggleagent={togglePick}
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
      onworkspace={threadId && !isForum ? () => workspaceOpen = !workspaceOpen : undefined}
      workspace={project?.root_path ? project.name : undefined}
      todos={planTodos}
      paused={!!meta.plan_hold}
      onpause={pausePlan}
      onresume={resumePlan}
      placeholder={isForum ? 'Добавьте мысль в обсуждение…' : meta.plan_hold ? 'План приостановлен. Что в нём не так?' : undefined}
    />
    </div>
  </section>

  {#if lightbox}
    <div class="lightbox" role="dialog" aria-modal="true" aria-label={lightbox.name} tabindex="-1" onclick={() => (lightbox = null)} onkeydown={(e) => e.key === 'Escape' && (lightbox = null)}>
      <img src={fileUrl(lightbox.url)} alt={lightbox.name} />
      <button class="icon-btn close" onclick={() => (lightbox = null)} aria-label="Закрыть"><Icon name="x" size={18} /></button>
    </div>
  {/if}

  {#if forumDialog}<ForumDialog onclose={() => (forumDialog = false)} topic={[...messages].reverse().find((m) => m.author_kind === 'user')?.body ?? ''} project={project?.id ?? ''} fromThread={threadId ?? ''} />{/if}
  {#if app.panel && threadId}
    <ProjectPanel {threadId} projectId={project?.id ?? ''} />
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
    /* room for the floating composer: the last message ends above it */
    padding-bottom: calc(var(--dock-h, 150px) + 8px);
  }
  .dock {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    z-index: 5;
    pointer-events: none;
  }
  .dock > :global(*) {
    pointer-events: auto;
  }
  /* a discussion: the calls as a post, the answers as comments under it */
  .post {
    margin: 10px 0 14px;
    padding: 8px 16px 12px;
    border: 1px solid var(--line);
    border-radius: 14px;
    background: var(--bg-raised);
  }
  .post .ph { width: 100%; text-align: left; display: flex; align-items: center; gap: 12px; font-size: 14px; min-height: 52px; border-radius: var(--radius-s); }
  .post .chev { display: inline-flex; color: var(--fg-3); transition: transform .15s var(--ease); }
  .post .chev.open { transform: rotate(90deg); }
  .ptitle { color: var(--fg-2); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; min-width: 0; }
  .ptitle b { color: var(--fg); font-weight: 600; }
  .post .faces { display: inline-flex; align-items: center; gap: 10px; margin-left: 6px; }
  .face { position: relative; display: inline-flex; border-radius: 50%; }
  .face.wait { opacity: .45; }
  .face.live::after { content: ''; position: absolute; inset: -2px; border-radius: 50%; border: 2px solid transparent; border-top-color: var(--fg-2); animation: sq-spin .9s linear infinite; }
  .face.ok::after, .face.bad::after { content: ''; position: absolute; right: -3px; bottom: -3px; width: 11px; height: 11px; border-radius: 50%; box-shadow: 0 0 0 2px var(--bg-raised); background: var(--ok); }
  .face.bad::after { background: var(--bad); }
  .pstate { margin-left: auto; display: inline-flex; align-items: center; gap: 6px; color: var(--fg-3); font-size: 12px; white-space: nowrap; font-variant-numeric: tabular-nums; }
  .pstate.live { color: var(--fg-2); }
  .post .loader { width: 10px; height: 10px; border-radius: 50%; border: 1.5px solid var(--line-strong); border-top-color: var(--fg); animation: sq-spin .8s linear infinite; }
  @keyframes sq-spin { to { transform: rotate(360deg); } }
  @media (prefers-reduced-motion: reduce) { .face.live::after, .post .loader { animation-duration: 2.4s; } }
  .ph {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 4px;
  }
.fold {
    width: 22px;
    height: 22px;
    margin-left: -26px;
    display: inline-grid;
    place-items: center;
    border-radius: 6px;
    color: var(--fg-3);
  }
  .fold:hover {
    background: var(--bg-hover);
    color: var(--fg);
  }
  .fold:focus-visible {
    outline: 2px solid var(--focus, var(--fg-2));
    outline-offset: 1px;
  }
  .sum {
    font-size: 12px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    min-width: 0;
  }
  .ph b {
    font-weight: 600;
  }
  .ph time {
    margin-left: auto;
    font-size: 12px;
  }
  .gather {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 0 7px;
    height: 20px;
    border-radius: 10px;
    background: var(--bg-hover);
    color: var(--fg-2);
    font-size: 11.5px;
    font-weight: 500;
  }
  .say {
    margin: 0 0 6px 30px;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .mention {
    font-weight: 600;
    color: var(--fg);
  }
  .decision {
    margin: 6px 0 6px 11px;
    padding: 6px 10px 2px 16px;
    border-left: 2px solid var(--fg-3);
    background: var(--bg-hover);
    border-radius: 0 var(--radius) var(--radius) 0;
  }
  .dh {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    font-weight: 600;
    color: var(--fg-2);
  }
  .answers-toggle { display: flex; align-items: center; gap: 6px; margin: 4px 0 6px 30px; min-height: 24px; color: var(--fg-2); font-size: 12px; font-weight: 600; border-radius: var(--radius-s); }
  .answers-toggle:hover { color: var(--fg); }
  .answers-toggle .chev { display: inline-flex; color: var(--fg-3); transition: transform .15s var(--ease); }
  .answers-toggle .chev.open { transform: rotate(90deg); }
  .answers-toggle .count { min-width: 18px; padding: 0 6px; border-radius: 9px; background: var(--bg-active); font-size: 11px; line-height: 18px; text-align: center; font-variant-numeric: tabular-nums; }
  .answers-toggle .faces { display: inline-flex; margin-left: 2px; }
  .answers-toggle .faces > :global(*) + :global(*) { margin-left: -5px; }
  .answers-toggle .faint { font-weight: 400; }
  .verdict-line {
    display: block;
    width: 100%;
    margin: 2px 0 2px;
    text-align: left;
    font-size: 13px;
    color: var(--fg);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .verdict-line .vl {
    font-weight: 600;
    color: var(--fg-2);
    margin-right: 6px;
  }
  .add-opinion {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    margin: 2px 0 0 30px;
    padding: 2px 8px;
    border-radius: 6px;
    font-size: 12px;
    color: var(--fg-3);
  }
  .add-opinion:hover {
    background: var(--bg-hover);
    color: var(--fg);
  }
  .add-opinion:focus-visible,
  .verdict-line:focus-visible {
    outline: 2px solid var(--focus, var(--fg-2));
    outline-offset: 1px;
  }
  .opinion {
    margin: 4px 0 0 30px;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .opinion textarea {
    width: 100%;
    resize: vertical;
    min-height: 48px;
    padding: 6px 8px;
    border-radius: var(--radius);
    border: 1px solid var(--line-strong);
    background: var(--bg-raised);
    color: var(--fg);
    font: inherit;
    font-size: 13px;
  }
  .oa {
    display: flex;
    justify-content: flex-end;
    gap: 6px;
  }
  .comments {
    margin: 6px 0 4px 11px;
    padding-left: 16px;
    border-left: 2px solid var(--line);
    display: flex;
    flex-direction: column;
    gap: 2px;
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
  .work-fold {
    margin: 0 0 6px;
    border-radius: 8px;
  }
  .work-fold summary {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 28px;
    padding: 2px 8px;
    border-radius: 8px;
    font-size: 12px;
    color: var(--fg-3);
    cursor: pointer;
    min-width: 0;
    list-style: none;
  }
  .work-fold summary::-webkit-details-marker {
    display: none;
  }
  .work-fold summary:hover {
    background: var(--accent-soft);
    color: var(--fg-2);
  }
  .work-fold summary:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }
  .wf-chev {
    display: inline-flex;
    transition: transform 0.15s var(--ease);
  }
  .work-fold[open] .wf-chev {
    transform: rotate(90deg);
  }
  .wf-faces {
    display: inline-flex;
    gap: 2px;
    flex: none;
  }
  .wf-who {
    color: var(--fg-2);
    font-weight: 500;
    white-space: nowrap;
  }
  .wf-n,
  .wf-when {
    white-space: nowrap;
    font-variant-numeric: tabular-nums;
    flex: none;
  }
  .wf-last {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .work-fold[open] .wf-last {
    visibility: hidden;
  }
  .wf-body {
    margin: 6px 0 10px 12px;
    padding-left: 12px;
    border-left: 2px solid var(--line);
  }
  @media (prefers-reduced-motion: reduce) {
    .wf-chev {
      transition: none;
    }
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
    bottom: calc(var(--dock-h, 150px) + 12px);
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
    scrollbar-gutter: stable;
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
  /* A toolbar is one row: the title gives way first, then the chips; the buttons never drop to a second line. */
  @media (min-width: 761px) {
    header { flex-wrap: nowrap; }
    header > :global(*) { flex: none; }
    header > .title { flex: 0 1 auto; min-width: 72px; max-width: none; }
    header > :global(.chip) { flex: 0 2 auto; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    header > :global(.grow) { flex: 1 1 0; min-width: 0; }
  }
  @media (max-width: 760px) { header { padding: 14px 12px 14px 52px; gap: 6px; } .empty { padding: 24px 16px; } .scroll { padding: 0 16px; } }
</style>
