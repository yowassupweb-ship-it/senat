<script lang="ts">
  import { get, patch, post, put, upload } from '../lib/api';
  import { squarePng } from '../lib/image';
  import { app, go } from '../lib/store.svelte';
  import { N, ROLE_TITLE, STATE_LABEL, count, dots, roleLabel, timeAgo } from '../lib/format';
  import { modelOptions, modelTitle } from '../lib/models';
  import type { Memory, Obligation, Project } from '../lib/types';
  import Avatar from '../components/Avatar.svelte';
  import BrandMark from '../components/BrandMark.svelte';
  import Dropdown from '../components/Dropdown.svelte';
  import Icon from '../components/Icon.svelte';
  import Page from '../components/Page.svelte';
  import RoutineList from '../components/RoutineList.svelte';
  import Switch from '../components/Switch.svelte';

  let { id }: { id?: string } = $props();

  // Where each specialist works: on the node, or on this person's own computer (the Senate app runs the turns).
  let exec = $state<{ modes: Record<string, string>; runner_online: boolean }>({ modes: {}, runner_online: false });
  async function loadExec() {
    try {
      exec = await get('/api/exec-modes');
    } catch {
      /* an older node without the setting */
    }
  }
  let execError = $state('');
  async function setExec(agentId: string, mode: 'node' | 'local') {
    execError = '';
    try {
      exec = await put('/api/exec-modes', { agent_id: agentId, mode });
    } catch (e: any) {
      execError = String(e.message || e).includes('JSON') ? 'Эта нода ещё не обновлена и не умеет переключать режим: она сейчас пересобирается, повторите через несколько минут.' : e.message;
    }
  }
  $effect(() => {
    loadExec();
    const t = setInterval(loadExec, 15000);
    return () => clearInterval(t);
  });

  let creating = $state(false);
  let draft = $state({ name: '', role: 'custom', profile: '', model_pref: '' });
  let error = $state('');

  type Sheet = {
    agent: any;
    stats: any;
    projects: Project[];
    obligations: Obligation[];
    memory: Memory[];
    profile_versions: any[];
    equipment: { mcp: any[]; api: any[] };
    skills?: { name: string; description: string; dir: string; routines: number }[];
  };
  let sheet = $state<Sheet | null>(null);
  let profile = $state('');
  let note = $state('');

  async function load() {
    if (!id) return (sheet = null);
    try {
      sheet = await get(`/api/agents/${id}/sheet`);
      profile = sheet!.agent.profile;
      note = '';
    } catch (e: any) {
      error = e.message;
    }
  }
  $effect(() => {
    id;
    load();
  });
  // The first load of the app carries profiles only; level and open work come with the full list.
  $effect(() => {
    get('/api/agents').then((a) => (app.agents = a)).catch(() => {});
  });

  // The specialist's picture: cut square, stored by the node, then shown everywhere by name.
  let photoInput = $state<HTMLInputElement>();
  let photoBusy = $state(false);
  async function setPhoto(file: File | undefined) {
    if (!file || !sheet) return;
    photoBusy = true;
    error = '';
    try {
      const up = await upload(await squarePng(file, 256), 'avatar.png');
      await patch(`/api/agents/${sheet.agent.id}`, { avatar_id: up.id });
      await refreshList();
      await load();
    } catch (e: any) {
      error = e.message || 'Не удалось поставить фото';
    } finally {
      photoBusy = false;
    }
  }
  async function clearPhoto() {
    if (!sheet) return;
    photoBusy = true;
    try {
      await patch(`/api/agents/${sheet.agent.id}`, { avatar_id: null });
      await refreshList();
      await load();
    } catch (e: any) {
      error = e.message;
    } finally {
      photoBusy = false;
    }
  }

  async function refreshList() {
    app.agents = await get('/api/agents');
  }
  async function create() {
    try {
      const a = await post('/api/agents', draft);
      await refreshList();
      creating = false;
      draft = { name: '', role: 'custom', profile: '', model_pref: '' };
      go({ kind: 'agents', id: a.id });
    } catch (e: any) {
      error = e.message;
    }
  }
  async function saveProfile() {
    await post(`/api/agents/${id}/profile`, { profile, note: note || 'правка пользователя' });
    await refreshList();
    load();
  }
  async function setModel(v: string) {
    await patch(`/api/agents/${id}`, { model_pref: v });
    await refreshList();
    load();
  }
  // Name and @username, by hand. The username is what `@` addresses; a stored one is Latin only.
  let renaming = $state(false);
  let rn = $state({ name: '', slug: '', gender: '' });
  const GENDER: Record<string, string> = { '': 'Не указан', m: 'Мужской', f: 'Женский' };
  let renameError = $state('');
  let renameBusy = $state(false);
  function startRename() {
    if (!sheet) return;
    rn = { name: sheet.agent.name, slug: sheet.agent.slug, gender: sheet.agent.gender ?? '' };
    renameError = '';
    renaming = true;
  }
  async function saveRename(e: Event) {
    e.preventDefault();
    if (!sheet) return;
    renameBusy = true;
    renameError = '';
    try {
      const body: Record<string, string> = {};
      if (rn.name.trim() !== sheet.agent.name) body.name = rn.name.trim();
      const slug = rn.slug.trim().replace(/^@/, '').toLowerCase();
      if (slug !== sheet.agent.slug) body.slug = slug;
      if (rn.gender !== (sheet.agent.gender ?? '')) body.gender = rn.gender;
      if (Object.keys(body).length) {
        await patch(`/api/agents/${sheet.agent.id}`, body);
        await refreshList();
        await load();
      }
      renaming = false;
    } catch (err: any) {
      renameError = err.message || 'Не удалось сохранить';
    } finally {
      renameBusy = false;
    }
  }
  async function setAccent(c: string) {
    await patch(`/api/agents/${id}`, { accent: c });
    await refreshList();
    load();
  }
  async function toggleEquip(kind: 'mcp' | 'api', itemId: string, on: boolean) {
    if (!sheet) return;
    const cur = sheet.equipment[kind].filter((x) => x.equipped && !x.via_shared).map((x) => x.id);
    const next = on ? [...cur, itemId] : cur.filter((x) => x !== itemId);
    await put(`/api/agents/${id}/equipment`, { [kind]: next });
    load();
  }
  async function chat() {
    const t = await post('/api/threads', { agent_id: id });
    app.threads.unshift(t);
    app.messages[t.id] = [];
    go({ kind: 'chat', id: t.id });
  }
  async function retire() {
    if (!confirm('Вывести специалиста из активной работы? Его история сохранится.')) return;
    try {
      await post(`/api/agents/${id}/retire`);
      await refreshList();
      go({ kind: 'agents' });
    } catch (e: any) {
      error = e.message;
    }
  }

  const models = $derived(modelOptions(app.accounts, { withDefault: true, defaultLabel: 'Решает среда', includeDemo: true }));
  const roles = Object.entries(ROLE_TITLE);
  // One tone per specialist, all of a single weight so a team never reads as a rainbow.
  const accents = ['#6f8fbf', '#7fa38c', '#b2927a', '#a98bb5', '#8e9aa8', '#b59a6a', '#a8848a', '#7f98a8', '#97a37f', '#8e8e93'];

  const st = $derived(sheet?.stats);
  const xpPct = $derived(st ? Math.min(100, Math.round(((st.xp - (st.level - 1) ** 2 * 6) / Math.max(1, st.next_level_xp - (st.level - 1) ** 2 * 6)) * 100)) : 0);
  const tiles = $derived(
    st
      ? [
          { v: st.obligations?.done ?? 0, l: 'Задач выполнено', icon: 'check', hint: '' },
          { v: st.reliability == null ? '—' : `${st.reliability}%`, l: 'Надёжность', icon: 'shield', hint: 'доля завершённых среди закрытых задач' },
          { v: Object.values(st.memory ?? {}).reduce((a: number, b: any) => a + b, 0), l: 'Знаний в памяти', icon: 'book', hint: '' },
          { v: st.memory?.verified ?? 0, l: 'Проверенных', icon: 'star', hint: '' },
          { v: st.corrections ?? 0, l: 'Исправлений', icon: 'refresh', hint: 'исправленных собственных записей' },
          { v: st.sessions ?? 0, l: 'Сессий работы', icon: 'bolt', hint: '' },
        ]
      : [],
  );
  /** How far the specialist is from the next level, 0..1 (a level starts at 6 * (level-1)^2 experience). */
  const xpFraction = (a: any) => {
    const lv = a.level ?? 1;
    const from = (lv - 1) ** 2 * 6;
    const to = a.next_level_xp ?? from + 6;
    return Math.max(0, Math.min(1, ((a.xp ?? 0) - from) / Math.max(1, to - from)));
  };
  const kindLabel: Record<string, string> = { observation: 'наблюдение', hypothesis: 'гипотеза', verified: 'проверено', decision: 'решение', preference: 'предпочтение', error: 'ошибка' };
  const companyName = (cid?: string | null) => app.companies.find((c) => c.id === cid);
</script>

{#if !id}
  <Page title="Специалисты">
    {#snippet actions()}<button class="btn primary" onclick={() => (creating = !creating)}><Icon name="plus" size={14} />Новый специалист</button>{/snippet}
    {#if creating}
      <form class="card pad form" style="margin-bottom:18px" onsubmit={(e) => { e.preventDefault(); create(); }}>
        <div class="row">
          <label style="flex:1">Имя<input class="field" bind:value={draft.name} required placeholder="Например, Редактор" /></label>
          <label style="width:200px">Класс
            <Dropdown block value={draft.role} options={roles.map(([k, v]) => ({ id: k, label: v }))} label="Роль" onpick={(v) => (draft.role = v)} /></label>
        </div>
        <label>Основная модель
          <Dropdown block value={draft.model_pref} options={models} label="Основная модель" onpick={(v) => (draft.model_pref = v)} /></label>
        <label>Личность
          <textarea class="field" rows="4" bind:value={draft.profile}></textarea></label>
        <div class="row"><button class="btn primary" type="submit">Создать</button><button class="btn" type="button" onclick={() => (creating = false)}>Отмена</button></div>
        {#if error}<p class="bad">{error}</p>{/if}
      </form>
    {/if}
    <div class="cards">
      {#each app.agents as a (a.id)}
        <button class="card click hero-card" onclick={() => go({ kind: 'agents', id: a.id })} style="--c:{a.accent}">
          <div class="top">
            <span class="port"><Avatar name={a.name} color={a.accent} size={60} progress={xpFraction(a)} /><span class="lvl" title="Уровень">{a.level ?? 1}</span></span>
            <div class="who">
              <b>{a.name}</b>
              {#if roleLabel(a)}<span class="muted">{roleLabel(a)}</span>{/if}
            </div>
          </div>
          <p class="muted clamp">{a.profile}</p>
          <div class="meta">
            <span class="chip">{#if a.model_pref}<BrandMark id={a.model_pref} size={12} tint />{modelTitle(app.accounts, a.model_pref)}{:else}Модель по умолчанию{/if}</span>
            {#if a.open_obligations}<span class="chip">{count(a.open_obligations, N.task)}</span>{/if}
            {#if a.running}<span class="chip accent">{a.running} в работе</span>{/if}
            {#if a.blocked}<span class="chip bad">{a.blocked} заблок.</span>{/if}
          </div>
        </button>
      {/each}
    </div>
  </Page>
{:else if sheet}
  {@const a = sheet.agent}
  <Page title="" back={{ label: 'Специалисты', onclick: () => go({ kind: 'agents' }) }}>
    {#snippet actions()}
      <button class="btn primary" onclick={chat}><Icon name="chat" size={14} />Написать</button>
    {/snippet}

    <section class="hero" style="--c:{a.accent}">
      <div class="portcol">
        <div class="port big"><Avatar name={a.name} color={a.accent} size={92} progress={xpFraction({ ...a, level: st?.level, xp: st?.xp, next_level_xp: st?.next_level_xp })} /><span class="lvl" title="Уровень">{st?.level ?? 1}</span></div>
        <input class="pick" type="file" accept="image/png,image/jpeg,image/gif,image/webp" bind:this={photoInput} onchange={(e) => { setPhoto(e.currentTarget.files?.[0]); e.currentTarget.value = ''; }} tabindex="-1" aria-hidden="true" />
        <div class="photoact">
          <button class="btn sm" type="button" onclick={() => photoInput?.click()} disabled={photoBusy} aria-busy={photoBusy}>{a.avatar ? 'Сменить фото' : 'Поставить фото'}</button>
          {#if a.avatar}<button class="btn sm" type="button" onclick={clearPhoto} disabled={photoBusy} aria-label="Убрать фото">Убрать</button>{/if}
        </div>
      </div>
      <div class="hbody">
        {#if renaming}
          <form class="rename" onsubmit={saveRename}>
            <label>Имя<input class="field" bind:value={rn.name} required maxlength="60" /></label>
            <label>Username<span class="at"><i>@</i><input class="field" bind:value={rn.slug} required minlength="2" maxlength="32" pattern="[A-Za-z0-9_\-]+" autocapitalize="off" spellcheck="false" /></span></label>
            <div class="gender" role="radiogroup" aria-label="Пол">
              {#each Object.entries(GENDER) as [k, label]}
                <button type="button" role="radio" aria-checked={rn.gender === k} class:on={rn.gender === k} onclick={() => (rn.gender = k)}>{label}</button>
              {/each}
            </div>
            {#if renameError}<p class="badc" role="alert">{renameError}</p>{/if}
            <div class="row" style="gap:6px"><button class="btn primary sm" type="submit" disabled={renameBusy || !rn.name.trim() || !rn.slug.trim()} aria-busy={renameBusy}>Сохранить</button><button class="btn sm" type="button" onclick={() => (renaming = false)}>Отмена</button></div>
          </form>
        {:else}
          <div class="nm"><h1>{a.name}</h1>{#if roleLabel(a)}<span class="chip">{roleLabel(a)}</span>{/if}<button class="icon-btn" onclick={startRename} aria-label="Изменить имя и username" title="Изменить имя и username"><Icon name="pencil" size={14} /></button></div>
          <div class="faint handle">{dots('@' + a.slug, a.gender && GENDER[a.gender])}</div>
        {/if}
        <div class="row" style="gap:6px;flex-wrap:wrap">
          <span class="chip">{#if a.model_pref}<BrandMark id={a.model_pref} size={12} tint />{/if}{modelTitle(app.accounts, a.model_pref)}</span>
          <span class="chip">{count(sheet.projects.length, N.project)}</span>
          <span class="chip">{sheet.obligations.length} открытых задач</span>
        </div>
        <div class="xp" title="Опыт считается по проверяемому: выполненные задачи, проверенные знания, исправленные ошибки, сессии">
          <div class="bar"><i style="width:{xpPct}%"></i></div>
          <span class="faint">Уровень {st?.level ?? 1} · опыт {st?.xp ?? 0} из {st?.next_level_xp ?? 0}</span>
        </div>
      </div>
      <div class="swatches" aria-label="Цвет специалиста">
        {#each accents as c}<button class="sw" class:on={a.accent === c} style="--sw:{c}" onclick={() => setAccent(c)} aria-label="Цвет {c}" aria-pressed={a.accent === c}></button>{/each}
      </div>
    </section>

    <div class="sheet">
      <div class="col">
        <div class="section">
          <h2>Где работает</h2>
          <div class="card pad where">
            <div class="tabs" role="radiogroup" aria-label="Где работает специалист">
              <button role="radio" aria-checked={(exec.modes[a.id] ?? 'node') === 'node'} class:on={(exec.modes[a.id] ?? 'node') === 'node'} onclick={() => setExec(a.id, 'node')}><Icon name="server" size={14} />На ноде</button>
              <button role="radio" aria-checked={exec.modes[a.id] === 'local'} class:on={exec.modes[a.id] === 'local'} onclick={() => setExec(a.id, 'local')}><Icon name="monitor" size={14} />Локально</button>
            </div>
            <p class="muted" style="margin:10px 0 0">
              {#if exec.modes[a.id] === 'local'}
                Ходы этого специалиста выполняет приложение Senate на вашем компьютере: его папки, его программы и доступы, его тестовые серверы. Память, чаты и задачи остаются на ноде.
                <b class={exec.runner_online ? 'okc' : 'badc'}>{exec.runner_online ? 'Приложение на компьютере подключено.' : 'Приложение на компьютере не подключено: пока оно не запущено, специалист работать не сможет.'}</b>
              {:else}
                Специалист работает на ноде: в её папках и с её программами. Тестовые серверы, которые он запускает, видны только на ноде.
              {/if}
            </p>
            {#if execError}<p class="badc" role="alert" style="margin:8px 0 0">{execError}</p>{/if}
          </div>
        </div>

        <div class="section">
          <h2>Характеристики</h2>
          <div class="tiles">
            {#each tiles as t}
              <div class="tile card" title={t.hint}><span class="ti"><Icon name={t.icon} size={14} /></span><b>{t.v}</b><span class="muted">{t.l}</span></div>
            {/each}
          </div>
        </div>

        <div class="section">
          <h2>Личность</h2>
          <div class="card pad form">
            <textarea class="field" rows="6" bind:value={profile} aria-label="Личность специалиста: как он работает и чем руководствуется"></textarea>
            <div class="row">
              <input class="field" bind:value={note} placeholder="Что изменили и почему (попадёт в историю)" />
              <button class="btn primary" disabled={profile === a.profile} onclick={saveProfile}>Сохранить</button>
            </div>
          </div>
        </div>

        <div class="section">
          <h2>Опыт по компаниям <span class="count">{st?.companies?.length ?? 0}</span></h2>
          <ul class="list">
            {#each st?.companies ?? [] as c (c.id)}
              <li><button class="item" onclick={() => go({ kind: 'companies', id: c.id })}>
                <i class="cdot" style="background:{c.color}"></i>
                <span class="grow"><span class="title">{c.name}</span>
                  <div class="sub">знаний о компании: {c.own_memory} · общих: {c.shared_memory} · решений: {c.decisions}</div></span>
                <span class="chip">{count(c.done, N.task)}</span><span class="chip">{count(c.projects, N.project)}</span></button></li>
            {:else}<li class="empty-note">Пока нет</li>{/each}
          </ul>
        </div>

        <div class="section">
          <h2>Знания <span class="count">{sheet.memory.length}</span></h2>
          <ul class="list">
            {#each sheet.memory.slice(0, 20) as m (m.id)}
              <li class="item" style="align-items:flex-start">
                <span class="grow"><span style="white-space:pre-wrap">{m.body}</span>
                  <div class="sub" style="white-space:normal">{#if m.company_id}{companyName(m.company_id)?.name ?? 'компания'} · {:else}общее мастерство · {/if}{m.source.startsWith('auto:') ? 'извлечено средой · ' : ''}{timeAgo(m.updated_at)}</div></span>
                <span class="chip {m.kind === 'verified' ? 'ok' : m.kind === 'error' ? 'bad' : m.kind === 'hypothesis' ? 'warn' : ''}">{kindLabel[m.kind]}</span>
              </li>
            {:else}<li class="empty-note">Пока ничего</li>{/each}
          </ul>
        </div>
      </div>

      <div class="col side">
        <div class="section">
          <h2>Основная модель</h2>
          <div class="card pad form">
            <Dropdown block value={a.model_pref} options={models} label="Основная модель" onpick={setModel} />
          </div>
        </div>

        <div class="section">
          <h2>Инструменты MCP <button class="lnk" onclick={() => go({ kind: 'mcp' })}>все серверы</button></h2>
          <ul class="list">
            {#each sheet.equipment.mcp as m (m.id)}
              <li class="item slot">
                <Icon name="plug" size={15} />
                <span class="grow"><span class="title">{m.name}</span><div class="sub">{m.note || (m.kind === 'http' ? m.url : m.command)}</div></span>
                {#if m.via_shared}<span class="chip">всем</span>{/if}
                <Switch on={m.equipped} disabled={m.via_shared} label="Выдать {m.name}" onchange={(v) => toggleEquip('mcp', m.id, v)} />
              </li>
            {:else}<li class="empty-note">Нет серверов</li>{/each}
          </ul>
        </div>

        <div class="section">
          <h2>Подключения API <button class="lnk" onclick={() => go({ kind: 'api' })}>все подключения</button></h2>
          <ul class="list">
            {#each sheet.equipment.api as m (m.id)}
              <li class="item slot">
                <Icon name="link" size={15} />
                <span class="grow"><span class="title">{m.label || m.name}</span><div class="sub">{m.base_url}</div></span>
                {#if m.via_shared}<span class="chip">всем</span>{/if}
                <Switch on={m.equipped} disabled={m.via_shared} label="Выдать {m.name}" onchange={(v) => toggleEquip('api', m.id, v)} />
              </li>
            {:else}<li class="empty-note">Подключений нет.</li>{/each}
          </ul>
        </div>

        <div class="section">
          <h2>Навыки <span class="count">{sheet.skills?.length ?? 0}</span></h2>
          <ul class="list">
            {#each sheet.skills ?? [] as k (k.name)}
              <li class="item slot">
                <Icon name="book" size={15} />
                <span class="grow"><span class="title">{k.name}</span><div class="sub">{k.description || 'без описания'}</div></span>
                {#if k.routines}<span class="chip" title="Навык сам заводит регулярные задачи">{k.routines} в расписании</span>{/if}
              </li>
            {:else}<li class="empty-note">Навыков нет. Навык — это папка с SKILL.md в <code>skills/{a.slug}/</code> в данных Senate: специалист читает его перед подходящей задачей.</li>{/each}
          </ul>
        </div>

        <div class="section">
          <RoutineList agentId={a.id} />
        </div>

        <div class="section">
          <h2>Проекты</h2>
          <ul class="list">
            {#each sheet.projects as p (p.id)}
              <li><button class="item" onclick={() => go({ kind: 'projects', id: p.id })}><Icon name="folder" size={14} /><span class="grow">{p.name}</span></button></li>
            {:else}<li class="empty-note">Не участвует</li>{/each}
          </ul>
        </div>

        <div class="section">
          <h2>Открытые задачи</h2>
          <ul class="list">
            {#each sheet.obligations as o (o.id)}
              <li><button class="item" onclick={() => go({ kind: 'obligations', id: o.id })}><span class="dot {o.state}"></span><span class="grow"><span class="title">{o.title}</span></span><span class="chip">{STATE_LABEL[o.state]}</span></button></li>
            {:else}<li class="empty-note">Нет открытых задач</li>{/each}
          </ul>
        </div>
        <button class="btn danger" onclick={retire}>Вывести из работы</button>
        {#if error}<p class="bad">{error}</p>{/if}
      </div>
    </div>
  </Page>
{:else}
  <Page title="Специалист"><p class="muted">{error || 'Загрузка…'}</p></Page>
{/if}

<style>
  .where .tabs { display: inline-flex; }
  .where .tabs button { display: inline-flex; align-items: center; gap: 6px; }
  .okc { color: var(--ok); font-weight: 600; }
  .badc { color: var(--warn, #d29922); font-weight: 600; }
  .hero-card {
    padding: 16px 16px 14px;
    display: flex;
    flex-direction: column;
    gap: 10px;
    position: relative;
    overflow: hidden;
  }
  .top {
    display: flex;
    gap: 12px;
    align-items: center;
    position: relative;
  }
  .who {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .who b {
    font-size: 15px;
  }
  .port {
    position: relative;
    display: inline-flex;
  }
  .port :global(.av) {
    box-shadow:
      inset 0 0 0 1px color-mix(in srgb, var(--c) 40%, transparent),
      0 0 0 3px var(--bg-raised),
      0 0 0 4.5px color-mix(in srgb, var(--c) 50%, transparent);
  }
  .lvl {
    position: absolute;
    right: -6px;
    bottom: -4px;
    min-width: 20px;
    height: 20px;
    padding: 0 5px;
    border-radius: 10px;
    background: var(--c);
    color: #fff;
    font-size: 11.5px;
    font-weight: 700;
    display: grid;
    place-items: center;
    box-shadow: 0 0 0 2px var(--bg-raised);
  }
  .clamp {
    display: -webkit-box;
    -webkit-line-clamp: 3;
    line-clamp: 3;
    -webkit-box-orient: vertical;
    overflow: hidden;
    margin: 0;
    font-size: 13px;
  }
  .meta {
    display: flex;
    gap: 6px;
    flex-wrap: wrap;
  }
  .bad {
    color: var(--bad);
  }
  .portcol { display: flex; flex-direction: column; align-items: center; gap: 10px; flex: none; }
  .photoact { display: flex; flex-wrap: wrap; justify-content: center; gap: 6px; }
  .pick { display: none; }
  .hero {
    display: flex;
    gap: 22px;
    align-items: center;
    padding: 22px 24px;
    margin: 0 0 22px;
    border-radius: var(--radius-l);
    /* the specialist's colour lives in the portrait ring; the surface stays graphite */
    background: var(--bg-raised);
    box-shadow: var(--shadow-1);
  }
  .port.big :global(.av) {
    box-shadow:
      inset 0 0 0 1.5px color-mix(in srgb, var(--c) 45%, transparent),
      0 0 0 4px var(--bg-raised),
      0 0 0 6px color-mix(in srgb, var(--c) 55%, transparent);
  }
  .port.big .lvl {
    min-width: 28px;
    height: 28px;
    border-radius: 14px;
    font-size: 14px;
    right: -8px;
    bottom: -6px;
  }
  .hbody {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 9px;
  }
  .nm {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .nm h1 {
    margin: 0;
    font-size: 26px;
    letter-spacing: -0.015em;
  }
  .xp {
    display: flex;
    align-items: center;
    gap: 12px;
    font-size: 12.5px;
  }
  .bar {
    width: 220px;
    max-width: 40vw;
    height: 6px;
    border-radius: 3px;
    background: var(--bg-active);
    overflow: hidden;
  }
  .bar i {
    display: block;
    height: 100%;
    border-radius: 3px;
    background: color-mix(in srgb, var(--c) 55%, var(--fg-2));
    transition: width 0.5s var(--ease);
  }
  .handle { font-size: 13px; margin: -2px 0 2px; }
  .rename { display: flex; flex-direction: column; gap: 10px; max-width: 360px; }
  .rename label { display: flex; flex-direction: column; gap: 4px; font-size: 12.5px; font-weight: 500; }
  .at { display: flex; align-items: center; gap: 6px; }
  .at i { font-style: normal; color: var(--fg-3); font-size: 15px; }
  .at .field { flex: 1; }
  .gender { display: flex; gap: 4px; padding: 3px; border-radius: var(--radius); background: var(--bg-sunken); width: fit-content; }
  .gender button { padding: 5px 11px; border-radius: calc(var(--radius) - 3px); font-size: 12.5px; color: var(--fg-2); }
  .gender button.on { background: var(--bg-raised); color: var(--fg); box-shadow: var(--shadow-1); }
  .gender button:focus-visible { outline: none; box-shadow: var(--focus-ring); }
  .swatches {
    display: grid;
    grid-template-columns: repeat(5, 24px);
    gap: 0;
    align-self: flex-start;
  }
  /* a 24 px target with a 16 px dot inside: easy to hit, still small to look at */
  .sw {
    width: 24px;
    height: 24px;
    border-radius: 50%;
    display: grid;
    place-items: center;
    background: transparent !important;
  }
  .sw::before {
    content: '';
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background: var(--sw);
    box-shadow: inset 0 0 0 1px rgba(0, 0, 0, 0.12);
    transition: transform 0.12s var(--ease);
  }
  .sw:hover::before {
    transform: scale(1.12);
  }
  .sw.on::before {
    box-shadow:
      0 0 0 2px var(--bg-raised),
      0 0 0 3.5px var(--fg-2);
  }
  .sw:focus-visible {
    outline: none;
    box-shadow: var(--focus-ring);
  }
  .sheet {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 340px;
    gap: 22px;
    align-items: start;
  }
  @media (max-width: 960px) {
    .sheet {
      grid-template-columns: 1fr;
    }
    .hero {
      flex-wrap: wrap;
    }
  }
  .tiles {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 10px;
  }
  .tile {
    padding: 12px 14px;
    display: flex;
    flex-direction: column;
    gap: 1px;
    position: relative;
  }
  .tile b {
    font-size: 24px;
    letter-spacing: -0.02em;
    font-weight: 650;
  }
  .tile .muted {
    font-size: 12.5px;
  }
  .ti {
    position: absolute;
    top: 12px;
    right: 12px;
    color: var(--fg-3);
  }
  .cdot {
    width: 10px;
    height: 10px;
    border-radius: 3px;
    flex: none;
  }
  .slot .sub {
    max-width: 180px;
  }
  .lnk {
    margin-left: auto;
    font-weight: 500;
    color: var(--accent);
    font-size: 12.5px;
  }
  .section > h2 {
    min-height: 20px;
  }

  /* the council's members: a glow in their own colour, the ring of experience around the face, a gold level */
  .cards { grid-template-columns: repeat(auto-fill, minmax(min(100%, 330px), 1fr)); gap: 16px; }
  .hero-card { padding: 18px 18px 16px; gap: 12px; background: var(--bg-raised); }
  .hero-card::before { display: none; }
  .top { gap: 14px; }
  .who b { font-size: 16px; font-weight: 600; letter-spacing: -0.015em; }
  .who .muted { font-size: 13px; }
  .port :global(.av) { box-shadow: none; }
  .lvl {
    right: -6px;
    bottom: -4px;
    min-width: 20px;
    height: 20px;
    border-radius: 10px;
    background: var(--fg-2);
    color: var(--bg-raised);
    font-size: 11.5px;
    font-weight: 650;
    box-shadow: 0 0 0 2px var(--bg-raised);
  }
  .clamp { margin: 0; font-size: 13.5px; line-height: 1.55; display: -webkit-box; -webkit-line-clamp: 3; line-clamp: 3; -webkit-box-orient: vertical; overflow: hidden; }
</style>

