<script lang="ts">
  import { get, patch, post } from '../lib/api';
  import { agentById, app, go } from '../lib/store.svelte';
  import { timeAgo } from '../lib/format';
  import Avatar from '../components/Avatar.svelte';
  import Icon from '../components/Icon.svelte';
  import Page from '../components/Page.svelte';

  let { id }: { id?: string } = $props();

  let creating = $state(false);
  let draft = $state({ name: '', brief: '', color: '#5e5ce6' });
  let d = $state<any>(null);
  let brief = $state('');
  let newProject = $state('');
  let newDecision = $state('');
  let error = $state('');
  const colors = ['#5e5ce6', '#0a84ff', '#30d158', '#ffd60a', '#ff9f0a', '#ff453a', '#bf5af2', '#64d2ff'];

  async function load() {
    if (!id) return (d = null);
    try {
      d = await get(`/api/companies/${id}`);
      brief = d.company.brief;
    } catch (e: any) {
      error = e.message;
    }
  }
  $effect(() => {
    id;
    load();
  });
  async function refresh() {
    app.companies = await get('/api/companies');
    app.projects = await get('/api/projects');
  }
  async function create() {
    try {
      const c = await post('/api/companies', draft);
      await refresh();
      creating = false;
      draft = { name: '', brief: '', color: '#5e5ce6' };
      go({ kind: 'companies', id: c.id });
    } catch (e: any) {
      error = e.message;
    }
  }
  async function save(fields: Record<string, string>) {
    await patch(`/api/companies/${id}`, fields);
    await refresh();
    load();
  }
  async function addProject() {
    if (!newProject.trim()) return;
    await post('/api/projects', { name: newProject, company_id: id });
    newProject = '';
    await refresh();
    load();
  }
  async function propose() {
    if (!newDecision.trim()) return;
    await post(`/api/companies/${id}/decisions`, { title: newDecision });
    newDecision = '';
    load();
  }
  async function decide(dec: any, status: string) {
    await post(`/api/decisions/${dec.id}/status`, { status });
    load();
  }
  const st: Record<string, string> = { proposed: 'предложено', accepted: 'принято', rejected: 'отклонено', superseded: 'заменено' };
</script>

{#if !id}
  <Page title="Компании">
    {#snippet actions()}<button class="btn primary" onclick={() => (creating = !creating)}><Icon name="plus" size={14} />Новая компания</button>{/snippet}
    {#if creating}
      <form class="card pad form" style="margin-bottom:18px" onsubmit={(e) => { e.preventDefault(); create(); }}>
        <label>Название<input class="field" bind:value={draft.name} required /></label>
        <label>Кто они и как работают<textarea class="field" rows="3" bind:value={draft.brief}></textarea></label>
        <div class="sw">{#each colors as c}<button type="button" class:on={draft.color === c} style="background:{c}" onclick={() => (draft.color = c)} aria-label="Цвет {c}"></button>{/each}</div>
        <div class="row"><button class="btn primary" type="submit">Создать</button><button class="btn" type="button" onclick={() => (creating = false)}>Отмена</button></div>
        {#if error}<p class="bad">{error}</p>{/if}
      </form>
    {/if}
    <div class="cards">
      {#each app.companies as c (c.id)}
        <button class="card pad click" onclick={() => go({ kind: 'companies', id: c.id })}>
          <div class="row"><i class="cd" style="background:{c.color}"></i><b>{c.name}</b></div>
          {#if c.brief}<p class="muted clamp">{c.brief}</p>{/if}
          <div class="row"><span class="chip">{c.projects ?? 0} проектов</span><span class="chip">{c.open_obligations ?? 0} задач</span></div>
        </button>
      {:else}<p class="empty-note">Компаний пока нет</p>{/each}
    </div>
  </Page>
{:else if d}
  <Page title={d.company.name}>
    {#snippet actions()}<button class="btn" onclick={() => go({ kind: 'companies' })}>Все компании</button><button class="btn primary" onclick={() => go({ kind: 'new' })}><Icon name="chat" size={14} />Новый чат</button>{/snippet}
    <div class="split">
      <div>
        <div class="section">
          <h2>О компании</h2>
          <div class="card pad form">
            <textarea class="field" rows="4" bind:value={brief} placeholder="Кто они, как работают, что для них важно"></textarea>
            <div class="row"><button class="btn primary" disabled={brief === d.company.brief} onclick={() => save({ brief })}>Сохранить</button>
              <div class="sw" style="margin-left:auto">{#each colors as c}<button type="button" class:on={d.company.color === c} style="background:{c}" onclick={() => save({ color: c })} aria-label="Цвет {c}"></button>{/each}</div></div>
          </div>
        </div>

        <div class="section">
          <h2>Общие решения <span class="count">{d.decisions.length}</span></h2>
          <ul class="list">
            {#each d.decisions as dec (dec.id)}
              <li class="item">
                <span class="grow"><span class="title">{dec.title}</span>{#if dec.body}<div class="sub">{dec.body}</div>{/if}</span>
                <span class="chip {dec.status === 'accepted' ? 'ok' : dec.status === 'proposed' ? 'warn' : ''}">{st[dec.status]}</span>
                {#if dec.status === 'proposed'}<button class="btn sm primary" onclick={() => decide(dec, 'accepted')}>Принять</button><button class="btn sm" onclick={() => decide(dec, 'rejected')}>Отклонить</button>{/if}
              </li>
            {:else}<li class="empty-note">Нет решений</li>{/each}
          </ul>
          <form class="row" style="margin-top:8px" onsubmit={(e) => { e.preventDefault(); propose(); }}>
            <input class="field" bind:value={newDecision} placeholder="Решение для всех проектов компании" /><button class="btn" type="submit">Предложить</button>
          </form>
        </div>

        <div class="section">
          <h2>Знания о компании <span class="count">{d.memory.length}</span></h2>
          <ul class="list">
            {#each d.memory as m (m.id)}<li class="item"><span class="grow" style="white-space:pre-wrap">{m.body}</span><span class="faint">{timeAgo(m.updated_at)}</span></li>{:else}<li class="empty-note">Пока ничего</li>{/each}
          </ul>
        </div>
      </div>

      <div>
        <div class="section">
          <h2>Проекты <span class="count">{d.projects.length}</span></h2>
          <ul class="list">
            {#each d.projects as p (p.id)}<li><button class="item" onclick={() => go({ kind: 'projects', id: p.id })}><Icon name="folder" size={14} /><span class="grow">{p.name}</span></button></li>{:else}<li class="empty-note">Нет проектов</li>{/each}
          </ul>
          <form class="row" style="margin-top:8px" onsubmit={(e) => { e.preventDefault(); addProject(); }}>
            <input class="field" bind:value={newProject} placeholder="Новый проект" /><button class="btn" type="submit">Добавить</button>
          </form>
        </div>
        <div class="section">
          <h2>Кто здесь работал</h2>
          <ul class="list">
            {#each d.people as p (p.agent.id)}
              <li><button class="item" onclick={() => go({ kind: 'agents', id: p.agent.id })}>
                <Avatar name={p.agent.name} color={p.agent.accent} size={24} /><span class="grow">{p.agent.name}<div class="sub">{p.own_memory} знаний · {p.done} задач</div></span></button></li>
            {:else}<li class="empty-note">Пока никто</li>{/each}
          </ul>
        </div>
      </div>
    </div>
  </Page>
{:else}
  <Page title="Компания"><p class="muted">{error || 'Загрузка…'}</p></Page>
{/if}

<style>
  .cd {
    width: 12px;
    height: 12px;
    border-radius: 4px;
    flex: none;
  }
  .clamp {
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
    margin: 8px 0;
    font-size: 13px;
  }
  .sw {
    display: flex;
    gap: 7px;
  }
  .sw button {
    width: 18px;
    height: 18px;
    border-radius: 50%;
    box-shadow: inset 0 0 0 1px rgba(0, 0, 0, 0.12);
  }
  .sw button.on {
    box-shadow:
      0 0 0 2px var(--bg-raised),
      0 0 0 3.5px var(--fg-2);
  }
  .bad {
    color: var(--bad);
  }
</style>
