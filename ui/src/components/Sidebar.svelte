<script lang="ts">
  import { app, attentionCount, go, type View } from '../lib/store.svelte';
  import { patch } from '../lib/api';
  import { ctx, type MenuItem } from '../lib/contextmenu.svelte';
  import { timeAgo } from '../lib/format';
  import type { Thread } from '../lib/types';
  import Icon from './Icon.svelte';
  import { focusScope } from '../lib/focus';

  let { onforum }: { onforum: () => void } = $props();

  let q = $state('');
  let navigation: HTMLElement;
  $effect(() => {
    if (app.sidebar && navigation) return focusScope(navigation, () => (app.sidebar = false)).destroy;
  });

  const match = (t: Thread) => !q || (t.title || 'Новый чат').toLowerCase().includes(q.toLowerCase());
  const lounge = $derived(app.threads.find((t) => t.kind === 'lounge'));
  const forums = $derived(app.threads.filter((t) => t.kind === 'forum' && match(t)));
  const chats = $derived(app.threads.filter((t) => t.kind === 'chat' && match(t)));

  // Chats are kept by company: a chat belongs to the company it was started in, or to the company of its project.
  const companyOf = (t: Thread) => t.company_id ?? app.projects.find((p) => p.id === t.project_id)?.company_id ?? null;
  const byCompany = $derived.by(() => {
    const known = new Set(app.companies.map((c) => c.id));
    const map = new Map<string, Thread[]>();
    const loose: Thread[] = [];
    for (const t of chats) {
      const c = companyOf(t);
      if (c && known.has(c)) map.set(c, [...(map.get(c) ?? []), t]);
      else loose.push(t);
    }
    return { groups: app.companies.filter((c) => map.has(c.id)).map((c) => ({ id: c.id, name: c.name, items: map.get(c.id)! })), loose };
  });
  const looseChats = $derived(byCompany.loose);

  // A folded company stays folded between launches; a search opens everything it finds.
  let folded = $state<string[]>((() => {
    try { return JSON.parse(localStorage.getItem('senate.side.folded') || '[]'); } catch { return []; }
  })());
  const isFolded = (id: string) => !q && folded.includes(id);
  function fold(id: string) {
    folded = folded.includes(id) ? folded.filter((x) => x !== id) : [...folded, id];
    try { localStorage.setItem('senate.side.folded', JSON.stringify(folded)); } catch { /* storage unavailable */ }
  }

  let renaming = $state<string | null>(null);
  let draft = $state('');
  function focusInput(n: HTMLInputElement) {
    n.focus();
    n.select();
  }
  function startRename(t: Thread) {
    renaming = t.id;
    draft = t.title || '';
  }
  async function commitRename(t: Thread) {
    if (renaming !== t.id) return;
    const v = draft.trim();
    renaming = null;
    if (v && v !== t.title) await patch(`/api/threads/${t.id}`, { title: v });
  }
  const threadMenu = (t: Thread): MenuItem[] => [
    { label: 'Открыть', icon: 'chat', run: () => go({ kind: 'chat', id: t.id }) },
    { label: 'Переименовать', icon: 'pencil', run: () => startRename(t) },
    { label: t.pinned ? 'Открепить' : 'Закрепить', icon: 'pin', run: () => patch(`/api/threads/${t.id}`, { pinned: !t.pinned }) },
    { sep: true },
    { label: 'Копировать название', icon: 'copy', run: () => navigator.clipboard?.writeText(t.title || '') },
    { sep: true },
    {
      label: 'Убрать в архив',
      icon: 'archive',
      danger: true,
      run: async () => {
        await patch(`/api/threads/${t.id}`, { archived: true });
        if (isChat(t.id)) go({ kind: 'new' });
      },
    },
  ];

  const cur = $derived(app.view);
  const isChat = (id: string) => cur.kind === 'chat' && cur.id === id;
  const chrome = !!(window as any).__SENATE__?.chrome;
  const isView = (k: View['kind']) => cur.kind === k;
  const badge = $derived(attentionCount());

  const nav: { kind: View['kind']; label: string; icon: string; img?: string }[] = [
    { kind: 'agents', label: 'Специалисты', icon: 'users', img: 'agents' },
    { kind: 'companies', label: 'Компании', icon: 'building', img: 'companies' },
    { kind: 'projects', label: 'Проекты', icon: 'folder', img: 'projects' },
    { kind: 'obligations', label: 'Задачи', icon: 'checks', img: 'obligations' },
    { kind: 'scheduled', label: 'Запланировано', icon: 'clock' },
    { kind: 'cloud', label: 'Облако', icon: 'cloud' },
    { kind: 'memory', label: 'Память', icon: 'book', img: 'memory' },
    { kind: 'mcp', label: 'MCP', icon: 'plug', img: 'mcp' },
    { kind: 'api', label: 'API', icon: 'link', img: 'api' },
    { kind: 'access', label: 'Доступы', icon: 'key', img: 'access' },
    { kind: 'settings', label: 'Настройки', icon: 'sliders' },
  ];
</script>

{#snippet item(t: Thread, deep: boolean)}
  {#if renaming === t.id}
    <div class="chat rename" class:deep>
      <input class="field" bind:value={draft} aria-label="Название чата" onkeydown={(e) => { if (e.key === 'Enter') commitRename(t); else if (e.key === 'Escape') renaming = null; }} onblur={() => commitRename(t)} use:focusInput />
    </div>
  {:else}
  <button use:ctx={() => threadMenu(t)} class="chat" class:deep class:on={isChat(t.id)} aria-current={isChat(t.id) ? "page" : undefined} onclick={() => go({ kind: 'chat', id: t.id })} title={t.title}>
    {#if t.kind === 'forum'}<span class="fi"><Icon name="forum" size={13} /></span>{/if}
    <span class="t">{t.title || 'Новый чат'}</span>
    {#if t.awaiting}<span class="chip warn" title="Ждёт вашего решения">решение</span>
    {:else if t.working}<i class="spin" aria-label="Работает"></i>
    {:else if t.pinned}<Icon name="pin" size={11} />
    {:else}<span class="when faint">{timeAgo(t.updated_at)}</span>{/if}
  </button>
  {/if}
{/snippet}

<svelte:window onresize={() => { if (window.innerWidth > 760) app.sidebar = false; }} />
<nav bind:this={navigation} class="side" class:open={app.sidebar} aria-label="Навигация">
  <div class="brand" hidden={chrome}>
    {#if !chrome}<img class="logo" src="/logo.png" width="52" height="52" alt="Senate" />{/if}
    {#if !app.online}<span class="chip bad" title="Нет связи с нодой">офлайн</span>{/if}
    <span class="grow"></span>
    <button class="icon-btn" onclick={() => (app.palette = true)} aria-label="Поиск и команды" title="Поиск (Ctrl+K)"><Icon name="search" size={15} /></button>
  </div>

  <div class="actions">
    <button class="new" onclick={() => go({ kind: 'new' })}><Icon name="new-chat" size={15} />Новый чат</button>
    <button class="new ghost" onclick={onforum} title="Обсудить широкий вопрос вместе и спланировать работу"><Icon name="forum" size={15} />Форум</button>
  </div>

  <div class="search">
    <Icon name="search" size={13} />
    <input bind:value={q} placeholder="Найти чат" aria-label="Найти чат" />
  </div>

  <div class="lists">
    {#if lounge && !q}
      <button class="chat lounge" class:on={isChat(lounge.id)} onclick={() => go({ kind: 'chat', id: lounge.id })}>
        <Icon name="chat" size={14} /><span class="t">Общее обсуждение</span>
        {#if lounge.working}<i class="spin"></i>{/if}
      </button>
    {/if}

    {#if forums.length}
      <div class="group">
        <div class="gh"><span class="gtoggle static"><Icon name="forum" size={13} /><span class="gt">Форумы</span></span></div>
        {#each forums as t (t.id)}{@render item(t, false)}{/each}
      </div>
    {/if}

    {#each byCompany.groups as g (g.id)}
      <div class="group">
        <div class="gh">
          <button class="gtoggle" aria-expanded={!isFolded(g.id)} onclick={() => fold(g.id)} title={g.name}>
            <span class="chev" class:shut={isFolded(g.id)}><Icon name="down" size={11} /></span>
            <Icon name="building" size={13} /><span class="gt">{g.name}</span><span class="cnt faint">{g.items.length}</span>
          </button>
        </div>
        {#if !isFolded(g.id)}{#each g.items as t (t.id)}{@render item(t, false)}{/each}{/if}
      </div>
    {/each}

    {#if looseChats.length}
      <div class="group">
        <div class="gh"><span class="gtoggle static"><Icon name="chat" size={13} /><span class="gt">{byCompany.groups.length ? 'Без компании' : 'Чаты'}</span></span></div>
        {#each looseChats as t (t.id)}{@render item(t, false)}{/each}
      </div>
    {/if}
    {#if q && !chats.length && !forums.length}<p class="faint pad">Ничего не найдено</p>{/if}
  </div>

  <div class="foot">
    {#each nav as n (n.kind)}
      <button class="nav" class:on={isView(n.kind)} aria-current={isView(n.kind) ? "page" : undefined} onclick={() => go({ kind: n.kind } as View)}>
        <span class="ico">{#if n.img}<img class="nimg" src={`/icons/${n.img}.png`} width="17" height="17" alt="" />{:else}<Icon name={n.icon} size={13} />{/if}</span>{n.label}
        {#if n.kind === 'obligations' && badge}<span class="badge">{badge}</span>{/if}
      </button>
    {/each}
  </div>
</nav>

<style>
  .side {
    width: var(--sidebar-w);
    flex: none;
    background: var(--bg-sidebar);
    border-right: 1px solid var(--line);
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
  .brand[hidden] { display: none; }
  .rename { padding: 2px 4px; }
  .rename .field { height: 30px; width: 100%; }
  .brand {
    height: 46px;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 10px 0 14px;
    flex: none;
  }
  .logo {
    width: 22px;
    height: 22px;
    border-radius: 6px;
    display: grid;
    place-items: center;
    background: var(--fg);
    color: var(--bg);
    font-weight: 700;
    font-size: 13px;
  }
  .grow {
    flex: 1;
  }
  .actions {
    display: flex;
    gap: 6px;
    padding: 0 10px 8px;
  }
  .new {
    flex: 1;
    height: 32px;
    border-radius: var(--radius-s);
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 10px;
    background: var(--bg-raised);
    box-shadow: var(--shadow-1);
    font-weight: 500;
    white-space: nowrap;
    transition: box-shadow 0.12s var(--ease);
  }
  .new:hover {
    box-shadow:
      0 0 0 1px var(--line-strong),
      0 2px 8px rgba(0, 0, 0, 0.08);
  }
  .new.ghost {
    flex: none;
    background: transparent;
    box-shadow: inset 0 0 0 1px var(--line-strong);
    color: var(--fg-2);
  }
  .search {
    margin: 0 10px 8px;
    height: 28px;
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 0 9px;
    border-radius: var(--radius-s);
    background: var(--bg-hover);
    color: var(--fg-3);
  }
  .search input {
    flex: 1;
    border: 0;
    outline: 0;
    background: none;
    min-width: 0;
  }
  .lists {
    flex: 1;
    overflow-y: auto;
    padding: 0 6px 8px;
  }
  .chat {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 5px 8px 5px 24px;
    border-radius: var(--radius-s);
    text-align: left;
    color: var(--fg);
  }
  .chat.deep {
    padding-left: 34px;
  }
  .chat.lounge {
    padding-left: 8px;
    margin-bottom: 6px;
    font-weight: 500;
  }
  .chat:hover {
    background: var(--bg-hover);
  }
  .chat.on {
    background: var(--bg-active);
  }
  .fi {
    color: var(--fg-3);
    display: inline-flex;
  }
  .t {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .when {
    font-size: 11.5px;
    color: var(--fg-2);
  }
  .spin {
    width: 11px;
    height: 11px;
    border-radius: 50%;
    border: 2px solid var(--line-strong);
    border-top-color: var(--accent);
    animation: spin 0.7s linear infinite;
    flex: none;
  }
  .group {
    margin-top: 8px;
  }
  .gh,
  .gtoggle {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 4px 6px;
    color: var(--fg-2);
    font-weight: 600;
    font-size: 12.5px;
    border-radius: var(--radius-s);
  }
  button.gtoggle { text-align: left; width: 100%; }
  .chev { display: inline-flex; width: 11px; transition: transform 0.12s var(--ease); }
  .chev.shut { transform: rotate(-90deg); }
  .cnt { margin-left: auto; font-size: 11.5px; font-weight: 400; }
  .gtoggle.static {
    cursor: default;
  }
  .gtoggle:hover:not(.static) {
    color: var(--fg);
  }
  .gt {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .pad {
    padding: 3px 26px;
    font-size: 12.5px;
  }
  .foot {
    border-top: 1px solid var(--line);
    padding: 6px;
    display: flex;
    flex-direction: column;
    gap: 1px;
    flex: none;
  }
  .nav {
    display: flex;
    align-items: center;
    gap: 9px;
    height: 29px;
    padding: 0 9px;
    border-radius: var(--radius-s);
    color: var(--fg-2);
    text-align: left;
  }
  .nav:hover {
    background: var(--bg-hover);
    color: var(--fg);
  }
  .nav.on {
    background: var(--bg-active);
    color: var(--fg);
  }
  .ico { width: 17px; height: 17px; display: grid; place-items: center; flex: none; }
  .nimg { width: 17px; height: 17px; margin: 0; flex: none; opacity: 0.92; }
  .nav:hover .nimg, .nav.on .nimg { opacity: 1; }
  /* the glass icons are light grey: darken them on a light surface */
  :global(:root[data-theme='light']) .nimg { filter: brightness(0.42) contrast(1.15); }
  @media (prefers-color-scheme: light) { :global(:root:not([data-theme='dark'])) .nimg { filter: brightness(0.42) contrast(1.15); } }
  .badge {
    margin-left: auto;
    min-width: 18px;
    height: 18px;
    padding: 0 5px;
    border-radius: 9px;
    background: var(--accent);
    color: var(--accent-fg);
    font-size: 11.5px;
    font-weight: 600;
    display: grid;
    place-items: center;
  }
  .side { background: var(--bg-sidebar); backdrop-filter: blur(26px) saturate(1.4); padding: 10px 8px 10px; width: var(--sidebar-w); border-right: 1px solid var(--line); }
  .brand { height: 68px; padding: 6px 10px 6px 12px; gap: 9px; }
  .logo { width: 52px; height: 52px; border-radius: 0; background: none; box-shadow: none; display: block; object-fit: contain; flex: none; }
  .search { height: 32px; box-shadow: inset 0 0 0 1px var(--line); }
  .search:focus-within { box-shadow: var(--focus-ring); }
  .chat { min-height: 34px; margin-bottom: 2px; }
  .nav { height: 26px; font-size: 12.5px; gap: 8px; padding: 0 8px; }
  .chat, .nav { border-radius: var(--radius-s); transition: background 0.12s var(--ease), color 0.12s var(--ease); }
  .chat.on, .nav.on { background: var(--bg-active); color: var(--fg); font-weight: 550; }
  .chat.on .when { color: var(--fg-2); }
  .foot { padding-top: 2px; }
  @media (max-width: 760px) {
    .side { display: none; position: fixed; inset: 0 auto 0 0; z-index: 60; width: min(290px, 90vw); box-shadow: var(--shadow-2); }
    .side.open { display: flex; }
    .brand { padding-left: 48px; }
  }
  @media (max-height: 640px) { .side { overflow-y: auto; } .lists { flex: 1 0 100px; } }
</style>
