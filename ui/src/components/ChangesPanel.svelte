<script lang="ts">
  import Spinner from './Spinner.svelte';
  import { get } from '../lib/api';
  import { app, loadGit, loadThreadGit, pickThreadRepo } from '../lib/store.svelte';
  import { baseName } from '../lib/format';
  import Diff from './Diff.svelte';
  import Icon from './Icon.svelte';

  // `threadId`: the folder the chat works in (works without a project too); `projectId`: that project's folder.
  // `bare`: inside the project panel, which has its own header.
  let { projectId = '', threadId = '', bare = false }: { projectId?: string; threadId?: string; bare?: boolean } = $props();

  const api = $derived(threadId ? `/api/threads/${threadId}` : `/api/projects/${projectId}`);
  const reload = () => (threadId ? loadThreadGit(threadId) : loadGit(projectId));
  const git = $derived(app.git[threadId ? 't:' + threadId : projectId]);
  let selected = $state<string | null>(null);
  let diff = $state('');
  let loading = $state(false);

  async function open(path: string | null) {
    selected = path;
    loading = true;
    try {
      const q = new URLSearchParams();
      if (path) q.set('path', path);
      if (threadId && git?.repo) q.set('repo', git.repo);
      const r = await get(`${api}/diff${q.size ? '?' + q : ''}`);
      diff = r.diff;
    } catch (e: any) {
      diff = '';
    } finally {
      loading = false;
    }
  }

  const label: Record<string, string> = { new: 'новый', modified: 'изменён', deleted: 'удалён', added: 'добавлен', renamed: 'переименован' };

  // Refresh when the panel opens and when the agent finishes something (store reloads git on message done).
  $effect(() => {
    threadId;
    projectId;
    reload();
  });
  $effect(() => {
    // keep an open diff fresh when the change list changes
    git?.changes?.length;
    if (selected) open(selected);
  });
</script>

<svelte:element this={bare ? 'div' : 'aside'} class="panel" class:bare aria-label="Изменения в проекте">
  {#if !bare}
    <header>
      <b>Изменения</b>
      <span class="faint">{git?.available ? git.stat || 'без изменений' : ''}</span>
      <span class="grow"></span>
      <button class="icon-btn" onclick={reload} aria-label="Обновить"><Icon name="refresh" size={14} /></button>
      <button class="icon-btn" onclick={() => (app.panel = false)} aria-label="Закрыть"><Icon name="x" size={14} /></button>
    </header>
  {/if}

  {#if !git}
    <Spinner fill />
  {:else if !git.available}
    {#if threadId && git.repos?.length}
      <div class="repos pad">
        <p class="faint">Сама папка не репозиторий, но внутри есть репозитории. Чьи изменения показать?</p>
        <ul class="repolist">
          {#each git.repos as r (r)}
            <li><button class="repo" onclick={() => { selected = null; pickThreadRepo(threadId, r); }}><Icon name="branch" size={12} /><span>{r}</span></button></li>
          {/each}
        </ul>
      </div>
    {:else}
      <div class="pad">
        <p><b>Изменений пока не видно</b></p>
        <p class="faint">{git.reason === 'не git-репозиторий' ? 'В рабочей папке нет git-репозитория. Привяжите папку проекта с репозиторием: «Папка и компьютер» выше или «Привязать проект / Git» под полем ввода.' : `Причина: ${git.reason}.`}</p>
      </div>
    {/if}
  {:else}
    <div class="meta faint"><Icon name="branch" size={12} /> {git.branch} · {git.last_commit}{#if bare} · {git.stat || 'без изменений'}{/if}</div>
    {#if threadId && git.repo}<div class="meta faint">репозиторий <b>{git.repo}</b> <button class="relink" onclick={() => { selected = null; pickThreadRepo(threadId, ''); }}>сменить</button></div>{/if}
    <ul>
      {#each git.changes ?? [] as c (c.path)}
        <li>
          <button class="file" class:on={selected === c.path} onclick={() => open(selected === c.path ? null : c.path)} title={c.path}>
            <span class="mark {c.status}"></span>
            <span class="name">{baseName(c.path)}</span>
            <span class="path faint">{c.path.includes('/') ? c.path.slice(0, c.path.lastIndexOf('/')) : ''}</span>
            <span class="st faint">{label[c.status]}</span>
          </button>
        </li>
      {:else}
        <li class="faint pad">Рабочее дерево чистое.</li>
      {/each}
    </ul>
    {#if selected}
      <div class="diffwrap">
        {#if loading && !diff}<Spinner fill />{:else}<Diff unified={diff} />{/if}
      </div>
    {/if}
  {/if}
</svelte:element>

<style>
  .repolist { list-style: none; margin: 8px 0 0; padding: 0; display: grid; gap: 4px; }
  .repo { display: flex; align-items: center; gap: 8px; width: 100%; padding: 6px 10px; border: 1px solid var(--line); border-radius: 8px; font-size: 12.5px; text-align: left; }
  .repo:hover { background: var(--accent-soft); }
  .repo:focus-visible, .relink:focus-visible { outline: 2px solid var(--accent); outline-offset: 1px; }
  .repo span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .relink { color: var(--accent); margin-left: 4px; border-radius: 4px; }
  .relink:hover { text-decoration: underline; }
  .pad p { margin: 0 0 4px; }
  .panel.bare {
    width: auto;
    border: 0;
    background: none;
    flex: 1;
  }
  .panel {
    width: 380px;
    flex: none;
    border-left: 1px solid var(--line);
    background: var(--bg-sidebar);
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
  header {
    height: 46px;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 10px 0 14px;
    flex: none;
  }
  .grow {
    flex: 1;
  }
  .pad {
    padding: 8px 14px;
    margin: 0;
  }
  .meta {
    padding: 0 14px 8px;
    font-size: 12px;
    display: flex;
    gap: 5px;
    align-items: center;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0 6px;
    overflow: auto;
    max-height: 40%;
    flex: none;
  }
  .file {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 4px 8px;
    border-radius: var(--radius-s);
    text-align: left;
  }
  .file:hover {
    background: var(--bg-hover);
  }
  .file.on {
    background: var(--bg-active);
  }
  .name {
    font-weight: 500;
    white-space: nowrap;
  }
  .path {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 12px;
  }
  .st {
    font-size: 11.5px;
  }
  .mark {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--warn);
    flex: none;
  }
  .mark.new,
  .mark.added {
    background: var(--ok);
  }
  .mark.deleted {
    background: var(--bad);
  }
  .diffwrap {
    flex: 1;
    min-height: 0;
    overflow: auto;
    padding: 8px;
  }
  .diffwrap :global(.diff) {
    max-height: none;
  }
</style>
