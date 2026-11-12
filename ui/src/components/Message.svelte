<script lang="ts">
  import type { Message } from '../lib/types';
  import { agentById, app } from '../lib/store.svelte';
  import { post } from '../lib/api';
  import { clock, dur, md, modelLabel, roleLabel, tokens } from '../lib/format';
  import { PROVIDER_NAME } from '../lib/models';
  import Avatar from './Avatar.svelte';
  import BrandMark from './BrandMark.svelte';
  import PlanCard from './PlanCard.svelte';
  import Chain from './Chain.svelte';
  import Icon from './Icon.svelte';
  import { fileUrl } from '../lib/api';
  import PermissionCard from './PermissionCard.svelte';

  let {
    m,
    onanswer,
    onstop,
    onretry,
    onimage,
  }: { m: Message; onanswer: (m: Message, allow: boolean, always?: boolean) => void; onstop: (m: Message) => void; onretry: (m: Message) => void; onimage?: (a: { url: string; name: string }) => void } = $props();

  const agent = $derived(agentById(m.author_id));
  const working = $derived(m.status === 'working' || m.status === 'queued');
  const p = $derived(m.props ?? {});
  let now = $state(Date.now());
  $effect(() => {
    if (!working) return;
    const t = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(t);
  });
  const elapsed = $derived(Math.max(0, Math.round((now - m.created_at) / 1000)));
  let copied = $state(false);
  let applying = $state(false);
  let reflecting = $state(false);

  const thread = $derived(app.threads.find((t) => t.id === m.thread_id));
  const planApplied = $derived(!!thread?.meta?.plan_applied);

  async function applyPlan() {
    applying = true;
    try {
      await post(`/api/forums/${m.thread_id}/apply`);
    } catch {
      /* already applied or no plan: the thread state shows it */
    } finally {
      applying = false;
    }
  }
  async function reflect() {
    reflecting = true;
    try {
      await post(`/api/messages/${m.id}/reflect`);
    } catch {
      /* the message shows the error */
    } finally {
      reflecting = false;
    }
  }
  let learnError = $state('');
  async function verify(id: string) {
    learnError = '';
    try {
      await post(`/api/memory/${id}/verify`, { basis: 'подтверждено пользователем' });
    } catch (e: any) {
      learnError = e.message;
    }
  }
  async function forget(id: string) {
    learnError = '';
    try {
      await post(`/api/memory/${id}/archive`);
    } catch (e: any) {
      learnError = e.message;
    }
  }
  const kindLabel: Record<string, string> = { observation: 'наблюдение', hypothesis: 'гипотеза', verified: 'проверено', decision: 'решение', preference: 'предпочтение', error: 'ошибка' };

  function copy() {
    navigator.clipboard?.writeText(m.body).then(() => {
      copied = true;
      setTimeout(() => (copied = false), 1200);
    });
  }

  // Code blocks get a copy button.
  function enhance(node: HTMLElement) {
    const run = () => {
      node.querySelectorAll('pre').forEach((pre) => {
        if (pre.dataset.enh) return;
        pre.dataset.enh = '1';
        const b = document.createElement('button');
        b.className = 'copy-code';
        b.type = 'button';
        b.textContent = 'Копировать';
        b.onclick = () => {
          navigator.clipboard?.writeText(pre.querySelector('code')?.textContent ?? pre.textContent ?? '');
          b.textContent = 'Скопировано';
          setTimeout(() => (b.textContent = 'Копировать'), 1200);
        };
        pre.appendChild(b);
      });
    };
    run();
    const mo = new MutationObserver(run);
    mo.observe(node, { childList: true, subtree: true });
    return { destroy: () => mo.disconnect() };
  }

  const html = $derived(m.body ? md(m.body) : '');
  // The model the run reports arrives only once it has started: until then the chat knows the backend it was
  // queued on. Both say the same thing to the person, so the chip shows whichever is known and never blinks out.
  const modelChip = $derived.by(() => {
    if (p.model) return modelLabel(p.model);
    const backend = p.backend ?? '';
    const model = backend.split(':').slice(1).join(':');
    return model ? modelLabel(model) : (PROVIDER_NAME[backend] ?? '');
  });
  const ctxPct = $derived(p.work?.context_window ? Math.round(((p.work.context_tokens ?? 0) / p.work.context_window) * 100) : 0);
</script>

{#if m.author_kind === 'system'}
  <div class="sys">{m.body}</div>
{:else if m.author_kind === 'user'}
  <div class="msg user">
    <div class="bubble">
      {#if p.replyTo}<div class="quote muted">{p.replyTo.text}</div>{/if}
      {#if m.body}<div class="utext">{m.body}</div>{/if}
      {#if p.attachments?.length}
        <div class="atts">
          {#each p.attachments as a}
            {#if a.url && a.type?.startsWith('image/')}
              <button class="thumb" type="button" onclick={() => onimage?.({ url: a.url!, name: a.name })} aria-label={`Открыть ${a.name}`} title={a.name}><img src={fileUrl(a.url)} alt={a.name} loading="lazy" /></button>
            {:else}
              <span class="chip"><Icon name="file" size={12} />{a.name}</span>
            {/if}
          {/each}
        </div>
      {/if}
    </div>
  </div>
{:else}
  <div class="msg agent" data-status={m.status} class:handoff={!!p.call}>
    {#if p.call}
      {@const to = agentById(p.call.to)}
      <div class="relay">
        <span class="pair">
          <Avatar name={agent?.name} color={agent?.accent} size={22} />
          <Icon name="right" size={13} />
          <Avatar name={to?.name} color={to?.accent} size={22} />
        </span>
        <span class="rtext"><b>{agent?.name}</b> зовёт <b>{to?.name ?? 'коллегу'}</b><span class="rq">{m.body.replace(/^→\s*@\S+:\s*/, '')}</span></span>
        <time class="faint">{clock(m.created_at)}</time>
      </div>
    {:else}
    <div class="head">
      <Avatar name={agent?.name} color={agent?.accent} size={22} />
      <b>{agent?.name ?? 'Специалист'}</b>
      {#if roleLabel(agent)}<span class="faint">{roleLabel(agent)}</span>{/if}
      {#if p.asked_by}<span class="chip accent" title={p.request}><Icon name="users" size={11} />отвечает {agentById(p.asked_by)?.name ?? 'коллеге'}</span>{/if}
      {#if p.reply_from}<span class="chip accent" title="Продолжает работу после ответа коллеги"><Icon name="users" size={11} />после {agentById(p.reply_from)?.name ?? 'коллеги'}</span>{/if}
      {#if modelChip}<span class="chip" title={p.backend}>{#if p.backend}<BrandMark id={p.backend} size={12} tint />{/if}{modelChip}</span>{/if}
      <time class="faint">{clock(m.created_at)}</time>
    </div>

    {#if p.steps?.length}
      <Chain steps={p.steps} live={working} durationMs={p.work?.duration_ms} />
    {/if}

    {#if p.ask && p.ask.kind === 'permission' && (p.ask.state === 'pending' || p.ask.state === 'denied' || p.ask.state === 'allowed') && (p.ask.state === 'pending' || working)}
      <PermissionCard ask={p.ask} onanswer={(allow, always) => onanswer(m, allow, always)} />
    {/if}

    {#if working && !m.body}
      <div class="status" role="status">
        <i class="dots" aria-hidden="true"><b></b><b></b><b></b></i>
        <span>{m.status === 'queued' ? 'в очереди' : p.phase || 'думает'}…</span>
        <span class="faint">{elapsed} с</span>
        <button class="stop" onclick={() => onstop(m)}><Icon name="stop" size={11} filled /> Остановить</button>
      </div>
    {/if}

    {#if p.attachments?.length}
      <div class="atts made">
        {#each p.attachments as a}
          {#if a.url && a.type?.startsWith('image/')}
            <button class="thumb" type="button" onclick={() => onimage?.({ url: a.url!, name: a.name })} aria-label={`Открыть ${a.name}`} title={a.name}><img src={fileUrl(a.url)} alt={a.name} loading="lazy" /></button>
          {:else}
            <span class="chip"><Icon name="file" size={12} />{a.name}</span>
          {/if}
        {/each}
      </div>
    {/if}
    {#if html}
      <div class="md" use:enhance>{@html html}</div>
      {#if working}
        <div class="status" role="status">
          <span class="faint">{p.phase || 'пишет'}… {elapsed} с</span>
          <button class="stop" onclick={() => onstop(m)}><Icon name="stop" size={11} filled /> Остановить</button>
        </div>
      {/if}
    {/if}

    {/if}
    {#if m.status === 'cancelled'}<div class="note faint"><Icon name="x" size={12} /> Остановлено</div>{/if}
    {#if m.status === 'failed'}
      <div class="err" role="alert">
        <Icon name="alert" size={15} />
        <div><b>Ответ не получен</b><span>{p.error ?? 'неизвестная ошибка'}</span></div>
        <button class="btn sm" onclick={() => onretry(m)}>Повторить</button>
      </div>
    {/if}
    {#if p.fallback}<div class="note warn"><Icon name="alert" size={12} /> {p.fallback}</div>{/if}
    {#if p.plan && !working}<PlanCard plan={p.plan} applied={planApplied} onapply={applyPlan} busy={applying} />{/if}
    {#if p.rateLimit}
      <div class="note warn"><Icon name="clock" size={12} /> Лимит подписки почти исчерпан ({Math.round((p.rateLimit.utilization ?? 0) * 100)}%)</div>
    {/if}

    {#if p.learning}
      <div class="learn"><i class="spin2"></i><span>Извлекаю знания…</span></div>
    {:else if p.learned?.length}
      <div class="learn full">
        <div class="lh"><Icon name="book" size={13} /><b>Запомнено · {p.learned.length}</b></div>
        <ul>
          {#each p.learned as l (l.id)}
            <li>
              <span class="lt">{l.body}</span>
              <span class="chip {l.kind === 'verified' ? 'ok' : l.kind === 'error' ? 'bad' : l.kind === 'hypothesis' ? 'warn' : ''}">{kindLabel[l.kind] ?? l.kind}</span>
              {#if l.auto}<span class="chip" title="Извлечено средой, не самим специалистом">авто</span>{/if}
              {#if l.kind !== 'verified'}<button class="mini" onclick={() => verify(l.id)} title="Отметить проверенным">Проверено</button>{/if}
              <button class="mini" onclick={() => forget(l.id)} title="Убрать из памяти">Убрать</button>
            </li>
          {/each}
        </ul>
        {#if learnError}<div class="note" style="color:var(--bad)">{learnError}</div>{/if}
      </div>
    {/if}

    {#if !working && (m.body || p.work)}
      <div class="foot">
        {#if m.body}
          <button class="icon-btn" onclick={copy} aria-label="Копировать ответ" title="Копировать"><Icon name={copied ? 'check' : 'copy'} size={14} /></button>
        {/if}
        {#if m.status === 'done' && m.body && !p.learned?.length && !p.learning && !thread?.meta?.status}
          <button class="rem" onclick={reflect} disabled={reflecting}>{reflecting ? 'Извлекаю…' : 'Запомнить главное'}</button>
        {/if}
        {#if p.work}
          <span class="faint" title="Контекст: {tokens(p.work.context_tokens)} из {tokens(p.work.context_window)}">
            {[dur(p.work.duration_ms), p.work.input_tokens ? `${tokens((p.work.input_tokens ?? 0) - (p.work.cached_input_tokens ?? 0) + (p.work.output_tokens ?? 0))} ток.` : '', ctxPct ? `контекст ${ctxPct}%` : '']
              .filter(Boolean)
              .join(' · ')}
          </span>
        {/if}
      </div>
    {/if}
  </div>
{/if}

<style>
  .msg {
    padding: 8px 0;
    animation: fadeIn 0.18s var(--ease);
  }
  .user {
    display: flex;
    justify-content: flex-end;
  }
  .bubble {
    max-width: min(78%, 640px);
    background: var(--bg-active);
    padding: 9px 13px;
    border-radius: 14px 14px 4px 14px;
  }
  .utext {
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .quote {
    font-size: 12px;
    margin-bottom: 4px;
    padding-bottom: 4px;
    border-bottom: 1px solid var(--line);
    max-height: 3em;
    overflow: hidden;
  }
  /* The frame is exactly the picture. The size limits are plain lengths on purpose: a percentage max-width on the
     image is ignored when the bubble works out how wide it must be, and the frame then swells to the screenshot's
     real width and leaves an empty strip beside the picture. */
  .thumb { display: block; flex: none; max-width: 100%; padding: 0; border-radius: var(--radius-s); overflow: hidden; cursor: zoom-in; box-shadow: 0 0 0 1px var(--line-strong); }
  .thumb img { display: block; width: auto; height: auto; max-width: 320px; max-height: 220px; }
  @media (max-width: 520px) { .thumb img { max-width: 240px; } }
  .thumb:focus-visible { outline: none; box-shadow: var(--focus-ring); }
  .atts.made {
    margin: 8px 0 2px;
  }
  .atts {
    display: flex;
    align-items: flex-start;
    gap: 6px;
    flex-wrap: wrap;
    margin-top: 6px;
  }
  /* one specialist calling another: a thread between two faces, not a line of text */
  .relay {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 14px;
    margin: 2px 0;
    border-radius: var(--radius);
    background: var(--bg-hover);
    box-shadow: inset 0 0 0 1px var(--line);
    font-size: 13px;
  }
  .relay .pair {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    flex: none;
    color: var(--fg-3);
  }
  .rtext {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 6px;
  }
  .rq {
    color: var(--fg-2);
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 4px;
  }
  .head b {
    font-weight: 600;
  }
  .head time {
    margin-left: auto;
    font-size: 12px;
  }
  .agent > :not(.head) {
    margin-left: 30px;
  }
  .md {
    overflow-wrap: anywhere;
  }
  .md :global(p) {
    margin: 0 0 0.7em;
  }
  .md :global(p:last-child) {
    margin-bottom: 0;
  }
  .md :global(h1),
  .md :global(h2),
  .md :global(h3) {
    font-size: 1em;
    font-weight: 650;
    margin: 1em 0 0.4em;
  }
  .md :global(ul),
  .md :global(ol) {
    padding-left: 1.4em;
    margin: 0 0 0.7em;
  }
  .md :global(li) {
    margin: 0.15em 0;
  }
  .md :global(a) {
    color: var(--accent);
  }
  .md :global(code) {
    font: 0.88em var(--mono);
    background: var(--bg-hover);
    padding: 0.1em 0.35em;
    border-radius: 4px;
  }
  .md :global(pre) {
    position: relative;
    background: var(--bg-sunken);
    border-radius: var(--radius-s);
    padding: 10px 12px;
    overflow: auto;
    margin: 0.6em 0;
    font-size: 12.5px;
    line-height: 1.55;
  }
  .md :global(pre code) {
    background: none;
    padding: 0;
    font-size: inherit;
  }
  .md :global(.copy-code) {
    position: absolute;
    top: 6px;
    right: 6px;
    font-size: 11px;
    padding: 2px 7px;
    border-radius: 5px;
    background: var(--bg-raised);
    color: var(--fg-2);
    box-shadow: var(--shadow-1);
    opacity: 0;
    transition: opacity 0.12s;
  }
  .md :global(pre:hover .copy-code) {
    opacity: 1;
  }
  .md :global(table) {
    border-collapse: collapse;
    margin: 0.6em 0;
    font-size: 13px;
  }
  .md :global(th),
  .md :global(td) {
    border: 1px solid var(--line);
    padding: 4px 9px;
  }
  .md :global(blockquote) {
    margin: 0.6em 0;
    padding: 0 0 0 12px;
    color: var(--fg-2);
    box-shadow: inset 2px 0 0 var(--line-strong);
  }
  .status {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--fg-2);
    margin-top: 4px;
  }
  .dots {
    display: inline-flex;
    gap: 3px;
  }
  .dots b {
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: var(--fg-3);
    animation: pulse 1.1s infinite ease-in-out;
  }
  .dots b:nth-child(2) {
    animation-delay: 0.15s;
  }
  .dots b:nth-child(3) {
    animation-delay: 0.3s;
  }
  @keyframes pulse {
    0%,
    80%,
    100% {
      opacity: 0.25;
    }
    40% {
      opacity: 1;
    }
  }
  .stop {
    margin-left: auto;
    display: inline-flex;
    align-items: center;
    gap: 5px;
    font-size: 12px;
    color: var(--fg-2);
    padding: 2px 8px;
    border-radius: 10px;
    background: var(--bg-hover);
  }
  .stop:hover {
    color: var(--bad);
  }
  .note {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12.5px;
    margin-top: 4px;
  }
  .note.warn {
    color: var(--warn);
  }
  .err {
    display: flex;
    gap: 10px;
    align-items: center;
    margin-top: 6px;
    padding: 9px 12px;
    border-radius: var(--radius);
    background: var(--bad-soft);
    color: var(--bad);
  }
  .err div {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .err span {
    color: var(--fg-2);
    font-size: 13px;
    overflow-wrap: anywhere;
  }
  .foot {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 4px;
    font-size: 12px;
    opacity: 0;
    transition: opacity 0.12s;
  }
  .agent:hover .foot,
  .agent:focus-within .foot {
    opacity: 1;
  }
  .learn {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 8px;
    font-size: 12.5px;
    color: var(--fg-3);
  }
  .learn.full {
    display: block;
    padding: 10px 12px;
    border-radius: var(--radius);
    background: var(--bg-sunken);
    color: var(--fg);
  }
  .lh {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--fg-2);
    margin-bottom: 6px;
  }
  .learn ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .learn li {
    display: flex;
    align-items: flex-start;
    gap: 6px;
    flex-wrap: wrap;
  }
  .lt {
    flex: 1 1 260px;
    min-width: 0;
    font-size: 13px;
    line-height: 1.4;
  }
  .mini {
    font-size: 12px;
    color: var(--fg-3);
    padding: 1px 6px;
    border-radius: 5px;
  }
  .mini:hover {
    background: var(--bg-hover);
    color: var(--fg);
  }
  .spin2 {
    width: 11px;
    height: 11px;
    border-radius: 50%;
    border: 2px solid var(--line-strong);
    border-top-color: var(--accent);
    animation: spin 0.7s linear infinite;
  }
  .rem {
    font-size: 12px;
    color: var(--fg-3);
    padding: 1px 7px;
    border-radius: 5px;
  }
  .rem:hover:not(:disabled) {
    background: var(--bg-hover);
    color: var(--fg);
  }
  .sys {
    text-align: center;
    color: var(--fg-3);
    font-size: 12.5px;
    padding: 8px 0;
  }
</style>
