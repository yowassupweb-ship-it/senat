<script lang="ts">
  import { onMount } from 'svelte';
  import { ApiError, bootstrapConnection, setConnection } from './lib/api';
  import { app, applyTheme, go, hashToView, loadState, startEvents } from './lib/store.svelte';
  import ActivePanel from './components/ActivePanel.svelte';
  import Icon from './components/Icon.svelte';
  import Chat from './components/Chat.svelte';
  import Palette from './components/Palette.svelte';
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
  import Settings from './views/Settings.svelte';

  let connecting = $state(false);
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
    return () => {
      window.removeEventListener('hashchange', onHash);
      window.removeEventListener('keydown', onKey);
    };
  });

  function connect(e: Event) {
    e.preventDefault();
    setConnection(hub, tokenInput.trim());
    boot();
  }

  const v = $derived(app.view);
</script>

{#if !connected}
  <main class="gate">
    <form onsubmit={connect}>
      <span class="logo">S</span>
      <h1>Подключение к Senate</h1>
      <p class="muted">
        Десктопное приложение подключается к своей ноде само. Если вы открыли Senate в браузере или хотите работать с удалённой нодой — введите её адрес и токен
        (<span class="mono">senate-node token</span>).
      </p>
      <label>Адрес ноды<input class="field" bind:value={hub} placeholder="пусто — этот же сервер, либо https://senate.example.com" /></label>
      <label>Токен<input class="field" type="password" bind:value={tokenInput} autocomplete="off" /></label>
      {#if problem}<p class="bad" role="alert">{problem}</p>{/if}
      <button class="btn primary" type="submit" disabled={connecting || !tokenInput.trim()} aria-busy={connecting}>{connecting ? 'Подключение…' : 'Подключиться'}</button>
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

<style>
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
  .logo {
    width: 34px;
    height: 34px;
    border-radius: 9px;
    display: grid;
    place-items: center;
    background: var(--fg);
    color: var(--bg);
    font-weight: 700;
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
