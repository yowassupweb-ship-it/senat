<script module lang="ts">
  export interface Settings {
    agent: string; // '' = auto-route
    model: string; // '' = default
    mode: string; // '' = project default
    effort: string;
  }
</script>

<script lang="ts">
  import { app } from '../lib/store.svelte';
  import { MODE_LABEL } from '../lib/format';
  import { modelOptions, modelTitle } from '../lib/models';
  import BrandMark from './BrandMark.svelte';
  import Avatar from './Avatar.svelte';
  import Dropdown from './Dropdown.svelte';
  import Icon from './Icon.svelte';

  let {
    settings,
    onchange,
    onsend,
    onstop,
    running = false,
    draftKey = 'new',
    placeholder = 'Напишите специалисту. @ — обратиться к конкретному.',
    projectMode = 'edit',
    autofocus = true,
    compact = false,
  }: {
    settings: Settings;
    onchange: (s: Settings) => void;
    onsend: (text: string) => void;
    onstop: () => void;
    running?: boolean;
    draftKey?: string;
    placeholder?: string;
    projectMode?: string;
    autofocus?: boolean;
    compact?: boolean;
  } = $props();

  let text = $state('');
  let ta: HTMLTextAreaElement;
  let mention = $state<{ q: string; start: number } | null>(null);
  let sel = $state(0);

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

  const candidates = $derived(mention ? app.agents.filter((a) => (a.slug + ' ' + a.name).toLowerCase().includes(mention!.q.toLowerCase())).slice(0, 6) : []);

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
    text = text.slice(0, mention.start) + '@' + slug + ' ' + text.slice(pos);
    mention = null;
    app.drafts[draftKey] = text;
    queueMicrotask(() => {
      ta.focus();
      const p = mention ? 0 : (text.indexOf('@' + slug + ' ') + slug.length + 2);
      ta.setSelectionRange(p, p);
      resize();
    });
  }

  function submit() {
    const t = text.trim();
    if (!t) return;
    onsend(t);
    text = '';
    app.drafts[draftKey] = '';
    mention = null;
    queueMicrotask(resize);
  }

  function onKey(e: KeyboardEvent) {
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
    if (e.key === 'Enter' && !e.shiftKey && !e.isComposing) {
      e.preventDefault();
      submit();
    }
  }

  const agentOpts = $derived([{ id: '', label: 'Авто', hint: 'Среда выберет специалиста по теме и @упоминаниям' }, ...app.agents.map((a) => ({ id: a.id, label: a.name, hint: a.role }))]);
  const modelOpts = $derived(modelOptions(app.accounts, { includeDemo: true }));
  const modeOpts = Object.entries(MODE_LABEL).map(([id, v]) => ({ id, label: v.label, hint: v.hint }));
  const effortOpts = [
    { id: '', label: 'Обычное', hint: 'Баланс скорости и качества' },
    { id: 'low', label: 'Быстро' },
    { id: 'high', label: 'Тщательно', hint: 'Дольше думает, дороже по лимитам' },
  ];
  const curAgent = $derived(app.agents.find((a) => a.id === settings.agent));
  const effMode = $derived(settings.mode || projectMode || 'edit');
  // The specialist's own main model is what the chat uses unless this chat overrides it.
  const effModel = $derived(settings.model || curAgent?.model_pref || app.defaultBackend);
</script>

<div class="composer">
  {#if mention && candidates.length}
    <div class="mentions" role="listbox">
      {#each candidates as a, i (a.id)}
        <button class="m" class:on={i === sel} role="option" aria-selected={i === sel} onmousedown={(e) => { e.preventDefault(); pickMention(a.slug); }}>
          <Avatar name={a.name} color={a.accent} size={20} />
          <b>{a.name}</b><span class="faint">@{a.slug}</span>
        </button>
      {/each}
    </div>
  {/if}

  <div class="box">
    <textarea bind:this={ta} bind:value={text} oninput={onInput} onkeydown={onKey} rows="1" {placeholder} aria-label="Сообщение"></textarea>
    <div class="bar">
      {#if !compact}
        <Dropdown value={settings.agent} options={agentOpts} up label="Специалист" onpick={(id) => onchange({ ...settings, agent: id })}>
          {#snippet children(cur)}
            {#if curAgent}<Avatar name={curAgent.name} color={curAgent.accent} size={16} />{:else}<Icon name="users" size={14} />{/if}
            {cur?.label}
          {/snippet}
        </Dropdown>
      {/if}
      {#if !compact}
        <Dropdown value={effModel} options={modelOpts} up label="Модель" onpick={(id) => onchange({ ...settings, model: id })}>
          {#snippet children()}<BrandMark id={effModel} size={14} tint />{modelTitle(app.accounts, effModel)}{/snippet}
        </Dropdown>
        <Dropdown value={effMode} options={modeOpts} up label="Режим разрешений" onpick={(id) => onchange({ ...settings, mode: id })}>
          {#snippet children(cur)}<Icon name="shield" size={13} />{cur?.label}{/snippet}
        </Dropdown>
        <Dropdown value={settings.effort} options={effortOpts} up label="Тщательность" onpick={(id) => onchange({ ...settings, effort: id })}>
          {#snippet children(cur)}{cur?.label}{/snippet}
        </Dropdown>
      {/if}
      <span class="grow"></span>
      {#if running}
        <button class="send stop" onclick={onstop} aria-label="Остановить"><Icon name="stop" size={13} filled /></button>
      {:else}
        <button class="send" onclick={submit} disabled={!text.trim()} aria-label="Отправить"><Icon name="up" size={16} /></button>
      {/if}
    </div>
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
  .send.stop {
    background: var(--fg);
    color: var(--bg);
  }
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
</style>
