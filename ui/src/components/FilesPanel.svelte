<script lang="ts">
  // The project folder as a tree, read on the computer it lives on. Folders open on click (big generated ones —
  // node_modules, target, dist — are shown faint and open only when asked); a file opens below the tree, with
  // "into the message" and "open in VS Code" on the computer that has it.
  import { get, post } from '../lib/api';
  import { app } from '../lib/store.svelte';
  import type { TreeEntry } from '../lib/types';
  import Icon from './Icon.svelte';
  import Splitter from './Splitter.svelte';

  // `base`: whose folder — `/api/projects/<id>` (a project's folder) or `/api/threads/<id>` (what a chat works in).
  // `onplace`: where the tree says it is (for the panel's header), when it is not a project folder.
  let { base, threadId = '', onplace }: { base: string; threadId?: string; onplace?: (p: { label: string; machine: string; machine_name: string }) => void } = $props();

  let children = $state<Record<string, TreeEntry[]>>({});
  let open = $state<Record<string, boolean>>({});
  let loading = $state<Record<string, boolean>>({});
  let error = $state('');
  let showHidden = $state(false);
  let selected = $state<string | null>(null);
  let file = $state<{ path: string; text?: string; binary?: boolean; size: number; truncated?: boolean } | null>(null);
  let fileError = $state('');
  let fileLoading = $state(false);
  let note = $state('');

  async function list(path: string) {
    loading[path] = true;
    try {
      const r = await get(`${base}/tree?path=${encodeURIComponent(path)}`);
      children[path] = r.entries;
      if (!path && r.place) onplace?.(r.place);
      if (!path) error = '';
    } catch (e: any) {
      if (!path) error = e.message;
    } finally {
      loading[path] = false;
    }
  }

  function reset() {
    children = {};
    open = {};
    selected = null;
    file = null;
    list('');
  }

  $effect(() => {
    base;
    reset();
  });

  // Fresh after a specialist finishes: re-read the folders that are open.
  export function refresh() {
    for (const p of ['', ...Object.keys(open).filter((k) => open[k])]) list(p);
    if (selected) show(selected);
  }

  function toggle(e: TreeEntry) {
    if (!e.dir) return show(e.path);
    open[e.path] = !open[e.path];
    if (open[e.path] && !children[e.path]) list(e.path);
  }

  async function show(path: string) {
    selected = path;
    fileLoading = true;
    fileError = '';
    try {
      file = await get(`${base}/file?path=${encodeURIComponent(path)}`);
    } catch (e: any) {
      file = null;
      fileError = e.message;
    } finally {
      fileLoading = false;
    }
  }

  function intoMessage(path: string) {
    if (!threadId) return;
    const cur = app.drafts[threadId] ?? '';
    const add = '`' + path + '`';
    app.drafts[threadId] = cur && !cur.endsWith(' ') && !cur.endsWith('\n') ? `${cur} ${add} ` : `${cur}${add} `;
    flash('Путь добавлен в сообщение');
  }

  async function openInEditor(path = '') {
    try {
      await post(`${base}/open`, { path });
      flash('Открывается в VS Code');
    } catch (e: any) {
      flash(e.message);
    }
  }

  let t: ReturnType<typeof setTimeout> | undefined;
  function flash(s: string) {
    note = s;
    clearTimeout(t);
    t = setTimeout(() => (note = ''), 2200);
  }

  const size = (n: number) => (n < 1024 ? `${n} Б` : n < 1024 * 1024 ? `${Math.round(n / 1024)} КБ` : `${(n / 1024 / 1024).toFixed(1)} МБ`);
  const lines = $derived(file?.text ? file.text.split('\n') : []);
  // Drop the trailing empty line a final newline leaves.
  const shownLines = $derived(lines.length && lines[lines.length - 1] === '' ? lines.slice(0, -1) : lines);
</script>

{#snippet level(path: string, depth: number)}
  {#each (children[path] ?? []).filter((e) => showHidden || !e.hidden) as e (e.path)}
    <li>
      <button
        class="row"
        class:on={selected === e.path}
        class:faint={e.heavy || e.hidden}
        style="padding-left:{8 + depth * 14}px"
        onclick={() => toggle(e)}
        ondblclick={() => !e.dir && openInEditor(e.path)}
        title={e.path}
        aria-expanded={e.dir ? !!open[e.path] : undefined}
      >
        <span class="chev">{#if e.dir}<Icon name={open[e.path] ? 'down' : 'right'} size={11} />{/if}</span>
        <Icon name={e.dir ? 'folder' : 'file'} size={13} />
        <span class="nm">{e.name}</span>
        {#if !e.dir}<span class="sz faint">{size(e.size)}</span>{/if}
      </button>
      {#if e.dir && open[e.path]}
        {#if loading[e.path] && !children[e.path]}
          <p class="faint load" style="padding-left:{30 + depth * 14}px">Загрузка…</p>
        {:else}
          <ul>{@render level(e.path, depth + 1)}</ul>
        {/if}
      {/if}
    </li>
  {/each}
{/snippet}

<div class="files">
  <div class="bar">
    <label class="hid"><input type="checkbox" bind:checked={showHidden} /> Скрытые</label>
    <span class="grow"></span>
    <button class="icon-btn" onclick={() => openInEditor()} aria-label="Открыть папку в VS Code" title="Открыть папку в VS Code"><Icon name="terminal" size={14} /></button>
  </div>
  {#if error}
    <p class="faint pad">{error}</p>
  {:else if !children['']}
    <p class="faint pad">Загрузка…</p>
  {:else}
    <div class="treebox" class:half={!!selected}>
      <ul class="tree">{@render level('', 0)}</ul>
      {#if selected}<Splitter pane="tree" edge="bottom" label="Высота дерева файлов" />{/if}
    </div>
  {/if}

  {#if selected}
    <div class="view">
      <div class="vhead">
        <span class="vpath" title={selected}>{selected}</span>
        {#if threadId}<button class="icon-btn" onclick={() => intoMessage(selected!)} aria-label="Вставить путь в сообщение" title="Вставить путь в сообщение"><Icon name="reply" size={13} /></button>{/if}
        <button class="icon-btn" onclick={() => openInEditor(selected!)} aria-label="Открыть в VS Code" title="Открыть в VS Code"><Icon name="terminal" size={13} /></button>
        <button class="icon-btn" onclick={() => ((selected = null), (file = null))} aria-label="Закрыть файл" title="Закрыть файл"><Icon name="x" size={13} /></button>
      </div>
      {#if fileLoading && !file}
        <p class="faint pad">Загрузка…</p>
      {:else if fileError}
        <p class="faint pad">{fileError}</p>
      {:else if file?.binary}
        <p class="faint pad">Двоичный файл, {size(file.size)}. Откройте его в VS Code.</p>
      {:else if file}
        <pre class="code"><code>{#each shownLines as l, i}<span class="ln">{i + 1}</span>{l}{'\n'}{/each}</code></pre>
        {#if file.truncated}<p class="faint pad">Показано первые 512 КБ из {size(file.size)}.</p>{/if}
      {/if}
    </div>
  {/if}
  {#if note}<div class="note" role="status">{note}</div>{/if}
</div>

<style>
  .files {
    display: flex;
    flex-direction: column;
    min-height: 0;
    flex: 1;
    position: relative;
  }
  .bar {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 10px 6px 14px;
    flex: none;
  }
  .hid {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    color: var(--fg-3);
  }
  .grow {
    flex: 1;
  }
  .pad {
    padding: 8px 14px;
    margin: 0;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .tree {
    padding: 0 6px 8px;
    overflow: auto;
    flex: 1;
    min-height: 0;
  }
  .treebox {
    position: relative;
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 0;
  }
  /* with a file open, the tree takes the share the person dragged it to */
  .treebox.half {
    flex: 0 0 var(--tree-h, 42%);
  }
  .row {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
    height: 24px;
    padding-right: 8px;
    border-radius: var(--radius-s);
    text-align: left;
    font-size: 12.5px;
    color: var(--fg);
  }
  .row.faint {
    color: var(--fg-3);
  }
  .row:hover {
    background: var(--bg-hover);
  }
  .row.on {
    background: var(--bg-active);
  }
  .chev {
    width: 11px;
    flex: none;
    display: inline-flex;
    color: var(--fg-3);
  }
  .nm {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .sz {
    font-size: 11px;
    font-variant-numeric: tabular-nums;
  }
  .load {
    margin: 0;
    font-size: 12px;
    height: 22px;
  }
  .view {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    border-top: 1px solid var(--line);
  }
  .vhead {
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 4px 6px 4px 14px;
    flex: none;
  }
  .vpath {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    direction: rtl;
    text-align: left;
    font-size: 12px;
    color: var(--fg-2);
  }
  .code {
    flex: 1;
    min-height: 0;
    overflow: auto;
    margin: 0;
    padding: 6px 10px 12px 0;
    font-family: var(--font-mono, ui-monospace, monospace);
    font-size: 12px;
    line-height: 1.55;
    tab-size: 2;
    background: var(--bg-sunken);
  }
  .ln {
    display: inline-block;
    width: 44px;
    padding-right: 12px;
    text-align: right;
    color: var(--fg-3);
    user-select: none;
  }
  .note {
    position: absolute;
    left: 12px;
    right: 12px;
    bottom: 12px;
    padding: 7px 10px;
    border-radius: var(--radius-s);
    background: var(--bg-raised);
    box-shadow: var(--shadow-1);
    font-size: 12px;
  }
</style>
