<script lang="ts">
  import type { Ask } from '../lib/types';
  import Icon from './Icon.svelte';
  import Diff from './Diff.svelte';

  let { ask, onanswer }: { ask: Ask; onanswer: (allow: boolean) => void } = $props();
  const input = $derived(ask.input ?? {});
  const isEdit = $derived(/^(Edit|MultiEdit)$/.test(ask.tool ?? ''));
  const isWrite = $derived(ask.tool === 'Write');
  const isShell = $derived(/^(Bash|PowerShell)$/.test(ask.tool ?? ''));
  const isPlan = $derived(ask.tool === 'ExitPlanMode');
</script>

<div class="perm" data-state={ask.state} role="group" aria-label="Запрос разрешения">
  <div class="top">
    <span class="ic"><Icon name="shield" size={16} /></span>
    <div class="q">
      <b>{#if ask.state === 'pending'}Нужно ваше разрешение{:else if ask.state === 'allowed'}Разрешено{:else}Отклонено{/if}</b>
      <span class="muted">{ask.question}</span>
    </div>
    {#if ask.state === 'pending'}
      <div class="acts">
        <button class="btn" onclick={() => onanswer(false)}>Отклонить</button>
        <button class="btn primary" onclick={() => onanswer(true)}>{isPlan ? 'Выполнять' : 'Разрешить'}</button>
      </div>
    {/if}
  </div>
  {#if ask.state === 'pending'}
    {#if isEdit}
      <Diff oldStr={input.old_string ?? ''} newStr={input.new_string ?? ''} />
    {:else if isWrite}
      <Diff newStr={(input.content ?? '').slice(0, 4000)} />
    {:else if isShell}
      <pre class="cmd mono">{input.command}</pre>
    {:else if isPlan}
      <pre class="cmd plan">{input.plan}</pre>
    {/if}
  {/if}
</div>

<style>
  .perm {
    margin: 8px 0 4px;
    padding: 10px 12px;
    border-radius: var(--radius);
    background: var(--bg-raised);
    box-shadow: var(--shadow-1);
    display: flex;
    flex-direction: column;
    gap: 8px;
    animation: fadeIn 0.15s var(--ease);
  }
  .perm[data-state='pending'] {
    box-shadow:
      0 0 0 1.5px var(--accent),
      var(--shadow-1);
  }
  .top {
    display: flex;
    gap: 10px;
    align-items: center;
  }
  .ic {
    color: var(--accent);
    display: inline-flex;
  }
  .perm:not([data-state='pending']) .ic {
    color: var(--fg-3);
  }
  .q {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .q span {
    overflow-wrap: anywhere;
  }
  .acts {
    display: flex;
    gap: 6px;
    flex: none;
  }
  .cmd {
    margin: 0;
    padding: 8px 10px;
    border-radius: var(--radius-s);
    background: var(--bg-sunken);
    font-size: 12px;
    white-space: pre-wrap;
    word-break: break-word;
    max-height: 220px;
    overflow: auto;
  }
  .plan {
    font-family: var(--font);
    font-size: 13px;
  }
</style>
