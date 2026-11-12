<script lang="ts">
  import { del, get, patch, post } from '../lib/api';
  import { agentById, onEvent } from '../lib/store.svelte';
  import type { Integration } from '../lib/types';
  import AgentChips from '../components/AgentChips.svelte';
  import Avatar from '../components/Avatar.svelte';
  import Icon from '../components/Icon.svelte';
  import Page from '../components/Page.svelte';
  import Switch from '../components/Switch.svelte';

  // Connections to external APIs, as in Shar and MBOX: the key lives on the node; the specialist calls the
  // service through a tool and never sees the key. Reads go straight through; writes follow the rule set here.
  let list = $state<Integration[]>([]);
  let approvals = $state<any[]>([]);
  let tests = $state<Record<string, { busy?: boolean; ok?: boolean; message?: string }>>({});
  let paths = $state<Record<string, string>>({});
  let editing = $state<string | null>(null);
  let adding = $state(false);
  let error = $state('');

  const blank = () => ({ name: '', label: '', base_url: '', auth_type: 'bearer', auth_name: '', secret: '', docs: '', write_mode: 'approve', shared: false, agents: [] as string[] });
  let form = $state(blank());

  const authLabel: Record<string, string> = { none: 'без ключа', bearer: 'Bearer-токен', header: 'ключ в заголовке', basic: 'логин:пароль', query: 'ключ в адресе' };
  const writeLabel: Record<string, string> = { deny: 'запись запрещена', approve: 'запись — с вашего согласия', auto: 'запись без вопросов' };
  const templates = [
    { title: 'Bearer-токен', note: 'Authorization: Bearer …', auth_type: 'bearer' },
    { title: 'Ключ в заголовке', note: 'например X-API-Key', auth_type: 'header', auth_name: 'X-API-Key' },
    { title: 'Логин и пароль', note: 'Basic-авторизация', auth_type: 'basic' },
    { title: 'Ключ в адресе', note: '?api_key=…', auth_type: 'query', auth_name: 'api_key' },
    { title: 'Без ключа', note: 'открытое API', auth_type: 'none' },
  ];

  async function load() {
    try {
      list = await get('/api/integrations');
      approvals = (await get('/api/approvals')).filter((a: any) => a.action === 'api.connect');
    } catch (e: any) {
      error = e.message;
    }
  }
  $effect(() => {
    load();
    return onEvent((t) => {
      if (t === 'attention') load();
    });
  });

  function useTemplate(t: (typeof templates)[number]) {
    adding = true;
    editing = null;
    form = { ...blank(), auth_type: t.auth_type, auth_name: (t as any).auth_name ?? '' };
  }
  function edit(m: Integration) {
    editing = m.id;
    adding = false;
    form = { name: m.name, label: m.label, base_url: m.base_url, auth_type: m.auth_type, auth_name: m.auth_name, secret: '', docs: m.docs, write_mode: m.write_mode, shared: m.shared, agents: [...m.agents] };
  }
  async function save() {
    error = '';
    try {
      if (editing) await patch(`/api/integrations/${editing}`, form);
      else await post('/api/integrations', form);
      editing = null;
      adding = false;
      form = blank();
      await load();
    } catch (e: any) {
      error = e.message;
    }
  }
  async function setAccess(m: Integration, agents: string[]) {
    await patch(`/api/integrations/${m.id}`, { agents });
    load();
  }
  async function toggle(m: Integration, enabled: boolean) {
    await patch(`/api/integrations/${m.id}`, { enabled });
    load();
  }
  async function test(m: Integration) {
    tests[m.id] = { busy: true };
    try {
      tests[m.id] = await post(`/api/integrations/${m.id}/test`, { path: paths[m.id] ?? '' });
    } catch (e: any) {
      tests[m.id] = { ok: false, message: e.message };
    }
  }
  async function remove(m: Integration) {
    if (!confirm(`Удалить подключение «${m.label || m.name}»?`)) return;
    await del(`/api/integrations/${m.id}`);
    load();
  }
  async function decide(a: any, approve: boolean) {
    await post(`/api/approvals/${a.id}/decide`, { approve });
    load();
  }
</script>

<Page title="API">
  {#snippet actions()}<button class="btn primary" onclick={() => { adding = !adding; editing = null; form = blank(); }}><Icon name="plus" size={14} />Подключить API</button>{/snippet}

  {#if error}<div class="notice bad" role="alert">{error}</div>{/if}

  {#if approvals.length}
    <div class="notice warn">
      <b>Специалисты просят подключить сервисы · {approvals.length}</b>
      <ul class="plain">
        {#each approvals as a (a.id)}
          <li class="ask">
            <Avatar name={agentById(a.agent_id)?.name} color={agentById(a.agent_id)?.accent} size={22} />
            <span class="grow"><b>{agentById(a.agent_id)?.name}</b> хочет подключить <code>{a.detail?.args?.name}</code>
              <div class="sub">{a.detail?.args?.docs}</div><div class="sub mono">{a.detail?.args?.base_url}</div>
              </span>
            <button class="btn sm primary" onclick={() => decide(a, true)}>Подключить</button>
            <button class="btn sm" onclick={() => decide(a, false)}>Отклонить</button>
          </li>
        {/each}
      </ul>
    </div>
  {/if}

  <section class="mcard">
    <header><div><h2>Подключения · {list.length}</h2></div></header>
    <ul class="rows">
      {#each list as m (m.id)}
        {@const t = tests[m.id]}
        <li class="r" class:off={!m.enabled}>
          <div class="line">
            <span class="ic"><Icon name="link" size={17} /></span>
            <div class="grow">
              <div class="ttl"><b>{m.label || m.name}</b><span class="chip mono">{m.name}</span>
                <span class="chip">{authLabel[m.auth_type]}</span>
                {#if m.auth_type !== 'none'}{#if m.has_secret}<span class="chip ok">ключ задан</span>{:else}<span class="chip warn">нужен ключ</span>{/if}{/if}
                <span class="chip {m.write_mode === 'auto' ? 'warn' : ''}">{writeLabel[m.write_mode]}</span>
                {#if m.shared}<span class="chip accent">всем</span>{/if}
                {#if !m.enabled}<span class="chip">выключено</span>{/if}</div>
              <div class="meta mono">{m.base_url}</div>
              {#if m.docs}<div class="meta">{m.docs}</div>{/if}
            </div>
            <div class="side">
              <Switch on={m.enabled} label="Включить {m.name}" onchange={(v) => toggle(m, v)} />
              <button class="btn sm" onclick={() => edit(m)}>Изменить</button>
              <button class="icon-btn" onclick={() => remove(m)} aria-label="Удалить {m.name}" title="Удалить"><Icon name="trash" size={15} /></button>
            </div>
          </div>
          <div class="access"><span class="lbl">Доступ</span><AgentChips selected={m.agents} shared={m.shared} onchange={(ids) => setAccess(m, ids)} /></div>
          <div class="probe">
            <input class="field mono" bind:value={paths[m.id]} placeholder="/ — путь для проверки, например /v1/me" aria-label="Путь для проверки" />
            <button class="btn sm" onclick={() => test(m)} disabled={t?.busy}>{t?.busy ? 'Проверяю…' : 'Проверить связь'}</button>
          </div>
          {#if t && !t.busy}<div class="notice {t.ok ? 'ok' : 'bad'}" style="margin:8px 0 0">{t.message}</div>{/if}
          {#if editing === m.id}{@render formView()}{/if}
        </li>
      {:else}
        <li class="empty-note">Подключений пока нет</li>
      {/each}
    </ul>
  </section>

  {#if adding && !editing}
    <section class="mcard"><header><div><h2>Новое подключение</h2></div></header>{@render formView()}</section>
  {/if}

  <section class="mcard">
    <header><div><h2>Шаблоны</h2></div></header>
    <div class="catalog">
      {#each templates as t (t.title)}
        <button class="preset" onclick={() => useTemplate(t)}>
          <span class="ic"><Icon name="key" size={16} /></span>
          <span class="grow"><b>{t.title}</b><small>{t.note}</small></span><Icon name="plus" size={15} />
        </button>
      {/each}
    </div>
  </section>
</Page>

{#snippet formView()}
  <form class="mform" onsubmit={(e) => { e.preventDefault(); save(); }}>
    <div class="row2">
      <label>Имя<input class="field mono" bind:value={form.name} required placeholder="crm" /></label>
      <label>Название<input class="field" bind:value={form.label} placeholder="CRM компании" /></label>
    </div>
    <label>Адрес API<input class="field mono" bind:value={form.base_url} required placeholder="https://api.example.com" /></label>
    <div class="row2">
      <label>Авторизация
        <select class="field" bind:value={form.auth_type}>{#each Object.entries(authLabel) as [k, v]}<option value={k}>{v}</option>{/each}</select></label>
      {#if form.auth_type === 'header' || form.auth_type === 'query'}<label>Имя {form.auth_type === 'header' ? 'заголовка' : 'параметра'}<input class="field mono" bind:value={form.auth_name} placeholder={form.auth_type === 'header' ? 'X-API-Key' : 'api_key'} /></label>{/if}
    </div>
    {#if form.auth_type !== 'none'}
      <label>{form.auth_type === 'basic' ? 'Логин:пароль' : 'Ключ'} <small>{editing ? 'оставьте пустым, чтобы не менять' : 'сохраняется на ноде и больше не показывается'}</small>
        <input class="field mono" type="password" autocomplete="off" bind:value={form.secret} placeholder={editing ? '••••••••' : ''} /></label>
    {/if}
    <label>Правило записи
      <select class="field" bind:value={form.write_mode}>{#each Object.entries(writeLabel) as [k, v]}<option value={k}>{v}</option>{/each}</select></label>
    <label>Что умеет API и как его вызывать<textarea class="field" rows="3" bind:value={form.docs}></textarea></label>
    <div class="trust"><Switch on={form.shared} label="Всем" onchange={(v) => (form.shared = v)} /><span><b>Выдать всем специалистам</b></span></div>
    {#if !form.shared}<div><span class="lbl">Кому выдать</span><AgentChips selected={form.agents} onchange={(ids) => (form.agents = ids)} /></div>{/if}
    <div class="row"><button class="btn primary" type="submit">{editing ? 'Сохранить' : 'Подключить'}</button><button class="btn" type="button" onclick={() => { editing = null; adding = false; }}>Отмена</button></div>
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
  .mcard header p {
    margin: 3px 0 0;
    color: var(--fg-2);
    font-size: 13px;
    max-width: 760px;
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
  .probe {
    display: flex;
    gap: 8px;
    margin: 10px 0 0 46px;
    max-width: 520px;
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
  code {
    font: 12px var(--mono);
    background: var(--bg-hover);
    padding: 1px 6px;
    border-radius: 4px;
  }
  .catalog {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(260px, 1fr));
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
  .preset:hover {
    background: var(--bg-hover);
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
  .mform label small {
    font-weight: 400;
    color: var(--fg-3);
  }
  .row2 {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 12px;
  }
  .trust {
    display: flex;
    gap: 12px;
    align-items: center;
  }
  .trust b {
    font-weight: 500;
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
