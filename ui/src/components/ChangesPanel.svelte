<script lang="ts">
  import { get } from '../lib/api';
  import { app, loadGit } from '../lib/store.svelte';
  import { baseName } from '../lib/format';
  import Diff from './Diff.svelte';
  import Icon from './Icon.svelte';

  let { projectId }: { projectId: string } = $props();

  const git = $derived(app.git[projectId]);
  let selected = $state<string | null>(null);
  let diff = $state('');
  let loading = $state(false);

  async function open(path: string | null) {
    selected = path;
    loading = true;
    try {
      const r = await get(`/api/projects/${projectId}/diff${path ? `?path=${encodeURIComponent(path)}` : ''}`);
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
    loadGit(projectId);
  });
  $effect(() => {
    // keep an open diff fresh when the change list changes
    git?.changes?.length;
    if (selected) open(selected);
  });
</script>

<aside class="panel" aria-label="Изменения в проекте">
  <header>
    <b>Изменения</b>
    <span class="faint">{git?.available ? git.stat || 'без изменений' : ''}</span>
    <span class="grow"></span>
    <button class="icon-btn" onclick={() => loadGit(projectId)} aria-label="Обновить"><Icon name="refresh" size={14} /></button>
    <button class="icon-btn" onclick={() => (app.panel = false)} aria-label="Закрыть"><Icon name="x" size={14} /></button>
  </header>

  {#if !git}
    <p class="faint pad">Загрузка…</p>
  {:else if !git.available}
    <p class="faint pad">Недоступно: {git.reason}. Папка проекта должна быть git-репозиторием.</p>
  {:else}
    <div class="meta faint"><Icon name="branch" size={12} /> {git.branch} · {git.last_commit}</div>
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
        {#if loading && !diff}<p class="faint pad">Загрузка…</p>{:else}<Diff unified={diff} />{/if}
      </div>
    {/if}
  {/if}
</aside>

<style>
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
