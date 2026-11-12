<script lang="ts">
  import { app, attentionCount, go, type View } from '../lib/store.svelte';
  import { timeAgo } from '../lib/format';
  import type { Thread } from '../lib/types';
  import Icon from './Icon.svelte';

  let { onforum }: { onforum: () => void } = $props();

  let q = $state('');

  const match = (t: Thread) => !q || (t.title || 'Новый чат').toLowerCase().includes(q.toLowerCase());
  const lounge = $derived(app.threads.find((t) => t.kind === 'lounge'));
  const forums = $derived(app.threads.filter((t) => t.kind === 'forum' && match(t)));
  const chats = $derived(app.threads.filter((t) => t.kind === 'chat' && match(t)));
  const looseChats = $derived(chats);

  const cur = $derived(app.view);
  const isChat = (id: string) => cur.kind === 'chat' && cur.id === id;
  const isView = (k: View['kind']) => cur.kind === k;
  const badge = $derived(attentionCount());

  const nav: { kind: View['kind']; label: string; icon: string }[] = [
    { kind: 'agents', label: 'Специалисты', icon: 'users' },
    { kind: 'companies', label: 'Компании', icon: 'building' },
    { kind: 'projects', label: 'Проекты', icon: 'folder' },
    { kind: 'obligations', label: 'Задачи', icon: 'checks' },
    { kind: 'memory', label: 'Память', icon: 'book' },
    { kind: 'mcp', label: 'MCP', icon: 'plug' },
    { kind: 'api', label: 'API', icon: 'link' },
    { kind: 'settings', label: 'Настройки', icon: 'sliders' },
  ];
</script>

{#snippet item(t: Thread, deep: boolean)}
  <button class="chat" class:deep class:on={isChat(t.id)} onclick={() => go({ kind: 'chat', id: t.id })} title={t.title}>
    {#if t.kind === 'forum'}<span class="fi"><Icon name="forum" size={13} /></span>{/if}
    <span class="t">{t.title || 'Новый чат'}</span>
    {#if t.awaiting}<span class="chip warn" title="Ждёт вашего решения">решение</span>
    {:else if t.working}<i class="spin" aria-label="Работает"></i>
    {:else if t.pinned}<Icon name="pin" size={11} />
    {:else}<span class="when faint">{timeAgo(t.updated_at)}</span>{/if}
  </button>
{/snippet}

<nav class="side" aria-label="Навигация">
  <div class="brand">
    <span class="logo" aria-hidden="true">S</span>
    <b>Senate</b>
    {#if !app.online}<span class="chip bad" title="Нет связи с нодой">офлайн</span>{/if}
    <span class="grow"></span>
    <button class="icon-btn" onclick={() => (app.palette = true)} aria-label="Поиск и команды" title="Поиск (Ctrl+K)"><Icon name="search" size={15} /></button>
  </div>

  <div class="actions">
    <button class="new" onclick={() => go({ kind: 'new' })}><Icon name="plus" size={15} />Новый чат</button>
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

    {#if looseChats.length}
      <div class="group">
        <div class="gh"><span class="gtoggle static"><Icon name="chat" size={13} /><span class="gt">Чаты</span></span></div>
        {#each looseChats as t (t.id)}{@render item(t, false)}{/each}
      </div>
    {/if}
    {#if q && !looseChats.length && !forums.length}<p class="faint pad">Ничего не найдено</p>{/if}
  </div>

  <div class="foot">
    {#each nav as n (n.kind)}
      <button class="nav" class:on={isView(n.kind)} onclick={() => go({ kind: n.kind } as View)}>
        <Icon name={n.icon} size={15} />{n.label}
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
</style>
