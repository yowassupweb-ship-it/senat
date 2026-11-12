<script lang="ts">
  import { onMount } from 'svelte';
  import { ApiError, bootstrapConnection, openExternal, setConnection } from './lib/api';
  import { parseLink } from './lib/nodes';
  import { managedLink, readLink, type LinkState } from './lib/link';
  import { app, applyTheme, go, hashToView, loadState, startEvents } from './lib/store.svelte';
  import ActivePanel from './components/ActivePanel.svelte';
  import Emblem from './components/Emblem.svelte';
  import Icon from './components/Icon.svelte';
  import Chat from './components/Chat.svelte';
  import Palette from './components/Palette.svelte';
  import Titlebar from './components/Titlebar.svelte';
  import ContextMenu from './components/ContextMenu.svelte';
  import { fallbackMenu } from './lib/contextmenu.svelte';
  import Sidebar from './components/Sidebar.svelte';
  import Agents from './views/Agents.svelte';
  import Access from './views/Access.svelte';
  import Api from './views/Api.svelte';
  import Companies from './views/Companies.svelte';
  import Mcp from './views/Mcp.svelte';
  import ForumDialog from './components/ForumDialog.svelte';
  import Memory from './views/Memory.svelte';
  import Obligations from './views/Obligations.svelte';
  import Projects from './views/Projects.svelte';
  import Cloud from './views/Cloud.svelte';
  import Scheduled from './views/Scheduled.svelte';
  import Settings from './views/Settings.svelte';

  let connecting = $state(false);
  // Desktop with a server node: the link is rebuilt by the app itself, so a failed start is "wait", not "ask the person".
  const managed = managedLink();
  let link = $state<LinkState | null>(null);
  let connected = $state(false);
  let problem = $state('');
  let hub = $state('');
  let tokenInput = $state('');
  let forumOpen = $state(false);

  async function boot() {
    if (connecting) return;
    connecting = true;
    problem = ''; 
    try {
      await loadState();
      connected = true;
      startEvents();
      app.view = hashToView(location.hash);
    } catch (e) {
      connected = false;
      problem = e instanceof ApiError && e.status === 401 ? 'Токен не подошёл. Проверьте его и попробуйте ещё раз.' : e instanceof ApiError && e.status === 0 ? 'Нода не отвечает. Проверьте адрес и что она запущена.' : String(e);
    } finally { connecting = false; }
  }

  onMount(() => {
    // While the node is not reachable the app retries by itself and says why; the person has nothing to press.
    const watch = managed
      ? setInterval(async () => {
          link = await readLink();
          if (!connected && !connecting) boot();
        }, 1500)
      : undefined;
    try {
      const t = localStorage.getItem('senate.theme') as 'auto' | 'light' | 'dark' | null;
      if (t) applyTheme(t);
    } catch {
      /* storage unavailable */
    }
    const have = bootstrapConnection();
    if (have) boot();
    const onHash = () => (app.view = hashToView(location.hash));
    window.addEventListener('hashchange', onHash);
    const onKey = (e: KeyboardEvent) => {
      if (!connected || e.isComposing) return;
      const mod = e.ctrlKey || e.metaKey;
      if (mod && e.key.toLowerCase() === 'k') {
        e.preventDefault();
        app.palette = !app.palette;
      } else if (mod && e.key === ',') {
        e.preventDefault(); go({ kind: 'settings' });
      } else if (mod && e.key.toLowerCase() === 'n') {
        e.preventDefault();
        go({ kind: 'new' });
      }
    };
    window.addEventListener('keydown', onKey);
    // Links in answers: the desktop window has no browser behind target=_blank, so open them on this computer.
    const onLink = (e: MouseEvent) => {
      if (e.defaultPrevented || e.button !== 0) return;
      const a = (e.target as HTMLElement | null)?.closest?.('a[href]') as HTMLAnchorElement | null;
      const href = a?.getAttribute('href') ?? '';
      if (a && /^https?:\/\//i.test(href) && openExternal(href)) e.preventDefault();
    };
    document.addEventListener('click', onLink);
    return () => {
      clearInterval(watch);
      document.removeEventListener('click', onLink);
      window.removeEventListener('hashchange', onHash);
      window.removeEventListener('keydown', onKey);
    };
  });

  function connect(e: Event) {
    e.preventDefault();
    const l = parseLink(hub) || parseLink(tokenInput);
    if (l) setConnection(l.url, l.token);
    else setConnection(hub, tokenInput.trim());
    boot();
  }

  const v = $derived(app.view);
  const chrome = !!(window as any).__SENATE__?.chrome;
</script>

<svelte:window oncontextmenu={fallbackMenu} />
<ContextMenu />
{#if chrome}<div class="frame"><Titlebar /></div>{/if}
<div class="body" class:chrome>
{#if !connected && managed}
  <main class="gate">
    <div class="wait" role="status" aria-live="polite">
      <Emblem size={64} />
      <h1>Подключаюсь к серверу…</h1>
      <p class="muted">{link?.detail || 'Устанавливаю защищённое соединение с нодой.'}</p>
      <p class="faint hint">Ничего нажимать не нужно: приложение подключится само, как только сервер станет доступен.</p>
    </div>
  </main>
{:else if !connected}
  <main class="gate">
    <form onsubmit={connect}>
      <Emblem size={64} />
      <h1>Подключение к Senate</h1>
      <p class="muted">
        Десктопное приложение подключается к своей ноде само. Если вы открыли Senate в браузере или хотите работать с удалённой нодой — введите её адрес и токен
        (<span class="mono">senate-node token</span>).
      </p>
      <label>Ссылка подключения или адрес ноды<input class="field" bind:value={hub} placeholder="ссылка из Настройки → Ноды, либо https://senate.example.com" /></label>
      <label>Токен (если вводите адрес)<input class="field" type="password" bind:value={tokenInput} autocomplete="off" /></label>
      {#if problem}<p class="bad" role="alert">{problem}</p>{/if}
      <button class="btn primary" type="submit" disabled={connecting || (!tokenInput.trim() && !parseLink(hub))} aria-busy={connecting}>{connecting ? 'Подключение…' : 'Подключиться'}</button>
    </form>
  </main>
{:else if app.ready}
  <div class="shell" class:sidebar-open={app.sidebar}>
    <button class="mobile-toggle icon-btn" aria-label="Открыть навигацию" aria-expanded={app.sidebar} onclick={() => (app.sidebar = !app.sidebar)}><Icon name="panel" /></button>
    {#if app.sidebar}<button class="sidebar-scrim" aria-label="Закрыть навигацию" onclick={() => (app.sidebar = false)}></button>{/if}
    <Sidebar onforum={() => (forumOpen = true)} />
    <main id="main-content" inert={app.sidebar}>
      {#if v.kind === 'chat'}
        {#key v.id}<Chat threadId={v.id} />{/key}
      {:else if v.kind === 'new'}
        {#key v.project ?? ''}<Chat projectId={v.project} />{/key}
      {:else if v.kind === 'agents'}<Agents id={v.id} />
      {:else if v.kind === 'projects'}<Projects id={v.id} />
      {:else if v.kind === 'companies'}<Companies id={v.id} />
      {:else if v.kind === 'obligations'}<Obligations id={v.id} />
      {:else if v.kind === 'mcp'}<Mcp />
      {:else if v.kind === 'api'}<Api />
      {:else if v.kind === 'access'}<Access />
      {:else if v.kind === 'scheduled'}<Scheduled />
      {:else if v.kind === 'cloud'}<Cloud />
      {:else if v.kind === 'memory'}<Memory />
      {:else if v.kind === 'settings'}<Settings />
      {/if}
    </main>
    <ActivePanel />
  </div>
  {#if app.palette}<Palette />{/if}
  {#if forumOpen}<ForumDialog onclose={() => (forumOpen = false)} />{/if}
{:else}
  <div class="boot muted">Загрузка…</div>
{/if}
</div>

<style>
  .frame { position: relative; z-index: 80; }
  .body { height: 100%; }
  .body.chrome { height: calc(100% - 38px); }
  .shell {
    display: flex;
    height: 100%;
  }
  main {
    flex: 1;
    min-width: 0;
    min-height: 0;
    background: transparent;
  }
  .gate {
    height: 100%;
    display: grid;
    place-items: center;
    padding: 20px;
  }
  .gate form {
    width: 400px;
    max-width: 100%;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .wait {
    width: 400px;
    max-width: 100%;
    display: flex;
    flex-direction: column;
    gap: 12px;
    text-align: center;
    align-items: center;
  }
  .wait p {
    margin: 0;
    line-height: 1.5;
  }
  .wait .hint {
    font-size: 12.5px;
  }
  .gate h1 {
    margin: 0;
    font-size: 22px;
  }
  .gate p {
    margin: 0;
  }
  .gate label {
    display: flex;
    flex-direction: column;
    gap: 4px;
    font-weight: 500;
  }
  .bad {
    color: var(--bad);
  }
  .boot {
    height: 100%;
    display: grid;
    place-items: center;
  }
  .mobile-toggle { display: none; }
  .sidebar-scrim { display: none; }
  .gate { background: var(--bg-sunken); overflow-y: auto; }
  .gate form { padding: 32px; width: 460px; border-radius: var(--radius-xl); background: var(--bg-raised); box-shadow: var(--shadow-2); gap: 18px; }
  @media (max-width: 760px) {
    .mobile-toggle { display: grid; position: fixed; top: 14px; left: 12px; z-index: 61; background: var(--bg-raised); }
    .sidebar-scrim { display: block; position: fixed; inset: 0; z-index: 59; background: var(--scrim); backdrop-filter: blur(4px); }
    .gate form { padding: 24px; }
  }
</style>
