<script lang="ts">
  // A question a specialist asked mid-work (senate_ask_person): they wait for the answer without ending the turn.
  import { post } from '../lib/api';
  import Icon from './Icon.svelte';

  type Q = { id: string; text: string; options?: string[]; state: 'pending' | 'answered' | 'expired'; answer?: string };
  let { q, messageId }: { q: Q; messageId: string } = $props();
  let text = $state('');
  let sending = $state(false);
  let error = $state('');

  async function send(answer: string) {
    if (!answer.trim() || sending) return;
    sending = true;
    error = '';
    try {
      await post(`/api/messages/${messageId}/question`, { answer: answer.trim() });
      text = '';
    } catch (e: any) {
      error = e.message;
    } finally {
      sending = false;
    }
  }
</script>

<div class="qcard" data-state={q.state} role="group" aria-label="Вопрос специалиста">
  <div class="top">
    <span class="ic"><Icon name="chat" size={15} /></span>
    <div class="q">
      <b>{q.state === 'pending' ? 'Вопрос к вам' : q.state === 'answered' ? 'Вы ответили' : 'Остался без ответа'}</b>
      <span>{q.text}</span>
    </div>
  </div>
  {#if q.state === 'pending'}
    {#if q.options?.length}
      <div class="opts">
        {#each q.options as o}<button class="btn sm" disabled={sending} onclick={() => send(o)}>{o}</button>{/each}
      </div>
    {/if}
    <form class="ans" onsubmit={(e) => { e.preventDefault(); send(text); }}>
      <input class="field" bind:value={text} placeholder={q.options?.length ? 'Или свой ответ' : 'Ваш ответ'} aria-label="Ответ специалисту" />
      <button class="btn primary sm" type="submit" disabled={!text.trim() || sending}>Ответить</button>
    </form>
    {#if error}<p class="err" role="alert">{error}</p>{/if}
  {:else if q.state === 'answered'}
    <p class="given">{q.answer}</p>
  {:else}
    <p class="faint given">Специалист продолжил по своему решению.</p>
  {/if}
</div>

<style>
  .qcard {
    margin: 8px 0 4px;
    padding: 10px 12px;
    border-radius: var(--radius);
    background: var(--bg-raised);
    box-shadow: var(--shadow-1);
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .qcard[data-state='pending'] {
    box-shadow: 0 0 0 1.5px var(--accent), var(--shadow-1);
  }
  .top {
    display: flex;
    gap: 10px;
    align-items: flex-start;
  }
  .ic {
    color: var(--accent);
    display: inline-flex;
    padding-top: 2px;
  }
  .qcard:not([data-state='pending']) .ic {
    color: var(--fg-3);
  }
  .q {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .opts {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .ans {
    display: flex;
    gap: 6px;
  }
  .ans .field {
    flex: 1;
    min-width: 0;
  }
  .given {
    margin: 0;
    padding-left: 25px;
    white-space: pre-wrap;
  }
  .err {
    margin: 0;
    color: var(--bad);
    font-size: 12px;
  }
</style>
