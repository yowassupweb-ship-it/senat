<script lang="ts">
  import { del, get, patch, post } from '../lib/api';
  import { agentById, app, onEvent } from '../lib/store.svelte';
  import type { McpServer } from '../lib/types';
  import AgentChips from '../components/AgentChips.svelte';
  import Avatar from '../components/Avatar.svelte';
  import Icon from '../components/Icon.svelte';
  import Page from '../components/Page.svelte';
  import Switch from '../components/Switch.svelte';

  // Design follows MBOX: a card with a title and a hint, rows with icon / title / meta / actions at the
  // right, notices for what needs attention. Access is per specialist: the designer gets one set of
  // tools, the developer another.
  let list = $state<McpServer[]>([]);
  let approvals = $state<any[]>([]);
  let tests = $state<Record<string, { busy?: boolean; ok?: boolean; message?: string; tools?: string[] }>>({});
  let editing = $state<string | null>(null);
  let adding = $state(false);
  let error = $state('');

  const blank = () => ({ name: '', kind: 'stdio' as 'stdio' | 'http', command: '', args: '', url: '', env: '', headers: '', note: '', trusted: false, shared: false, agents: [] as string[] });
  let form = $state(blank());

  const presets = [
    { name: 'apple-developer', title: 'Apple Developer', note: 'Документация Apple: API, Human Interface Guidelines, WWDC, примеры кода', kind: 'stdio', command: 'npx', args: '-y @kimsungwhee/apple-docs-mcp', trusted: true, roles: ['developer', 'designer'] },
    { name: 'playwright', title: 'Браузер (Playwright)', note: 'Открывать страницы, кликать, снимать скриншоты — проверять сайты', kind: 'stdio', command: 'npx', args: '-y @playwright/mcp@latest', trusted: false, roles: ['developer', 'designer', 'analyst'] },
    { name: 'context7', title: 'Context7', note: 'Актуальная документация библиотек и фреймворков', kind: 'stdio', command: 'npx', args: '-y @upstash/context7-mcp', trusted: true, roles: ['developer'] },
    { name: 'figma', title: 'Figma Dev Mode', note: 'Макеты и токены из Figma (нужно включить сервер в приложении Figma)', kind: 'http', url: 'http://127.0.0.1:3845/mcp', trusted: true, roles: ['designer'] },
    { name: 'github', title: 'GitHub', note: 'Репозитории, задачи и pull request-ы', kind: 'stdio', command: 'npx', args: '-y @modelcontextprotocol/server-github', env: 'GITHUB_PERSONAL_ACCESS_TOKEN=', trusted: false, roles: ['developer'] },
    { name: 'fetch', title: 'Чтение страниц', note: 'Получать страницы сайтов как текст', kind: 'stdio', command: 'uvx', args: 'mcp-server-fetch', trusted: true, roles: ['analyst', 'assistant'] },
  ];

  async function load() {
    try {
      list = await get('/api/mcp');
      approvals = (await get('/api/approvals')).filter((a: any) => a.action === 'mcp.install');
    } catch (e: any) {
      error = e.message;
    }
  }
  $effect(() => {
    load();
    const off = onEvent((t) => {
      if (t === 'mcp' || t === 'attention') load();
    });
    return off;
  });

  const lines = (s: string) => Object.fromEntries(s.split('\n').map((l) => l.trim()).filter(Boolean).map((l) => { const i = l.indexOf('='); return i < 0 ? [l, ''] : [l.slice(0, i).trim(), l.slice(i + 1)]; }));
  const splitArgs = (s: string) => (s.match(/"[^"]*"|\S+/g) ?? []).map((x) => x.replace(/^"|"$/g, ''));

  function applyPreset(p: (typeof presets)[number]) {
    adding = true;
    editing = null;
    const who = app.agents.filter((a) => p.roles.includes(a.role)).map((a) => a.id);
    form = { ...blank(), name: p.name, kind: p.kind as 'stdio' | 'http', command: p.command ?? '', args: p.args ?? '', url: (p as any).url ?? '', env: (p as any).env ?? '', note: p.note, trusted: p.trusted, agents: who };
  }

  function edit(m: McpServer) {
    editing = m.id;
    adding = false;
    form = { name: m.name, kind: m.kind, command: m.command, args: m.args.join(' '), url: m.url, env: m.env_keys.map((k) => `${k}=`).join('\n'), headers: m.header_keys.map((k) => `${k}=`).join('\n'), note: m.note, trusted: m.trusted, shared: m.shared, agents: [...m.agents] };
  }

  async function save() {
    error = '';
    const body: any = { name: form.name, kind: form.kind, command: form.command, args: splitArgs(form.args), url: form.url, env: lines(form.env), headers: lines(form.headers), note: form.note, trusted: form.trusted, shared: form.shared, agents: form.agents };
    try {
      if (editing) await patch(`/api/mcp/${editing}`, body);
      else await post('/api/mcp', body);
      editing = null;
      adding = false;
      form = blank();
      await load();
    } catch (e: any) {
      error = e.message;
    }
  }

  async function setAccess(m: McpServer, agents: string[]) {
    await patch(`/api/mcp/${m.id}`, { agents });
    load();
  }
  async function toggle(m: McpServer, k: 'enabled' | 'trusted' | 'shared', v: boolean) {
    await patch(`/api/mcp/${m.id}`, { [k]: v });
    load();
  }
  async function test(m: McpServer) {
    tests[m.id] = { busy: true };
    try {
      tests[m.id] = await post(`/api/mcp/${m.id}/test`);
    } catch (e: any) {
      tests[m.id] = { ok: false, message: e.message };
    }
  }
  async function remove(m: McpServer) {
    if (!confirm(`Удалить сервер «${m.name}»? Он пропадёт у всех специалистов.`)) return;
    await del(`/api/mcp/${m.id}`);
    load();
  }
  async function decide(a: any, approve: boolean) {
    await post(`/api/approvals/${a.id}/decide`, { approve });
    load();
  }
  const has = (name: string) => list.some((m) => m.name === name);
</script>

<Page title="MCP">
  {#snippet actions()}<button class="btn primary" onclick={() => { adding = !adding; editing = null; form = blank(); }}><Icon name="plus" size={14} />Добавить сервер</button>{/snippet}

  {#if error}<div class="notice bad" role="alert">{error}</div>{/if}

  {#if approvals.length}
    <div class="notice warn">
      <b>Специалисты просят установить инструменты · {approvals.length}</b>
      <ul class="plain">
        {#each approvals as a (a.id)}
          <li class="ask">
            <Avatar name={agentById(a.agent_id)?.name} color={agentById(a.agent_id)?.accent} size={22} />
            <span class="grow"><b>{agentById(a.agent_id)?.name}</b> хочет <code>{a.detail?.args?.name}</code> — {a.detail?.args?.note}
              {#if a.detail?.args?.reason}<div class="sub">Зачем сейчас: {a.detail.args.reason}</div>{/if}
              <div class="sub mono">{a.detail?.args?.kind === 'http' ? a.detail?.args?.url : `${a.detail?.args?.command ?? ''} ${(a.detail?.args?.args ?? []).join(' ')}`}</div></span>
            <button class="btn sm primary" onclick={() => decide(a, true)}>Установить</button>
            <button class="btn sm" onclick={() => decide(a, false)}>Отклонить</button>
          </li>
        {/each}
      </ul>
    </div>
  {/if}

  <section class="mcard">
    <header><div><h2>Серверы · {list.length}</h2></div></header>
    <ul class="rows">
      {#each list as m (m.id)}
        {@const t = tests[m.id]}
        <li class="r" class:off={!m.enabled}>
          <div class="line">
            <span class="ic"><Icon name="plug" size={17} /></span>
            <div class="grow">
              <div class="ttl"><b>{m.name}</b><span class="chip">{m.kind === 'http' ? 'HTTP' : 'локальный'}</span>
                {#if m.trusted}<span class="chip ok" title="Его инструменты не спрашивают разрешения">без вопросов</span>{/if}
                {#if m.shared}<span class="chip accent">всем</span>{/if}
                {#if m.missing.length}<span class="chip warn">нужны ключи: {m.missing.join(', ')}</span>{/if}
                {#if !m.enabled}<span class="chip">выключен</span>{/if}</div>
              <div class="meta">{m.note || '—'}</div>
              <div class="meta mono">{m.kind === 'http' ? m.url : `${m.command} ${m.args.join(' ')}`}</div>
            </div>
            <div class="side">
              <Switch on={m.enabled} label="Включить {m.name}" onchange={(v) => toggle(m, 'enabled', v)} />
              <button class="btn sm" onclick={() => test(m)} disabled={t?.busy}>{t?.busy ? 'Проверяю…' : 'Проверить'}</button>
              <button class="btn sm" onclick={() => edit(m)}>Изменить</button>
              <button class="icon-btn" onclick={() => remove(m)} aria-label="Удалить {m.name}" title="Удалить"><Icon name="trash" size={15} /></button>
            </div>
          </div>
          <div class="access"><span class="lbl">Доступ</span><AgentChips selected={m.agents} shared={m.shared} onchange={(ids) => setAccess(m, ids)} /></div>
          {#if t && !t.busy}
            <div class="notice {t.ok ? 'ok' : 'bad'}" style="margin:8px 0 0">{t.message}{#if t.tools?.length}<div class="tools">{#each t.tools as x}<code>{x}</code>{/each}</div>{/if}</div>
          {/if}
          {#if editing === m.id}{@render formView()}{/if}
        </li>
      {:else}
        <li class="empty-note">Серверов пока нет</li>
      {/each}
    </ul>
  </section>

  {#if adding && !editing}
    <section class="mcard"><header><div><h2>Новый сервер</h2></div></header>{@render formView()}</section>
  {/if}

  <section class="mcard">
    <header><div><h2>Каталог</h2></div></header>
    <div class="catalog">
      {#each presets as p (p.name)}
        <button class="preset" onclick={() => applyPreset(p)} disabled={has(p.name)}>
          <span class="ic"><Icon name="plug" size={16} /></span>
          <span class="grow"><b>{p.title}</b><small>{p.note}</small></span>
          {#if has(p.name)}<span class="chip ok">установлен</span>{:else}<Icon name="plus" size={15} />{/if}
        </button>
      {/each}
    </div>
  </section>
</Page>

{#snippet formView()}
  <form class="mform" onsubmit={(e) => { e.preventDefault(); save(); }}>
    <div class="row2">
      <label>Имя<input class="field" bind:value={form.name} required placeholder="playwright" /></label>
      <label>Тип
        <div class="tabs"><button type="button" class:on={form.kind === 'stdio'} onclick={() => (form.kind = 'stdio')}>Локальный (команда)</button><button type="button" class:on={form.kind === 'http'} onclick={() => (form.kind = 'http')}>HTTP (адрес)</button></div></label>
    </div>
    {#if form.kind === 'stdio'}
      <div class="row2">
        <label>Команда<input class="field mono" bind:value={form.command} placeholder="npx" /></label>
        <label>Аргументы<input class="field mono" bind:value={form.args} placeholder="-y @playwright/mcp@latest" /></label>
      </div>
      <label>Переменные окружения<textarea class="field mono" rows="2" bind:value={form.env} placeholder="API_TOKEN=…"></textarea></label>
    {:else}
      <label>Адрес<input class="field mono" bind:value={form.url} placeholder="https://…/mcp" /></label>
      <label>Заголовки<textarea class="field mono" rows="2" bind:value={form.headers} placeholder="Authorization=Bearer …"></textarea></label>
    {/if}
    <label>Что это и зачем<input class="field" bind:value={form.note} /></label>
    <div class="trust"><Switch on={form.trusted} label="Без вопросов" onchange={(v) => (form.trusted = v)} /><span><b>Не спрашивать разрешения</b></span></div>
    <div class="trust"><Switch on={form.shared} label="Всем" onchange={(v) => (form.shared = v)} /><span><b>Выдать всем специалистам</b></span></div>
    {#if !form.shared}<div><span class="lbl">Кому выдать</span><AgentChips selected={form.agents} onchange={(ids) => (form.agents = ids)} /></div>{/if}
    <div class="row"><button class="btn primary" type="submit">{editing ? 'Сохранить' : 'Добавить'}</button><button class="btn" type="button" onclick={() => { editing = null; adding = false; }}>Отмена</button></div>
  </form>
{/snippet}

<style>
  .mcard {
    background: var(--bg-raised);
    border-radius: var(--radius);
    box-shadow: var(--shadow-1);
    margin-bottom: 18px;
    overflow: hidden;
  }
  .mcard > header {
    padding: 14px 18px 10px;
  }
  .mcard h2 {
    margin: 0;
    font-size: 14.5px;
    font-weight: 650;
  }
  .rows {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .r {
    padding: 12px 18px;
    border-top: 1px solid var(--line);
  }
  .r.off .line,
  .r.off .access {
    opacity: 0.55;
  }
  .line {
    display: flex;
    gap: 12px;
    align-items: flex-start;
  }
  .ic {
    width: 34px;
    height: 34px;
    border-radius: 9px;
    background: var(--bg-hover);
    color: var(--fg-2);
    display: grid;
    place-items: center;
    flex: none;
  }
  .grow {
    flex: 1;
    min-width: 0;
  }
  .ttl {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-wrap: wrap;
  }
  .meta {
    color: var(--fg-2);
    font-size: 13px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .meta.mono {
    color: var(--fg-3);
    font-size: 12px;
  }
  .side {
    display: flex;
    align-items: center;
    gap: 8px;
    flex: none;
  }
  .access {
    display: flex;
    align-items: center;
    gap: 10px;
    margin: 10px 0 0 46px;
  }
  .lbl {
    font-size: 12px;
    font-weight: 600;
    color: var(--fg-3);
    text-transform: uppercase;
    letter-spacing: 0.03em;
    display: block;
    margin-bottom: 4px;
  }
  .access .lbl {
    margin: 0;
  }
  .notice {
    padding: 11px 14px;
    border-radius: var(--radius);
    margin: 0 0 16px;
    font-size: 13.5px;
  }
  .notice.warn {
    background: var(--warn-soft);
  }
  .notice.ok {
    background: var(--ok-soft);
  }
  .notice.bad {
    background: var(--bad-soft);
    color: var(--bad);
  }
  .plain {
    list-style: none;
    margin: 8px 0 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .ask {
    display: flex;
    gap: 10px;
    align-items: flex-start;
  }
  .sub {
    color: var(--fg-2);
    font-size: 12.5px;
  }
  .tools {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    margin-top: 6px;
  }
  .tools code,
  code {
    font: 12px var(--mono);
    background: var(--bg-hover);
    padding: 1px 6px;
    border-radius: 4px;
  }
  .catalog {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(min(100%, 300px), 1fr));
    gap: 1px;
    background: var(--line);
    border-top: 1px solid var(--line);
  }
  .preset {
    display: flex;
    gap: 12px;
    align-items: center;
    padding: 12px 18px;
    text-align: left;
    background: var(--bg-raised);
  }
  .preset:hover:not(:disabled) {
    background: var(--bg-hover);
  }
  .preset:disabled {
    cursor: default;
  }
  .preset .ic {
    background: var(--accent-soft);
    color: var(--accent);
  }
  .preset b {
    display: block;
  }
  .preset small {
    color: var(--fg-2);
    font-size: 12.5px;
    display: block;
    line-height: 1.35;
  }
  .mform {
    padding: 14px 18px 18px;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .mform label {
    display: flex;
    flex-direction: column;
    gap: 4px;
    font-weight: 500;
    font-size: 13px;
  }
  .row2 {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 12px;
  }
  .trust {
    display: flex;
    gap: 12px;
    align-items: flex-start;
  }
  .trust b {
    font-weight: 500;
    display: block;
  }
  .r > .mform {
    margin: 10px 0 0 46px;
    padding: 14px;
    border-radius: var(--radius);
    background: var(--bg-sunken);
  }
  @media (max-width: 800px) {
    .row2 {
      grid-template-columns: 1fr;
    }
    .line {
      flex-wrap: wrap;
    }
  }
</style>
