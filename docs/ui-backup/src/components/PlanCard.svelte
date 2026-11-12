<script lang="ts">
  import { agentById } from '../lib/store.svelte';
  import Avatar from './Avatar.svelte';
  import Icon from './Icon.svelte';

  let { plan, applied, onapply, busy = false }: { plan: any; applied: boolean; onapply: () => void; busy?: boolean } = $props();
  const work = $derived((plan?.work ?? []) as any[]);
  const decisions = $derived((plan?.decisions ?? []) as any[]);
</script>

<div class="plan" class:done={applied}>
  <header>
    <span class="ic"><Icon name="flag" size={15} /></span>
    <b>План работы</b>
    {#if applied}<span class="chip ok">принят</span>{/if}
  </header>
  {#if plan?.summary}<p class="sum">{plan.summary}</p>{/if}

  {#if decisions.length}
    <h4>Решения</h4>
    <ul>{#each decisions as d}<li><Icon name="check" size={13} /><span><b>{d.title}</b>{#if d.body} — {d.body}{/if}</span><span class="chip">{d.scope === 'company' ? 'компания' : 'проект'}</span></li>{/each}</ul>
  {/if}

  {#if work.length}
    <h4>Задачи</h4>
    <ol>
      {#each work as w, i}
        <li>
          <span class="n">{i + 1}</span>
          <div class="body"><b>{w.title}</b>
            {#if w.result}<div class="s">Результат: {w.result}</div>{/if}
            {#if w.criteria}<div class="s">Готово, когда: {w.criteria}</div>{/if}
            {#if w.depends_on?.length}<div class="s">После: {w.depends_on.join(', ')}</div>{/if}</div>
          {#if w.owner_id}<span class="who"><Avatar name={agentById(w.owner_id)?.name} color={agentById(w.owner_id)?.accent} size={20} />{w.owner_name}</span>{/if}
        </li>
      {/each}
    </ol>
  {/if}

  {#if plan?.open_questions?.length}
    <h4>Открытые вопросы</h4>
    <ul>{#each plan.open_questions as q}<li><Icon name="alert" size={13} /><span>{q}</span></li>{/each}</ul>
  {/if}

  {#if !applied}
    <footer><button class="btn primary" onclick={onapply} disabled={busy}>Принять план</button><span class="faint">Задачи и решения появятся только после вашего согласия</span></footer>
  {/if}
</div>

<style>
  .plan {
    margin: 8px 0 2px;
    padding: 14px 16px;
    border-radius: var(--radius);
    background: var(--bg-raised);
    box-shadow:
      0 0 0 1.5px var(--accent),
      var(--shadow-1);
  }
  .plan.done {
    box-shadow: var(--shadow-1);
  }
  header {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .ic {
    color: var(--accent);
    display: inline-flex;
  }
  .sum {
    margin: 6px 0 4px;
    color: var(--fg-2);
  }
  h4 {
    margin: 12px 0 6px;
    font-size: 12px;
    font-weight: 600;
    color: var(--fg-3);
    text-transform: uppercase;
    letter-spacing: 0.03em;
  }
  ul,
  ol {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 7px;
  }
  li {
    display: flex;
    gap: 9px;
    align-items: flex-start;
  }
  li > :global(svg) {
    margin-top: 3px;
    color: var(--fg-3);
    flex: none;
  }
  li > span:not(.chip):not(.n):not(.who) {
    flex: 1;
  }
  .n {
    width: 20px;
    height: 20px;
    border-radius: 50%;
    background: var(--bg-hover);
    display: grid;
    place-items: center;
    font-size: 12px;
    color: var(--fg-2);
    flex: none;
  }
  .body {
    flex: 1;
    min-width: 0;
  }
  .s {
    color: var(--fg-2);
    font-size: 13px;
  }
  .who {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: 13px;
    color: var(--fg-2);
    flex: none;
  }
  footer {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-top: 14px;
    font-size: 12.5px;
  }
</style>
