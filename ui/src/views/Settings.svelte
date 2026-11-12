<script lang="ts">
  import { del, disconnect, get, hubUrl, post, put } from '../lib/api';
  import { app, applyTheme } from '../lib/store.svelte';
  import { dateTime } from '../lib/format';
  import { modelOptions } from '../lib/models';
  import type { Account, PermissionRule } from '../lib/types';
  import BrandMark from '../components/BrandMark.svelte';
  import Dropdown from '../components/Dropdown.svelte';
  import Icon from '../components/Icon.svelte';
  import Page from '../components/Page.svelte';

  let s = $state<any>(null);
  let grants = $state<{ grants: any[]; actions: string[] } | null>(null);
  let rules = $state<PermissionRule[]>([]);
  async function dropRule(id: string) {
    try {
      await del(`/api/permission-rules/${id}`);
      rules = await get('/api/permission-rules');
    } catch (e: any) {
      error = e.message;
    }
  }
  let codes = $state<Record<string, string>>({});
  let showModels = $state<Record<string, boolean>>({});
  let busy = $state<Record<string, boolean>>({});
  let error = $state('');
  let copied = $state('');

  async function load() {
    s = await get('/api/settings');
    grants = await get('/api/grants');
    rules = await get('/api/permission-rules');
  }
  $effect(() => {
    load().catch((e) => (error = e.message));
  });

  const install: Record<string, string> = { claude: 'npm i -g @anthropic-ai/claude-code', codex: 'npm i -g @openai/codex', kimi: 'npm i -g @moonshot-ai/kimi-code' };
  const isDesktop = () => !!(window as any).__SENATE__;

  async function act(p: string, fn: () => Promise<any>) {
    busy[p] = true;
    error = '';
    try {
      const r = await fn();
      if (Array.isArray(r)) app.accounts = r;
    } catch (e: any) {
      error = e.message;
    } finally {
      busy[p] = false;
    }
  }
  const login = (a: Account) => act(a.provider, () => post(`/api/accounts/${a.provider}/login`));
  const cancel = (a: Account) => act(a.provider, () => del(`/api/accounts/${a.provider}/login`));
  const logout = (a: Account) => confirm(`Выйти из ${a.label}? Специалисты на этой модели перейдут на модель по умолчанию.`) && act(a.provider, () => del(`/api/accounts/${a.provider}`));
  const check = (a: Account) => act(a.provider, () => post(`/api/accounts/${a.provider}/check`));
  const sendCode = (a: Account) =>
    act(a.provider, async () => {
      await post(`/api/accounts/${a.provider}/login/code`, { code: codes[a.provider] });
      codes[a.provider] = '';
    });

  async function openLink(a: Account) {
    const url = a.login?.url;
    if (!url) return;
    if (isDesktop()) await act(a.provider, () => post(`/api/accounts/${a.provider}/open`));
    else window.open(url, '_blank', 'noopener');
  }
  function copy(text: string, key: string) {
    navigator.clipboard?.writeText(text).then(() => {
      copied = key;
      setTimeout(() => (copied = ''), 1200);
    });
  }

  const defaultModels = $derived(modelOptions(app.accounts, { includeDemo: true }));
  async function setDefault(v: string) {
    s = await put('/api/settings', { default_backend: v });
    app.defaultBackend = s.effective_backend;
  }
  async function setLearn(v: string) {
    s = await put('/api/settings', { learn_level: v });
  }
  const learnLevels = [['off', 'Выкл'], ['low', 'Минимум'], ['medium', 'Умеренно'], ['high', 'Максимум']];
  async function rescan() {
    s = await put('/api/settings', { rescan: true });
    app.accounts = await get('/api/accounts');
  }

  const modeOf = (action: string) => grants?.grants.find((g) => !g.agent_id && !g.project_id && g.action === action)?.mode ?? null;
  async function setGrant(action: string, mode: string) {
    await put('/api/grants', { action, mode });
    grants = await get('/api/grants');
  }
  const actionLabel: Record<string, [string, string]> = {
    'message.post': ['Писать сообщения', 'auto'],
    'memory.write': ['Записывать в память', 'auto'],
    'memory.promote': ['Переносить знание в общую память', 'approve'],
    'obligation.update': ['Менять свои задачи', 'auto'],
    'obligation.create': ['Принимать на себя задачи', 'auto'],
    'obligation.delegate': ['Ставить задачи коллегам', 'auto'],
    'obligation.manage': ['Закрывать и передавать чужие задачи проекта', 'approve'],
    'colleague.call': ['Звать коллег в разговор', 'auto'],
    'decision.propose': ['Предлагать решения', 'auto'],
    'decision.accept': ['Принимать решения', 'approve'],
    'artifact.write': ['Сохранять артефакты', 'auto'],
    'external.send': ['Писать во внешние сервисы', 'approve'],
    'profile.change': ['Менять профили', 'approve'],
    'project.create': ['Заводить проекты', 'approve'],
    'mcp.install': ['Устанавливать MCP', 'approve'],
    'api.connect': ['Подключать API', 'approve'],
  };
  const modes = [['auto', 'Сам'], ['approve', 'С согласия'], ['deny', 'Нельзя']];
  const themes = [['auto', 'Как в системе'], ['light', 'Светлая'], ['dark', 'Тёмная']];
  const windowLabel: Record<string, string> = { five_hour: '5 часов', seven_day: 'Неделя', seven_day_opus: 'Неделя · Opus', seven_day_sonnet: 'Неделя · Sonnet' };
  const resets = (sec: number) => (sec ? dateTime(sec * 1000) : '');
</script>

<Page title="Настройки">
  {#if error}<div class="notice bad" role="alert">{error}</div>{/if}
  {#if s}
    <section class="block">
      <h2>Агенты</h2>
      <div class="agents">
        {#each app.accounts as a (a.provider)}
          <article class="acc" data-state={a.state}>
            <div class="head">
              <span class="logo"><BrandMark id={a.provider} size={22} tint /></span>
              <div class="grow">
                <div class="nm"><b>{a.label}</b>
                  {#if a.state === 'ready'}<span class="chip ok">подключён</span>{:else if a.state === 'pending'}<span class="chip accent">вход…</span>{:else if !a.installed}<span class="chip">не установлен</span>{:else}<span class="chip warn">нужен вход</span>{/if}
                  {#if a.version}<span class="chip mono">{a.version.replace(/\s*\(.*\)/, '')}</span>{/if}
                  {#if a.update_available}<span class="chip warn" title="Список моделей берётся из CLI: устаревший CLI может его не знать">доступна {a.latest_version}</span>{/if}</div>
                {#if a.account}<div class="sub">{a.account}</div>{/if}
              </div>
              <div class="btns">
                {#if !a.installed}
                  <button class="btn sm" onclick={() => copy(install[a.provider], `i-${a.provider}`)}><Icon name={copied === `i-${a.provider}` ? 'check' : 'copy'} size={13} />{install[a.provider]}</button>
                {:else if a.state === 'ready'}
                  <button class="btn sm" onclick={() => check(a)} disabled={busy[a.provider]}><Icon name="refresh" size={13} />Обновить</button>
                  <button class="btn sm danger" onclick={() => logout(a)} disabled={busy[a.provider]}>Выйти</button>
                {:else if a.state === 'pending'}
                  <button class="btn sm" onclick={() => cancel(a)}>Отменить</button>
                {:else}
                  <button class="btn sm primary" onclick={() => login(a)} disabled={busy[a.provider]}>Войти</button>
                {/if}
              </div>
            </div>

            {#if a.login}
              <div class="flow" data-state={a.login.state}>
                {#if a.login.state === 'failed'}
                  <div class="msg bad">{a.login.message}</div>
                {:else}
                  {#if a.login.message}<div class="msg">{a.login.message}</div>{/if}
                  {#if a.login.url}
                    <div class="link">
                      <span class="mono url">{a.login.url}</span>
                      <button class="btn sm primary" onclick={() => openLink(a)}><Icon name="link" size={13} />Открыть</button>
                      <button class="icon-btn" onclick={() => copy(a.login!.url, `u-${a.provider}`)} aria-label="Копировать ссылку"><Icon name={copied === `u-${a.provider}` ? 'check' : 'copy'} size={14} /></button>
                    </div>
                  {/if}
                  {#if a.login.code}
                    <div class="code">
                      <span class="lbl">Код</span><b class="mono">{a.login.code}</b>
                      <button class="icon-btn" onclick={() => copy(a.login!.code, `c-${a.provider}`)} aria-label="Копировать код"><Icon name={copied === `c-${a.provider}` ? 'check' : 'copy'} size={14} /></button>
                    </div>
                  {/if}
                  {#if a.login.needs_code && a.login.url}
                    <form class="paste" onsubmit={(e) => { e.preventDefault(); sendCode(a); }}>
                      <input class="field mono" bind:value={codes[a.provider]} placeholder="Код со страницы" autocomplete="off" />
                      <button class="btn sm primary" type="submit" disabled={!codes[a.provider]?.trim() || busy[a.provider]}>Подтвердить</button>
                    </form>
                  {/if}
                {/if}
              </div>
            {/if}

            {#if a.state === 'ready'}
              <div class="foot">
                <button class="lnk" onclick={() => (showModels[a.provider] = !showModels[a.provider])}>{a.models.length} моделей <Icon name={showModels[a.provider] ? 'down' : 'right'} size={12} /></button>
                {#if a.limits?.windows}
                  <div class="limits">
                    {#each Object.entries(a.limits.windows) as [k, w]}
                      <div class="lim" title={w.resets_at ? `Обновится ${resets(w.resets_at)}` : ''}>
                        <span>{windowLabel[k] ?? k}</span>
                        <div class="bar"><i class:hot={w.used >= 80} style="width:{Math.min(100, w.used)}%"></i></div>
                        <b>{w.used}%</b>
                      </div>
                    {/each}
                  </div>
                {/if}
              </div>
              {#if showModels[a.provider]}<div class="models">{#each a.models as m}<span class="chip">{m.label}</span>{/each}</div>{/if}
            {/if}
          </article>
        {/each}
      </div>
      <div class="row" style="margin-top:10px"><button class="btn" onclick={rescan}><Icon name="refresh" size={14} />Найти заново</button></div>
    </section>

    <section class="block">
      <h2>По умолчанию</h2>
      <div class="rowset">
        <div class="rs"><span>Модель для чатов без основной модели</span>
          <div style="width:300px;max-width:100%"><Dropdown block value={s.effective_backend} options={defaultModels} label="Модель по умолчанию" onpick={setDefault} /></div></div>
        <div class="rs"><span>Уровень обучения</span><div class="tabs">{#each learnLevels as [k, l]}<button class:on={s.learn_level === k} onclick={() => setLearn(k)}>{l}</button>{/each}</div></div>
        <div class="rs"><span>Оформление</span><div class="tabs">{#each themes as [k, l]}<button class:on={app.theme === k} aria-pressed={app.theme === k} onclick={() => applyTheme(k as 'auto' | 'light' | 'dark')}>{l}</button>{/each}</div></div>
      </div>
    </section>

    <section class="block">
      <h2>Что специалисты делают сами</h2>
      <div class="rowset">
        {#each grants?.actions ?? [] as a (a)}
          {@const cur = modeOf(a) ?? actionLabel[a]?.[1] ?? 'deny'}
          <div class="rs"><span>{actionLabel[a]?.[0] ?? a}</span><div class="tabs">{#each modes as [k, l]}<button class:on={cur === k} aria-pressed={cur === k} onclick={() => setGrant(a, k)}>{l}</button>{/each}</div></div>
        {/each}
      </div>
    </section>

    <section class="block">
      <h2>Разрешено навсегда</h2>
      <p class="muted" style="margin:0 0 10px;font-size:12.5px">Сюда попадает то, на что вы нажали «Разрешать всегда». По этим правилам специалисты больше не спрашивают. Опасное (`git push`, удаление, `ssh`) и планы запомнить нельзя.</p>
      <div class="rowset">
        {#each rules as r (r.id)}
          <div class="rs"><span>{r.label}</span><button class="btn sm" onclick={() => dropRule(r.id)} aria-label={`Больше не разрешать: ${r.label}`}>Убрать</button></div>
        {:else}
          <div class="rs"><span class="muted">Пока ничего. Нажмите «Разрешать всегда» в запросе разрешения.</span></div>
        {/each}
      </div>
    </section>

    <section class="block">
      <h2>Нода</h2>
      <dl class="card pad kv">
        <dt>Адрес</dt><dd class="mono">{hubUrl()}</dd>
        <dt>Версия</dt><dd>{app.version}</dd>
        <dt>Данные</dt><dd class="mono">{s.data_dir}</dd>
        <dt>Связь</dt><dd>{app.online ? 'на связи' : 'нет связи'}</dd>
      </dl>
      <p style="margin-top:10px"><button class="btn" onclick={() => { disconnect(); location.reload(); }}>Отключиться от этой ноды</button></p>
    </section>
  {:else}
    <p class="muted">Загрузка…</p>
  {/if}
</Page>

<style>
  .block {
    margin-bottom: 28px;
  }
  .block > h2 {
    margin: 0 0 10px;
    font-size: 13px;
    font-weight: 600;
    color: var(--fg-2);
  }
  .notice.bad {
    padding: 10px 14px;
    border-radius: var(--radius);
    background: var(--bad-soft);
    color: var(--bad);
    margin-bottom: 14px;
  }
  .agents {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .acc {
    background: var(--bg-raised);
    border-radius: var(--radius);
    box-shadow: var(--shadow-1);
    padding: 14px 16px;
  }
  .acc[data-state='ready'] {
    box-shadow:
      var(--shadow-1),
      inset 0 0 0 1px color-mix(in srgb, var(--ok) 25%, transparent);
  }
  .head {
    display: flex;
    gap: 12px;
    align-items: center;
  }
  .logo {
    width: 38px;
    height: 38px;
    border-radius: 10px;
    background: var(--bg-hover);
    display: grid;
    place-items: center;
    flex: none;
  }
  .grow {
    flex: 1;
    min-width: 0;
  }
  .nm {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-wrap: wrap;
  }
  .nm b {
    font-size: 15px;
  }
  .sub {
    color: var(--fg-2);
    font-size: 13px;
  }
  .btns {
    display: flex;
    gap: 6px;
    flex: none;
  }
  .flow {
    margin-top: 12px;
    padding: 12px 14px;
    border-radius: var(--radius);
    background: var(--bg-sunken);
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .msg {
    font-size: 13.5px;
  }
  .msg.bad {
    color: var(--bad);
  }
  .link {
    display: flex;
    gap: 8px;
    align-items: center;
  }
  .url {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 12.5px;
    color: var(--fg-2);
  }
  .code {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .code b {
    font-size: 22px;
    letter-spacing: 0.08em;
  }
  .lbl {
    font-size: 12px;
    color: var(--fg-3);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .paste {
    display: flex;
    gap: 8px;
    max-width: 420px;
  }
  .foot {
    display: flex;
    align-items: center;
    gap: 16px;
    margin-top: 12px;
    flex-wrap: wrap;
  }
  .lnk {
    color: var(--accent);
    font-size: 13px;
    display: inline-flex;
    align-items: center;
    gap: 4px;
  }
  .limits {
    display: flex;
    gap: 14px;
    flex-wrap: wrap;
    margin-left: auto;
  }
  .lim {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 12.5px;
    color: var(--fg-2);
  }
  .bar {
    width: 90px;
    height: 5px;
    border-radius: 3px;
    background: var(--bg-active);
    overflow: hidden;
  }
  .bar i {
    display: block;
    height: 100%;
    background: var(--accent);
    border-radius: 3px;
  }
  .bar i.hot {
    background: var(--warn);
  }
  .models {
    display: flex;
    flex-wrap: wrap;
    gap: 5px;
    margin-top: 10px;
  }
  .rowset {
    background: var(--bg-raised);
    border-radius: var(--radius);
    box-shadow: var(--shadow-1);
  }
  .rs {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    padding: 10px 16px;
    min-height: 48px;
  }
  .rs + .rs {
    border-top: 1px solid var(--line);
  }
  @media (max-width: 980px) { .head, .rs { flex-wrap: wrap; } .btns { flex-wrap: wrap; } }
  @media (max-width: 760px) { .rs { gap: 10px; } .head { align-items: flex-start; } .tabs { flex-wrap: wrap; } .btns { max-width: 100%; } .btns .btn { white-space: normal; height: auto; min-height: 30px; overflow-wrap: anywhere; } }
</style>
