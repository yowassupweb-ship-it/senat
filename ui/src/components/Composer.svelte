<script module lang="ts">
  export interface Settings {
    agent: string; // '' = auto-route
    mode: string; // '' = project default
    effort: string;
  }
  export interface Todo {
    content: string;
    status: 'pending' | 'in_progress' | 'completed';
    activeForm?: string;
  }
</script>

<script lang="ts">
  import { app } from '../lib/store.svelte';
  import { currentConnection, get, openExternal, post, upload, type Uploaded } from '../lib/api';
  import { MODE_LABEL, N, count } from '../lib/format';
  import BrandMark from './BrandMark.svelte';
  import Avatar from './Avatar.svelte';
  import Dropdown from './Dropdown.svelte';
  import Icon from './Icon.svelte';

  let {
    settings,
    picked,
    ontoggleagent,
    onchange,
    onsend,
    onstop,
    running = false,
    draftKey = 'new',
    placeholder = 'Напишите специалисту. @ — обратиться к конкретному.',
    projectMode = 'edit',
    autofocus = true,
    compact = false,
    preview,
    threadId,
    git,
    ongit,
    todos = [],
    paused = false,
    onpause,
    onresume,
  }: {
    /** Specialists addressed together (when given, the specialist menu allows several). */
    picked?: string[];
    ontoggleagent?: (id: string) => void;
    settings: Settings;
    onchange: (s: Settings) => void;
    onsend: (text: string, attachments?: Uploaded[]) => void | boolean | Promise<void | boolean>;
    onstop: () => void;
    running?: boolean;
    draftKey?: string;
    placeholder?: string;
    projectMode?: string;
    autofocus?: boolean;
    compact?: boolean;
    /** Link to the local dev server, set by the specialist; replaces the keyboard hint. */
    preview?: { url: string; label: string; on?: 'node' | 'machine' };
    threadId?: string;
    /** Branch and number of changes of the project folder; the chip toggles the changes panel. */
    git?: { label: string; count: number; active: boolean; title?: string };
    ongit?: () => void;
    /** The specialist's current plan (their TodoWrite list): intermediate tasks of this run. */
    todos?: Todo[];
    /** The person stopped the work because they disagree with the plan. */
    paused?: boolean;
    onpause?: () => void | Promise<void>;
    onresume?: () => void | Promise<void>;
  } = $props();

  let tasksOpen = $state(false);
  // Pausing leaves the plan on screen, closes the window and puts the cursor where the objection is written.
  async function pause() {
    await onpause?.();
    tasksOpen = false;
    ta?.focus();
  }
  async function resume() {
    tasksOpen = false;
    await onresume?.();
  }
  let underEl: HTMLDivElement;
  function onWindowPointer(e: Event) {
    if (tasksOpen && underEl && !underEl.contains(e.target as Node)) tasksOpen = false;
  }
  function onWindowKey(e: KeyboardEvent) {
    if (e.key === 'Escape' && tasksOpen) tasksOpen = false;
  }
  const doneCount = $derived(todos.filter((t) => t.status === 'completed').length);
  const current = $derived(todos.find((t) => t.status === 'in_progress'));
  const hasStrip = $derived(!!preview || !!git || todos.length > 0);
  $effect(() => {
    if (!todos.length) tasksOpen = false;
  });

  // The desktop window has no way to open external links itself: the node opens it in the system browser.
  async function openPreview(e: MouseEvent) {
    if (!preview) return;
    if (preview.on === 'node') {
      // started on the server: its localhost is not this computer's — the node passes the page through
      e.preventDefault();
      try {
        const u = new URL(preview.url);
        const r = await get(`/api/preview/${u.port || '80'}?path=${encodeURIComponent(u.pathname.slice(1) + u.search)}`);
        const full = currentConnection().url.replace(/\/+$/, '') + r.url;
        if (!openExternal(full)) window.open(full, '_blank', 'noopener');
      } catch {
        // an older node cannot pass pages through: nothing better to open
      }
      return;
    }
    // the desktop window opens it itself (on this computer, not on the node); a plain browser follows the anchor
    if (openExternal(preview.url)) e.preventDefault();
  }

  let text = $state('');
  let sending = $state(false);
  let ta: HTMLTextAreaElement;
  let mention = $state<{ q: string; start: number } | null>(null);
  let sel = $state(0);

  // ---------- attachments: screenshots first of all (paste, drop or pick) ----------
  interface Pending {
    key: number;
    name: string;
    blob?: string; // local preview, shown at once while the file is still uploading
    up?: Uploaded;
    error?: string;
  }
  const MAX_FILES = 8;
  const MAX_MB = 25;
  let files = $state<Pending[]>([]);
  let fileInput: HTMLInputElement;
  let dragging = $state(false);
  let attachNote = $state('');
  let seq = 0;
  const uploading = $derived(files.some((f) => !f.up && !f.error));
  const ready = $derived(files.filter((f) => f.up));
  const canSend = $derived(!sending && !uploading && (!!text.trim() || ready.length > 0));

  async function addFiles(list: File[]) {
    attachNote = '';
    for (const file of list) {
      if (files.length >= MAX_FILES) {
        attachNote = `Не больше ${count(MAX_FILES, N.file)} в одном сообщении`;
        break;
      }
      if (file.size > MAX_MB * 1024 * 1024) {
        attachNote = `«${file.name || 'файл'}» больше ${MAX_MB} МБ`;
        continue;
      }
      files.push({ key: ++seq, name: file.name || 'скриншот.png', blob: file.type.startsWith('image/') ? URL.createObjectURL(file) : undefined });
      const item = files[files.length - 1];
      upload(file, item.name)
        .then((u) => (item.up = u))
        .catch((e) => (item.error = e?.message || 'не загрузилось'));
    }
    ta?.focus();
  }
  function dropFile(key: number) {
    const i = files.findIndex((f) => f.key === key);
    if (i < 0) return;
    if (files[i].blob) URL.revokeObjectURL(files[i].blob!);
    files.splice(i, 1);
    attachNote = '';
  }
  function clearFiles() {
    files.forEach((f) => f.blob && URL.revokeObjectURL(f.blob));
    files = [];
    attachNote = '';
  }
  // another chat has its own message: what was attached here does not follow
  let lastKey = '';
  $effect(() => {
    if (lastKey && draftKey !== lastKey) clearFiles();
    lastKey = draftKey;
  });
  function onPaste(e: ClipboardEvent) {
    const list = [...(e.clipboardData?.files ?? [])];
    if (!list.length) return; // plain text: the textarea handles it
    e.preventDefault();
    addFiles(list);
  }
  const hasFiles = (e: DragEvent) => [...(e.dataTransfer?.types ?? [])].includes('Files');
  function onDragOver(e: DragEvent) {
    if (!hasFiles(e)) return;
    e.preventDefault();
    dragging = true;
  }
  function onDrop(e: DragEvent) {
    if (!hasFiles(e)) return;
    e.preventDefault();
    dragging = false;
    addFiles([...(e.dataTransfer?.files ?? [])]);
  }

  $effect(() => {
    // switching chats swaps the draft
    text = app.drafts[draftKey] ?? '';
    queueMicrotask(resize);
  });

  function resize() {
    if (!ta) return;
    ta.style.height = 'auto';
    ta.style.height = Math.min(ta.scrollHeight, 260) + 'px';
  }

  $effect(() => {
    if (autofocus && ta) ta.focus();
  });

  // the highlighted row stays in view when the list scrolls
  function keepVisible(node: HTMLElement, active: boolean) {
    if (active) node.scrollIntoView({ block: 'nearest' });
    return { update: (a: boolean) => { if (a) node.scrollIntoView({ block: 'nearest' }); } };
  }
  const candidates = $derived(mention ? app.agents.filter((a) => (a.slug + ' ' + a.name).toLowerCase().includes(mention!.q.toLowerCase())) : []);

  // Slash commands. Only one so far: /compact shortens a long chat by having the specialist write a summary and
  // carry on from it in a fresh session, so a chat can go on for as long as the work does.
  const COMMANDS = [{ name: 'compact', hint: 'Сжать контекст чата: специалист сделает сводку и продолжит с неё. После команды можно добавить, что учесть.' }];
  const slash = $derived.by(() => {
    if (compact || mention) return [];
    const m = /^\/([a-z]*)$/i.exec(text);
    return m && text !== '/compact' ? COMMANDS.filter((c) => c.name.startsWith(m[1].toLowerCase())) : [];
  });
  function pickSlash(name: string) {
    text = `/${name} `;
    app.drafts[draftKey] = text;
    queueMicrotask(() => {
      ta.focus();
      ta.setSelectionRange(text.length, text.length);
      resize();
    });
  }

  function onInput() {
    app.drafts[draftKey] = text;
    resize();
    const pos = ta.selectionStart;
    const m = /@([\p{L}\p{N}_-]*)$/u.exec(text.slice(0, pos));
    mention = m ? { q: m[1], start: pos - m[0].length } : null;
    sel = 0;
  }

  function pickMention(slug: string) {
    if (!mention) return;
    const pos = ta.selectionStart;
    const cursor = mention.start + slug.length + 2;
    text = text.slice(0, mention.start) + '@' + slug + ' ' + text.slice(pos);
    mention = null;
    app.drafts[draftKey] = text;
    queueMicrotask(() => {
      ta.focus();
      ta.setSelectionRange(cursor, cursor);
      resize();
    });
  }

  async function submit() {
    const t = text.trim();
    // Not blocked while someone is working: the backend queues a new session per addressee, so the
    // person can write to another specialist (or add to the task) without waiting.
    if (!canSend) return;
    const key = draftKey;
    sending = true;
    try {
      const sent = await onsend(t, ready.map((f) => f.up!));
      if (sent === false) return;
      app.drafts[key] = '';
      text = '';
      clearFiles();
      mention = null;
      queueMicrotask(resize);
    } finally { sending = false; }
  }

  function onKey(e: KeyboardEvent) {
    if (e.isComposing) return;
    if (mention && candidates.length) {
      if (e.key === 'ArrowDown') {
        e.preventDefault();
        sel = (sel + 1) % candidates.length;
        return;
      }
      if (e.key === 'ArrowUp') {
        e.preventDefault();
        sel = (sel - 1 + candidates.length) % candidates.length;
        return;
      }
      if (e.key === 'Enter' || e.key === 'Tab') {
        e.preventDefault();
        pickMention(candidates[sel].slug);
        return;
      }
      if (e.key === 'Escape') {
        mention = null;
        return;
      }
    }
    if (slash.length && (e.key === 'Enter' || e.key === 'Tab')) {
      e.preventDefault();
      pickSlash(slash[0].name);
      return;
    }
    if (e.key === 'Enter' && !e.shiftKey && !e.isComposing) {
      e.preventDefault();
      submit();
    }
  }

  const agentOpts = $derived([{ id: '', label: 'Авто', hint: 'Среда выберет специалиста по теме и @упоминаниям' }, ...app.agents.map((a) => ({ id: a.id, label: a.name, hint: a.role }))]);
  const modeOpts = Object.entries(MODE_LABEL).map(([id, v]) => ({ id, label: v.label, hint: v.hint }));
  const effortOpts = [
    { id: '', label: 'Обычное', hint: 'Баланс скорости и качества' },
    { id: 'low', label: 'Быстро' },
    { id: 'high', label: 'Тщательно', hint: 'Дольше думает, дороже по лимитам' },
  ];
  const curAgent = $derived(app.agents.find((a) => a.id === settings.agent));
  const group = $derived((picked ?? []).map((id) => app.agents.find((a) => a.id === id)).filter((a): a is NonNullable<typeof a> => !!a));
  const groupLabel = $derived(group.length === 1 ? group[0].name : group.length === 2 ? `${group[0].name}, ${group[1].name}` : `${group.length} специалиста`);
  const effMode = $derived(settings.mode || projectMode || 'edit');
</script>

<svelte:window onpointerdown={onWindowPointer} onkeydown={onWindowKey} />

<div class="composer">
  {#if mention && candidates.length}
    <div class="mentions" role="listbox">
      {#each candidates as a, i (a.id)}
        <button class="m" class:on={i === sel} use:keepVisible={i === sel} role="option" aria-selected={i === sel} onmousedown={(e) => e.preventDefault()} onclick={() => pickMention(a.slug)}>
          <Avatar name={a.name} color={a.accent} size={20} />
          <b>{a.name}</b><span class="faint">@{a.slug}</span>
        </button>
      {/each}
    </div>
  {/if}

  {#if slash.length}
    <div class="mentions" role="listbox" aria-label="Команды">
      {#each slash as c (c.name)}
        <button class="m on" role="option" aria-selected="true" onmousedown={(e) => e.preventDefault()} onclick={() => pickSlash(c.name)}>
          <b>/{c.name}</b><span class="faint">{c.hint}</span>
        </button>
      {/each}
    </div>
  {/if}

  <div class="box" class:drag={dragging} ondragover={onDragOver} ondragleave={() => (dragging = false)} ondrop={onDrop} role="group" aria-label="Сообщение">
    {#if files.length}
      <div class="files">
        {#each files as f (f.key)}
          <div class="file" class:busy={!f.up && !f.error} class:err={!!f.error} title={f.error || f.name}>
            {#if f.blob}<img src={f.blob} alt={f.name} />{:else}<Icon name="file" size={16} /><span class="fn">{f.name}</span>{/if}
            <button class="rm" type="button" onclick={() => dropFile(f.key)} aria-label={`Убрать ${f.name}`}><Icon name="x" size={11} /></button>
          </div>
        {/each}
      </div>
    {/if}
    {#if attachNote}<div class="anote" role="status">{attachNote}</div>{/if}
    <textarea bind:this={ta} bind:value={text} oninput={onInput} onkeydown={onKey} onpaste={onPaste} rows="1" disabled={sending} {placeholder} aria-label="Сообщение"></textarea>
    <input class="pick" type="file" multiple bind:this={fileInput} onchange={(e) => { addFiles([...(e.currentTarget.files ?? [])]); e.currentTarget.value = ''; }} tabindex="-1" aria-hidden="true" />
    <div class="bar">
      <button class="icon-btn" type="button" onclick={() => fileInput.click()} aria-label="Прикрепить файл или скриншот" title="Прикрепить файл (или вставьте скриншот: Ctrl+V)"><Icon name="clip" size={16} /></button>
      {#if !compact}
        <Dropdown value={settings.agent} options={agentOpts} up label="Специалист" selected={ontoggleagent ? (picked ?? []) : undefined} ontoggle={ontoggleagent} onpick={(id) => onchange({ ...settings, agent: id })}>
          {#snippet children(cur)}
            {#if ontoggleagent && group.length}
              <span class="stack">{#each group.slice(0, 3) as g (g.id)}<Avatar name={g.name} color={g.accent} size={16} />{/each}</span>
              <span class="gl2">{groupLabel}</span>
            {:else}
              {#if curAgent}<Avatar name={curAgent.name} color={curAgent.accent} size={16} />{:else}<Icon name="img:agents" size={14} />{/if}
              {cur?.label}
            {/if}
          {/snippet}
        </Dropdown>
      {/if}
      {#if !compact}
        <Dropdown value={effMode} options={modeOpts} up label="Режим разрешений" onpick={(id) => onchange({ ...settings, mode: id })}>
          {#snippet children(cur)}<Icon name="permissions" size={13} />{cur?.label}{/snippet}
        </Dropdown>
        <Dropdown value={settings.effort} options={effortOpts} up label="Тщательность" onpick={(id) => onchange({ ...settings, effort: id })}>
          {#snippet children(cur)}<Icon name="effort" size={13} />{cur?.label}{/snippet}
        </Dropdown>
      {/if}
      <span class="grow"></span>
      <!-- "Send" is always there: someone working does not make writing to anyone impossible. Stopping is a separate, quieter button. -->
      {#if running}
        <button class="stopbtn" type="button" onclick={onstop} aria-label="Остановить работу" title="Остановить работу"><Icon name="stop" size={12} filled />Стоп</button>
      {/if}
      <button class="send" onclick={submit} disabled={!canSend} aria-busy={sending || uploading} title={running ? "Отправить — сообщение встанет в очередь или уйдёт другому специалисту" : "Отправить"} aria-label={sending ? "Отправка…" : uploading ? "Загружается файл…" : "Отправить"}><Icon name="up" size={16} /></button>
    </div>
  </div>
  <!-- Under the composer: where the chat stands (git), what the specialist is doing now (plan), the local server.
       The row has a fixed height whatever it shows, so nothing below or above it moves when it changes. -->
  <div class="under" bind:this={underEl}>
    {#if hasStrip}
      <div class="strip" role="toolbar" aria-label="Состояние работы">
        {#if git}
          <button class="pill" class:on={git.active} aria-pressed={git.active} onclick={ongit} title={git.title || "Изменения в папке проекта"}>
            <Icon name="branch" size={13} /><span class="gl">{git.label}</span>{#if git.count}<b>{git.count}</b>{/if}
          </button>
        {/if}
        {#if preview}
          <a class="pill link" href={preview.url} target="_blank" rel="noopener noreferrer" onclick={openPreview} title={preview.url}>
            <Icon name="link" size={13} /><span class="pl">{preview.label}</span><span class="pu faint">{preview.on === 'node' ? 'на сервере · ' : ''}{preview.url.replace(/^https?:\/\//, '')}</span>
          </a>
        {/if}
        {#if todos.length}
          <button class="pill" class:on={tasksOpen} class:paused aria-expanded={tasksOpen} aria-haspopup="true" onclick={() => (tasksOpen = !tasksOpen)} title="План специалиста">
            <Icon name="list" size={13} /><span class="cnt">{doneCount}/{todos.length}</span>{#if paused}<span class="pz">приостановлен</span>{/if}
            {#if current && !paused}<span class="cur faint">{current.activeForm || current.content}</span>{/if}
          </button>
        {/if}
      </div>
    {:else}
      <div class="hint faint">Enter — отправить <span>·</span> Shift + Enter — новая строка</div>
    {/if}
    <!-- A floating panel over the page, not part of the flow: opening it moves nothing. -->
    {#if tasksOpen && todos.length}
      <div class="tasks" role="dialog" aria-label="План специалиста">
        <div class="thead"><b>План</b><span class="faint">{doneCount} из {todos.length}</span>{#if paused}<button class="btn sm" onclick={resume}>Продолжить как было</button>{:else}<button class="btn sm danger" onclick={pause} title="Остановить работу и написать, что не так">Приостановить</button>{/if}<button class="icon-btn" onclick={() => (tasksOpen = false)} aria-label="Закрыть"><Icon name="x" size={13} /></button></div>
        {#if paused}<div class="hold" role="status">Работа остановлена. Напишите, что в плане не так: агент не продолжит его, пока вы не ответите.</div>{/if}
        <div class="tlist">
          {#each todos as t, i (i)}
            <div class="task" class:done={t.status === 'completed'} class:now={t.status === 'in_progress'}>
              <i class="chk" aria-hidden="true">{#if t.status === 'completed'}<Icon name="check" size={11} />{/if}</i>
              <span>{t.status === 'in_progress' ? t.activeForm || t.content : t.content}</span>
            </div>
          {/each}
        </div>
      </div>
    {/if}
  </div>
</div>

<style>
  .composer {
    position: relative;
    padding: 0 20px 10px;
    max-width: 860px;
    width: 100%;
    margin: 0 auto;
  }
  .box {
    background: var(--bg-raised);
    border-radius: var(--radius-l);
    box-shadow:
      0 0 0 1px var(--line-strong),
      0 2px 10px rgba(0, 0, 0, 0.05);
    transition: box-shadow 0.12s var(--ease);
  }
  .box:focus-within {
    box-shadow:
      0 0 0 1.5px var(--accent),
      0 2px 12px rgba(0, 0, 0, 0.07);
  }
  textarea {
    display: block;
    width: 100%;
    border: 0;
    outline: 0;
    background: none;
    resize: none;
    padding: 12px 14px 4px;
    line-height: 1.5;
    max-height: 260px;
  }
  textarea::placeholder {
    color: var(--fg-3);
  }
  .stack { display: inline-flex; }
  .stack > :global(* + *) { margin-left: -5px; box-shadow: 0 0 0 1.5px var(--bg-raised); border-radius: 50%; }
  .gl2 { max-width: 190px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .bar {
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 4px 8px 8px;
  }
  .grow {
    flex: 1;
  }
  .send {
    width: 30px;
    height: 30px;
    border-radius: 50%;
    display: grid;
    place-items: center;
    background: var(--accent);
    color: var(--accent-fg);
    transition: filter 0.12s;
  }
  .send:hover:not(:disabled) {
    filter: brightness(1.1);
  }
  .send:disabled {
    background: var(--bg-active);
    color: var(--fg-3);
    cursor: default;
  }
  .stopbtn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 30px;
    margin-right: 8px;
    padding: 0 12px;
    border-radius: 999px;
    font-size: 12px;
    color: var(--fg-2, var(--fg));
    box-shadow: 0 0 0 1px var(--line-strong);
  }
  .stopbtn:hover { background: var(--bg-hover); color: var(--fg); }
  .stopbtn:focus-visible { outline: none; box-shadow: var(--focus-ring); }
  .hint {
    text-align: center;
    font-size: 11.5px;
    padding-top: 6px;
  }
  .mentions {
    position: absolute;
    bottom: calc(100% - 4px);
    left: 34px;
    z-index: 40;
    min-width: 240px;
    max-height: min(340px, 50vh);
    overflow-y: auto;
    padding: 4px;
    background: var(--bg-raised);
    border-radius: var(--radius);
    box-shadow: var(--shadow-2);
    animation: fadeIn 0.1s var(--ease);
  }
  .m {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 5px 8px;
    border-radius: var(--radius-s);
    text-align: left;
  }
  .m.on,
  .m:hover {
    background: var(--bg-hover);
  }
  .composer { padding-bottom: 18px; }
  .box { border-radius: var(--radius-xl); box-shadow: var(--shadow-composer); }
  /* the field is focused nearly always: it brightens its own hairline instead of wearing a ring */
  .box:focus-within { box-shadow: var(--shadow-composer-focus); }
  textarea { padding: 16px 16px 10px; min-height: 64px; }
  .bar { flex-wrap: wrap; padding: 6px 10px 10px; gap: 3px; }
  .send { flex: none; }
  .hint { padding-top: 8px; font-size: 11px; }
  .hint span { padding: 0 6px; }
  /* keyboard shortcuts mean nothing on a touch screen */
  @media (hover: none) and (pointer: coarse) { .hint { display: none; } }
  .under {
    display: flex;
    align-items: center;
    justify-content: center;
    height: 36px; /* fixed: the hint, the strip and any change in them take the same room */
    padding-top: 6px;
    box-sizing: border-box;
  }
  .under .hint { padding-top: 0; }
  .strip {
    display: flex;
    align-items: center;
    justify-content: flex-start;
    gap: 6px;
    width: 100%;
    min-width: 0;
    padding: 2px 3px; /* room for the focus ring: nothing clips it */
  }
  .pill {
    display: inline-flex;
    align-items: center;
    flex: 0 1 auto;
    gap: 6px;
    min-width: 0;
    height: 26px;
    padding: 0 11px;
    border-radius: 999px;
    background: var(--bg-raised);
    box-shadow: 0 0 0 1px var(--line-strong);
    color: var(--fg-2, var(--fg));
    font-size: 12px;
    white-space: nowrap;
    text-decoration: none;
    cursor: pointer;
  }
  .pill:hover { background: var(--bg-hover); }
  .pill.on { box-shadow: 0 0 0 1.5px var(--accent); }
  .pill:focus-visible { outline: none; box-shadow: var(--focus-ring); }
  .pill.link { color: var(--accent); }
  .pill span { padding: 0; }
  .pill b { padding: 0 6px; border-radius: 999px; background: var(--accent); color: var(--accent-fg); font-size: 10.5px; line-height: 16px; font-variant-numeric: tabular-nums; }
  .pill .gl { max-width: 160px; overflow: hidden; text-overflow: ellipsis; }
  .pill .cnt { font-variant-numeric: tabular-nums; }
  .pill .pl { font-weight: 600; }
  /* the plan pill is last: when its line of text changes, only its own width moves */
  .pill .cur { max-width: 240px; overflow: hidden; text-overflow: ellipsis; }
  .pill .pu { max-width: 180px; overflow: hidden; text-overflow: ellipsis; }
  .pick { display: none; }
  .box.drag { box-shadow: 0 0 0 2px var(--accent), var(--shadow-composer); }
  .files { display: flex; flex-wrap: wrap; gap: 8px; padding: 12px 14px 0; }
  .file { position: relative; display: inline-flex; align-items: center; gap: 6px; max-width: 200px; min-height: 56px; border-radius: var(--radius-s); background: var(--bg-hover); box-shadow: 0 0 0 1px var(--line-strong); overflow: hidden; }
  .file img { display: block; height: 56px; width: auto; max-width: 140px; object-fit: cover; }
  .file .fn { padding: 0 28px 0 4px; font-size: 12px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .file :global(svg) { margin-left: 10px; flex: none; }
  .file.busy img, .file.busy { opacity: 0.55; }
  .file.err { box-shadow: 0 0 0 1.5px var(--bad); }
  .file .rm { position: absolute; top: 3px; right: 3px; width: 18px; height: 18px; display: grid; place-items: center; border-radius: 50%; background: rgba(0, 0, 0, 0.62); color: #fff; }
  .file .rm:hover { background: rgba(0, 0, 0, 0.85); }
  .anote { padding: 6px 16px 0; font-size: 12px; color: var(--warn); }
  .pill.paused { color: var(--warn, var(--fg)); box-shadow: 0 0 0 1.5px var(--warn, var(--line-strong)); }
  .pill .pz { font-size: 11px; font-weight: 600; }
  .hold { margin: 0 8px 6px; padding: 7px 10px; border-radius: var(--radius-s); background: var(--bg-hover); font-size: 12px; line-height: 1.4; }
  .tasks {
    position: absolute;
    left: 20px;
    bottom: 100%; /* of .composer: sits on top of the input, covers nothing of it */
    z-index: 50;
    width: min(480px, calc(100% - 40px));
    max-height: min(340px, 55vh);
    display: flex;
    flex-direction: column;
    border-radius: var(--radius);
    background: var(--bg-raised);
    box-shadow: 0 0 0 1px var(--line-strong), var(--shadow-2);
  }
  .thead { display: flex; align-items: center; gap: 8px; padding: 8px 8px 6px 14px; font-size: 12.5px; }
  .thead .faint { flex: 1; font-size: 11.5px; }
  .tlist { overflow-y: auto; padding: 0 6px 6px; }
  .task { display: flex; align-items: flex-start; gap: 8px; padding: 4px 8px; font-size: 12.5px; line-height: 1.4; }
  .task .chk { flex: none; box-sizing: border-box; width: 16px; height: 16px; margin-top: 1px; border-radius: 50%; border: 1.5px solid var(--line-strong); background: none; display: grid; place-items: center; }
  .task.now { font-weight: 600; }
  .task.now .chk { border-color: var(--accent); background: radial-gradient(circle, var(--accent) 0 35%, transparent 40%); } /* design-ok: a hard-edged dot, not a gradient */
  .task.done { color: var(--fg-3); }
  .task.done .chk { background: var(--accent); border-color: var(--accent); color: var(--accent-fg); }
  .task span { padding: 0; }
  @media (max-width: 760px) { .composer { padding: 0 12px 12px; } .hint { font-size: 10px; } }
</style>
