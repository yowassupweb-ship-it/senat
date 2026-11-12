<script lang="ts">
  import type { Step } from '../lib/types';
  import { dots, dur } from '../lib/format';
  import Icon from './Icon.svelte';
  import Diff from './Diff.svelte';

  let { steps, live = false, durationMs = 0 }: { steps: Step[]; live?: boolean; durationMs?: number } = $props();

  let open = $state(false);
  let expanded = $state<Record<number, boolean>>({});
  // While the agent works the chain is open so you can watch; when done it folds into one line.
  const isOpen = $derived(live || open);
  const tools = $derived(steps.filter((s) => s.kind === 'tool'));
  const VISIBLE = 5;
  let all = $state(false);
  const indexed = $derived(steps.map((s, i) => ({ s, i })));
  const shown = $derived(all || steps.length <= VISIBLE ? indexed : indexed.slice(-VISIBLE));
  const errors = $derived(tools.filter((s) => s.is_error && s.status !== 'denied').length);

  function icon(name = ''): string {
    if (/^(Bash|PowerShell)$/.test(name)) return 'bash';
    if (/WebSearch/.test(name)) return 'web-search';
    if (/WebFetch|browser|chrome|playwright|puppeteer/i.test(name)) return 'browser';
    if (/^(Read|Glob|Grep|LS)$|senate_(recall|find)$/.test(name)) return 'read';
    if (/Edit|Write|Notebook/.test(name)) return 'pencil';
    if (/memory|remember|learn|record/i.test(name)) return 'memory-write';
    return 'tool';
  }
  const isEdit = (s: Step) => /^(Edit|MultiEdit)$/.test(s.name ?? '');
  const isWrite = (s: Step) => s.name === 'Write';
  const isShell = (s: Step) => /^(Bash|PowerShell)$/.test(s.name ?? '');
  const editPairs = (s: Step): { o: string; n: string }[] =>
    s.name === 'MultiEdit'
      ? (s.input?.edits ?? []).map((e: any) => ({ o: e.old_string ?? '', n: e.new_string ?? '' }))
      : [{ o: s.input?.old_string ?? '', n: s.input?.new_string ?? '' }];
  const pretty = (v: any) => (typeof v === 'string' ? v : JSON.stringify(v, null, 2));
  const plural = (n: number) => (n === 1 ? 'шаг' : n < 5 ? 'шага' : 'шагов');
</script>

<div class="chain" class:live>
  <button class="head" onclick={() => (open = !open)} aria-expanded={isOpen}>
    <span class="chev" class:rot={isOpen}><Icon name="right" size={13} /></span>
    <strong>{#if live}Работает{:else if tools.length}Выполнено: {tools.length} {plural(tools.length)}{:else}Ход работы{/if}</strong>
    <span class="faint">
      {dots(errors ? `${errors} с ошибкой` : '', !live && durationMs ? dur(durationMs) : '')}
    </span>
  </button>

  {#if isOpen}
    <ol class="steps">
      {#if !all && steps.length > VISIBLE}
        <li><button class="more" onclick={() => (all = true)}>ещё {steps.length - VISIBLE} выше</button></li>
      {/if}
      {#each shown as { s, i } (i)}
        {#if s.kind === 'text'}
          <li class="say">{s.text}</li>
        {:else}
          <li class="step" data-status={s.status}>
            <button class="srow" onclick={() => (expanded[i] = !expanded[i])} aria-expanded={!!expanded[i]}>
              <span class="ic"><Icon name={icon(s.name)} size={13} /></span>
              <span class="hint">{s.hint || s.name}</span>
              <span class="state">
                {#if s.status === 'running'}<i class="spin"></i>
                {:else if s.status === 'awaiting'}<span class="chip warn">ждёт разрешения</span>
                {:else if s.status === 'denied'}<span class="chip">отклонено</span>
                {:else if s.is_error}<span class="chip bad">ошибка</span>
                {:else if s.ms != null}<span class="faint">{s.ms >= 1000 ? `${Math.round(s.ms / 1000)} с` : `${s.ms} мс`}</span>{/if}
              </span>
            </button>
            {#if expanded[i]}
              <div class="detail">
                {#if isEdit(s)}
                  {#if s.patch && s.patch.length}
                    <Diff hunks={s.patch} />
                  {:else}
                    {#each editPairs(s) as p}<Diff oldStr={p.o} newStr={p.n} />{/each}
                  {/if}
                {:else if isWrite(s)}
                  <Diff newStr={s.input?.content ?? ''} />
                {:else if isShell(s)}
                  <pre class="io mono">{s.input?.command ?? ''}</pre>
                  {#if s.output}<pre class="io mono out">{s.output}</pre>{/if}
                {:else}
                  {#if s.input && Object.keys(s.input).length}<pre class="io mono">{pretty(s.input)}</pre>{/if}
                  {#if s.output}<pre class="io mono out">{s.output}</pre>{/if}
                {/if}
                {#if (isEdit(s) || isWrite(s)) && s.output && s.is_error}<pre class="io mono out">{s.output}</pre>{/if}
              </div>
            {/if}
          </li>
        {/if}
      {/each}
    </ol>
  {/if}
</div>

<style>
  .chain {
    font-size: 13px;
    margin: 2px 0 6px;
  }
  .head {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    color: var(--fg-2);
    padding: 2px 6px 2px 0;
    border-radius: var(--radius-s);
  }
  .head:hover {
    color: var(--fg);
  }
  .head strong {
    font-weight: 550;
  }
  .chev {
    display: inline-flex;
    transition: transform 0.15s var(--ease);
  }
  .chev.rot {
    transform: rotate(90deg);
  }
  .steps {
    list-style: none;
    margin: 4px 0 0 6px;
    padding: 0 0 0 12px;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  .more {
    color: var(--accent);
    font-size: 12px;
    padding: 2px 0;
  }
  .say {
    color: var(--fg-2);
    padding: 3px 0;
    white-space: pre-wrap;
  }
  .step .srow {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 3px 6px;
    margin-left: -6px;
    border-radius: var(--radius-s);
    text-align: left;
  }
  .step .srow:hover {
    background: var(--bg-hover);
  }
  .ic {
    color: var(--fg-3);
    display: inline-flex;
  }
  .hint {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .step[data-status='denied'] .hint {
    color: var(--fg-3);
    text-decoration: line-through;
  }
  .state {
    flex: none;
    font-size: 12px;
  }
  .spin {
    display: inline-block;
    width: 11px;
    height: 11px;
    border-radius: 50%;
    border: 2px solid var(--line-strong);
    border-top-color: var(--accent);
    animation: spin 0.7s linear infinite;
  }
  .detail {
    margin: 4px 0 6px 22px;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .io {
    margin: 0;
    padding: 8px 10px;
    border-radius: var(--radius-s);
    background: var(--bg-sunken);
    font-size: 12px;
    line-height: 1.5;
    max-height: 240px;
    overflow: auto;
    white-space: pre-wrap;
    word-break: break-word;
  }
  .io.out {
    color: var(--fg-2);
  }
</style>
