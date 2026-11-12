<script lang="ts">
  import { del, get, patch, post } from '../lib/api';
  import { agentById, app, onEvent } from '../lib/store.svelte';
  import { dateTime } from '../lib/format';
  import type { Access } from '../lib/types';
  import Avatar from '../components/Avatar.svelte';
  import Icon from '../components/Icon.svelte';
  import Page from '../components/Page.svelte';
  import Switch from '../components/Switch.svelte';

  // What the person lets specialists use: servers, databases, sites. The secret lives on the node and never comes
  // back to any screen. Each specialist is given each access separately: no / asks you every time / allowed.
  let list = $state<Access[]>([]);
  let asks = $state<any[]>([]);
  let logs = $state<Record<string, any[]>>({});
  let openLog = $state<string | null>(null);
  let editing = $state<string | null>(null);
  let adding = $state(false);
  let error = $state('');

  const kinds: Record<string, string> = { server: 'Сервер', database: 'База данных', site: 'Сайт', service: 'Сервис', other: 'Другое' };
  const hostHint: Record<string, string> = {
    server: 'ssh://deploy@203.0.113.7:22',
    database: 'postgres://app@db.example.com:5432/shop',
    site: 'https://admin.example.com',
    service: 'https://api.example.com',
    other: '',
  };
  const modes: [string, string, string][] = [
    ['deny', 'Нет', 'Специалист не получит доступ'],
    ['ask', 'Спросит', 'Каждый раз придёт запрос вам: разрешить один раз или отклонить'],
    ['allow', 'Разрешён', 'Специалист получает доступ сам, когда он нужен для работы'],
  ];

  const blank = () => ({ name: '', label: '', kind: 'server', host: '', login: '', secret: '', note: '' });
  let form = $state(blank());

  async function load() {
    try {
      list = await get('/api/accesses');
      asks = (await get('/api/approvals')).filter((a: any) => a.action === 'access.use');
    } catch (e: any) {
      error = e.message;
    }
  }
  $effect(() => {
    load();
    return onEvent((t) => {
      if (t === 'attention' || t === 'accesses') load();
    });
  });

  function edit(m: Access) {
    editing = m.id;
    adding = false;
    form = { name: m.name, label: m.label, kind: m.kind, host: m.host, login: m.login, secret: '', note: m.note };
  }
  async function save() {
    error = '';
    try {
      if (editing) await patch(`/api/accesses/${editing}`, form);
      else await post('/api/accesses', form);
      editing = null;
      adding = false;
      form = blank();
      await load();
    } catch (e: any) {
      error = e.message;
    }
  }
  async function grant(m: Access, agentId: string, mode: string) {
    try {
      await patch(`/api/accesses/${m.id}`, { grants: { [agentId]: mode } });
      await load();
    } catch (e: any) {
      error = e.message;
    }
  }
  async function toggle(m: Access, enabled: boolean) {
    await patch(`/api/accesses/${m.id}`, { enabled });
    load();
  }
  async function dropSecret(m: Access) {
    if (!confirm(`Удалить сохранённый пароль или ключ у «${m.label || m.name}»? Доступ перестанет работать, пока вы не зададите новый.`)) return;
    await patch(`/api/accesses/${m.id}`, { clear_secret: true });
    load();
  }
  async function remove(m: Access) {
    if (!confirm(`Удалить доступ «${m.label || m.name}» вместе с паролем и всеми выданными правами?`)) return;
    await del(`/api/accesses/${m.id}`);
    load();
  }
  async function decide(a: any, approve: boolean) {
    await post(`/api/approvals/${a.id}/decide`, { approve });
    load();
  }
  async function showLog(m: Access) {
    if (openLog === m.id) return (openLog = null);
    openLog = m.id;
    try {
      logs[m.id] = await get(`/api/accesses/${m.id}/log`);
    } catch (e: any) {
      error = e.message;
    }
  }
  const eventText = (e: any) => {
    const who = agentById(e.actor)?.name ?? (e.actor === 'user' ? 'Вы' : e.actor);
    if (e.kind === 'access.used') return `${who}: ${e.data?.how === 'отказано' ? 'просил, но доступа нет' : 'получил доступ'}${e.data?.reason ? ` — ${e.data.reason}` : ''}`;
    if (e.kind === 'access.granted') return `Вы: ${agentById(e.data?.agent)?.name ?? 'специалисту'} — ${modes.find((x) => x[0] === e.data?.mode)?.[1] ?? e.data?.mode}`;
    if (e.kind === 'access.added') return 'Доступ добавлен';
    if (e.kind === 'access.updated') return 'Доступ изменён';
    return e.kind;
  };
  const standing = (m: Access) => {
    const n = Object.values(m.grants).filter((x) => x === 'allow' || x === 'ask').length;
    return n ? `выдан ${n} из ${app.agents.length}` : 'никому не выдан';
  };
  // Only those who have it are listed; the rest are added from a list. With many specialists this stays short.
  const granted = (m: Access) => app.agents.filter((a) => m.grants[a.id]);
  const without = (m: Access) => app.agents.filter((a) => !m.grants[a.id]);
  async function grantAll(m: Access, mode: string) {
    try {
      await patch(`/api/accesses/${m.id}`, { grants: Object.fromEntries(without(m).map((a) => [a.id, mode])) });
      await load();
    } catch (e: any) {
      error = e.message;
    }
  }
</script>

<Page title="Доступы">
  {#snippet actions()}<button class="btn primary" onclick={() => { adding = !adding; editing = null; form = blank(); }}><Icon name="plus" size={14} />Добавить доступ</button>{/snippet}

  <p class="intro muted">Серверы, базы и сайты, которыми специалисты могут пользоваться в работе. Пароли и ключи хранятся только на этой ноде и нигде не показываются, а в историю чатов не попадают. Вы решаете, кому из специалистов и что выдать: ничего, «спросит каждый раз» или «разрешён».</p>

  {#if error}<div class="notice bad" role="alert">{error}</div>{/if}

  {#if asks.length}
    <div class="notice warn" role="alert">
      <b>Специалисты просят доступ · {asks.length}</b>
      <ul class="plain">
        {#each asks as a (a.id)}
          <li class="ask">
            <Avatar name={agentById(a.agent_id)?.name} color={agentById(a.agent_id)?.accent} size={22} />
            <span class="grow"><b>{agentById(a.agent_id)?.name}</b> просит доступ <code>{a.detail?.args?.name}</code>
              {#if a.detail?.args?.reason}<div class="sub">{a.detail.args.reason}</div>{/if}</span>
            <button class="btn sm primary" onclick={() => decide(a, true)}>Дать один раз</button>
            <button class="btn sm" onclick={() => decide(a, false)}>Отклонить</button>
          </li>
        {/each}
      </ul>
    </div>
  {/if}

  <section class="mcard">
    <header><div><h2>Доступы · {list.length}</h2></div></header>
    <ul class="rows">
      {#each list as m (m.id)}
        <li class="r" class:off={!m.enabled}>
          <div class="line">
            <span class="ic"><Icon name={m.kind === 'site' || m.kind === 'service' ? 'globe' : m.kind === 'database' ? 'server' : 'terminal'} size={17} /></span>
            <div class="grow">
              <div class="ttl"><b>{m.label || m.name}</b><span class="chip mono">{m.name}</span><span class="chip">{kinds[m.kind] ?? m.kind}</span>
                {#if m.has_secret}<span class="chip ok">пароль задан</span>{:else}<span class="chip warn">нет пароля</span>{/if}
                <span class="chip {Object.keys(m.grants).length ? 'accent' : ''}">{standing(m)}</span>
                {#if !m.enabled}<span class="chip">выключено</span>{/if}</div>
              <div class="meta mono">{m.host}{m.login ? ` · ${m.login}` : ''}</div>
              {#if m.note}<div class="meta">{m.note}</div>{/if}
            </div>
            <div class="side">
              <Switch on={m.enabled} label="Включить {m.name}" onchange={(v) => toggle(m, v)} />
              <button class="btn sm" onclick={() => edit(m)}>Изменить</button>
              <button class="icon-btn" onclick={() => remove(m)} aria-label="Удалить {m.name}" title="Удалить"><Icon name="trash" size={15} /></button>
            </div>
          </div>

          <div class="grants" role="group" aria-label="Кому выдан {m.name}">
            <span class="lbl">Кому выдан</span>
            {#each granted(m) as a (a.id)}
              {@const cur = m.grants[a.id]}
              <div class="g">
                <span class="who"><Avatar name={a.name} color={a.accent} size={20} />{a.name}</span>
                <div class="gact">
                  <div class="tabs">{#each modes.filter((x) => x[0] !== 'deny') as [k, l, hint]}<button class:on={cur === k} class:warn={k === 'allow' && cur === k} aria-pressed={cur === k} title={hint} onclick={() => grant(m, a.id, k)}>{l}</button>{/each}</div>
                  <button class="icon-btn" onclick={() => grant(m, a.id, 'deny')} aria-label="Забрать доступ у {a.name}" title="Забрать доступ"><Icon name="x" size={13} /></button>
                </div>
              </div>
            {:else}
              <div class="faint" style="font-size:13px">Пока никому</div>
            {/each}
            {#if without(m).length}
              <div class="add">
                <select class="field" aria-label="Выдать специалисту" onchange={(e) => { const v = e.currentTarget.value; e.currentTarget.value = ''; if (v) grant(m, v, 'ask'); }}>
                  <option value="">Выдать специалисту…</option>
                  {#each without(m) as a (a.id)}<option value={a.id}>{a.name}</option>{/each}
                </select>
                {#if without(m).length > 1}<button class="lnk" onclick={() => grantAll(m, 'ask')} title="Всем остальным: придёт запрос каждый раз">Всем остальным: спросит</button>{/if}
              </div>
            {/if}
          </div>

          <div class="foot">
            <button class="btn sm" onclick={() => showLog(m)} aria-expanded={openLog === m.id}>{openLog === m.id ? 'Скрыть историю' : 'История использования'}</button>
            {#if m.has_secret}<button class="btn sm" onclick={() => dropSecret(m)}>Удалить пароль</button>{/if}
          </div>
          {#if openLog === m.id}
            <ul class="log">
              {#each logs[m.id] ?? [] as e}<li><time class="faint">{dateTime(e.at)}</time><span>{eventText(e)}</span></li>{:else}<li class="faint">Пока никто не пользовался</li>{/each}
            </ul>
          {/if}
          {#if editing === m.id}{@render formView()}{/if}
        </li>
      {:else}
        <li class="empty-note">Доступов пока нет. Добавьте сервер, базу или сайт, а потом выберите, кому из специалистов их выдать.</li>
      {/each}
    </ul>
  </section>

  {#if adding && !editing}
    <section class="mcard"><header><div><h2>Новый доступ</h2></div></header>{@render formView()}</section>
  {/if}
</Page>

{#snippet formView()}
  <form class="mform" onsubmit={(e) => { e.preventDefault(); save(); }}>
    <div class="row2">
      <label>Имя <small>как к нему обращаются специалисты</small><input class="field mono" bind:value={form.name} required placeholder="prod-vps" /></label>
      <label>Название<input class="field" bind:value={form.label} placeholder="Боевой сервер" /></label>
    </div>
    <div class="row2">
      <label>Вид<select class="field" bind:value={form.kind}>{#each Object.entries(kinds) as [k, v]}<option value={k}>{v}</option>{/each}</select></label>
      <label>Адрес<input class="field mono" bind:value={form.host} required placeholder={hostHint[form.kind]} /></label>
    </div>
    <div class="row2">
      <label>Логин<input class="field mono" bind:value={form.login} autocomplete="off" placeholder="deploy" /></label>
      <label>Пароль или ключ <small>{editing ? 'пусто — не менять' : 'сохранится на ноде и больше не покажется'}</small>
        <input class="field mono" type="password" autocomplete="new-password" bind:value={form.secret} placeholder={editing ? '••••••••' : ''} /></label>
    </div>
    <label>Для чего и как им пользоваться <small>это видит специалист</small>
      <textarea class="field" rows="3" bind:value={form.note} placeholder="Деплой сайта: ssh, затем cd /var/www/cruise && git pull. Базу не трогать."></textarea></label>
    <div class="row"><button class="btn primary" type="submit">{editing ? 'Сохранить' : 'Добавить'}</button><button class="btn" type="button" onclick={() => { editing = null; adding = false; }}>Отмена</button></div>
  </form>
{/snippet}

<style>
  .intro { margin: 0 0 16px; max-width: 760px; font-size: 13.5px; line-height: 1.55; }
  .mcard { background: var(--bg-raised); border-radius: var(--radius); box-shadow: var(--shadow-1); margin-bottom: 18px; overflow: hidden; }
  .mcard > header { padding: 14px 18px 10px; }
  .mcard h2 { margin: 0; font-size: 14.5px; font-weight: 650; }
  .rows { list-style: none; margin: 0; padding: 0; }
  .r { padding: 12px 18px; border-top: 1px solid var(--line); }
  .r.off .line, .r.off .grants { opacity: 0.55; }
  .empty-note { padding: 16px 18px; color: var(--fg-3); font-size: 13.5px; border-top: 1px solid var(--line); }
  .line { display: flex; gap: 12px; align-items: flex-start; }
  .ic { width: 34px; height: 34px; border-radius: 9px; background: var(--bg-hover); color: var(--fg-2); display: grid; place-items: center; flex: none; }
  .grow { flex: 1; min-width: 0; }
  .ttl { display: flex; align-items: center; gap: 6px; flex-wrap: wrap; }
  .meta { color: var(--fg-2); font-size: 13px; overflow: hidden; text-overflow: ellipsis; }
  .meta.mono { color: var(--fg-3); font-size: 12px; }
  .side { display: flex; align-items: center; gap: 8px; flex: none; }
  .lbl { font-size: 12px; font-weight: 600; color: var(--fg-3); text-transform: uppercase; letter-spacing: 0.03em; display: block; margin-bottom: 4px; }

  .grants { margin: 12px 0 0 46px; display: flex; flex-direction: column; gap: 6px; max-width: 560px; }
  .g { display: flex; align-items: center; justify-content: space-between; gap: 10px; flex-wrap: wrap; }
  .who { display: inline-flex; align-items: center; gap: 8px; font-size: 13px; }
  .gact { display: inline-flex; align-items: center; gap: 6px; }
  .add { display: flex; align-items: center; gap: 12px; margin-top: 4px; flex-wrap: wrap; }
  .add select { width: 230px; }
  .lnk { color: var(--accent); font-size: 12.5px; }
  .lnk:hover { text-decoration: underline; }
  .tabs { display: inline-flex; padding: 2px; border-radius: 8px; background: var(--bg-hover); gap: 2px; }
  .tabs button { padding: 3px 10px; border-radius: 6px; font-size: 12.5px; color: var(--fg-2); }
  .tabs button:hover { color: var(--fg); }
  .tabs button.on { background: var(--bg-raised); color: var(--fg); box-shadow: var(--shadow-1); font-weight: 600; }
  .tabs button.on.warn { color: var(--warn); }
  .tabs button:focus-visible { outline: none; box-shadow: var(--focus-ring); }

  .foot { display: flex; gap: 8px; margin: 12px 0 0 46px; flex-wrap: wrap; }
  .log { list-style: none; margin: 8px 0 0 46px; padding: 8px 12px; border-radius: var(--radius-s); background: var(--bg-sunken); display: flex; flex-direction: column; gap: 4px; font-size: 12.5px; max-height: 240px; overflow-y: auto; }
  .log li { display: flex; gap: 10px; }
  .log time { flex: none; font-variant-numeric: tabular-nums; }

  .notice { padding: 11px 14px; border-radius: var(--radius); margin: 0 0 16px; font-size: 13.5px; }
  .notice.warn { background: var(--warn-soft); }
  .notice.bad { background: var(--bad-soft); color: var(--bad); }
  .plain { list-style: none; margin: 8px 0 0; padding: 0; display: flex; flex-direction: column; gap: 8px; }
  .ask { display: flex; gap: 10px; align-items: flex-start; }
  .sub { color: var(--fg-2); font-size: 12.5px; }
  code { font: 12px var(--mono); padding: 1px 5px; border-radius: 5px; background: var(--bg-hover); }
  .mform { display: flex; flex-direction: column; gap: 12px; padding: 14px 18px 16px; }
  .mform label { display: flex; flex-direction: column; gap: 4px; font-size: 13px; font-weight: 600; }
  .mform label small { font-weight: 400; color: var(--fg-3); }
  .row2 { display: grid; grid-template-columns: 1fr 1fr; gap: 12px; }
  @media (max-width: 760px) { .row2 { grid-template-columns: 1fr; } .grants, .foot, .log { margin-left: 0; } .line { flex-wrap: wrap; } }
</style>
