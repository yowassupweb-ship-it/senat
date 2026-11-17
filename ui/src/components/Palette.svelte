<script lang="ts">
  import { focusScope } from "../lib/focus";
  import { app, go, applyTheme, type View } from '../lib/store.svelte';
  import { post } from '../lib/api';
  import { timeAgo } from '../lib/format';
  import Icon from './Icon.svelte';

  let q = $state('');
  let sel = $state(0);
  let input: HTMLInputElement;

  // kind orders the results: what the person opens most (chats, projects) before sections and settings
  type Cmd = { label: string; hint?: string; icon: string; run: () => void; kind?: 'chat' | 'project' | 'agent' | 'nav' | 'act'; at?: number; live?: boolean };

  const all = $derived.by<Cmd[]>(() => {
    const out: Cmd[] = [
      { label: 'Новый чат', icon: 'new-chat', run: () => go({ kind: 'new' }) },
      { label: 'Специалисты', icon: 'img:agents', run: () => go({ kind: 'agents' }) },
      { label: 'Проекты', icon: 'img:projects', run: () => go({ kind: 'projects' }) },
      { label: 'Задачи', icon: 'img:obligations', run: () => go({ kind: 'obligations' }) },
      { label: 'Облако', icon: 'cloud', run: () => go({ kind: 'cloud' }) },
      { label: 'Запланировано', icon: 'clock', run: () => go({ kind: 'scheduled' }) },
      { label: 'Память', icon: 'img:memory', run: () => go({ kind: 'memory' }) },
      { label: 'MCP', icon: 'img:mcp', run: () => go({ kind: 'mcp' }) },
      { label: 'API', icon: 'img:api', run: () => go({ kind: 'api' }) },
      { label: 'Компании', icon: 'img:companies', run: () => go({ kind: 'companies' }) },
      { label: 'Доступы', icon: 'img:access', run: () => go({ kind: 'access' }) },
      { label: 'Настройки', icon: 'sliders', run: () => go({ kind: 'settings' }) },
      { label: 'Тема: светлая', icon: 'sparkle', run: () => applyTheme('light') },
      { label: 'Тема: тёмная', icon: 'sparkle', run: () => applyTheme('dark') },
      { label: 'Тема: как в системе', icon: 'sparkle', run: () => applyTheme('auto') },
    ];
    for (const c of out) c.kind = c.label.startsWith('Тема') || c.label === 'Новый чат' ? 'act' : 'nav';
    for (const t of app.threads) {
      if (t.archived) continue;
      const p = t.project_id ? app.projects.find((x) => x.id === t.project_id)?.name : '';
      const state = t.awaiting ? 'ждёт вас' : t.working ? 'работает' : timeAgo(t.updated_at);
      out.push({ label: t.title || 'Новый чат', hint: p ? `${p} · ${state}` : state, icon: 'chat', kind: 'chat', at: t.updated_at, live: !!(t.awaiting || t.working), run: () => go({ kind: 'chat', id: t.id }) });
    }
    for (const p of app.projects) {
      out.push({ label: p.name, hint: 'проект', icon: 'img:projects', kind: 'project', run: () => go({ kind: 'projects', id: p.id }) });
      out.push({ label: `Новый чат в «${p.name}»`, icon: 'new-chat', kind: 'act', run: () => go({ kind: 'new', project: p.id }) });
    }
    for (const a of app.agents) {
      out.push({
        label: `Написать: ${a.name}`,
        icon: 'img:agents',
        kind: 'agent',
        run: async () => {
          const t = await post('/api/threads', { agent_id: a.id });
          app.threads.unshift(t);
          app.messages[t.id] = [];
          go({ kind: 'chat', id: t.id } as View);
        },
      });
    }
    return out;
  });

  // Typed on the wrong keyboard layout ("ckjdj" for "слово") still finds it: the person should not have to retype.
  const EN = "`qwertyuiop[]asdfghjkl;'zxcvbnm,.";
  const RU = "ёйцукенгшщзхъфывапролджэячсмитьбю";
  // both ways: Latin typed for Cyrillic, and Cyrillic typed for Latin (an English project name on the Russian layout)
  const swap = (t: string) => [...t].map((ch) => { const i = EN.indexOf(ch); if (i >= 0) return RU[i]; const j = RU.indexOf(ch); return j >= 0 ? EN[j] : ch; }).join('');
  const RANK: Record<string, number> = { chat: 0, project: 0, agent: 1, act: 2, nav: 2 };
  function score(c: Cmd, words: string[]): number | null {
    const hay = (c.label + ' ' + (c.hint ?? '')).toLowerCase();
    if (!words.every((w) => hay.includes(w))) return null;
    const label = c.label.toLowerCase();
    const first = words[0];
    const where = label.startsWith(first) ? 0 : label.split(/[s«:·-]+/).some((p) => p.startsWith(first)) ? 1 : 2;
    return where * 10 + (RANK[c.kind ?? 'nav'] ?? 2);
  }
  const list = $derived.by(() => {
    const query = q.trim().toLowerCase();
    // nothing typed: what the person most likely wants back, the chats that wait for them or work, then the latest
    if (!query) {
      const chats = all.filter((c) => c.kind === 'chat').sort((a, b) => Number(!!b.live) - Number(!!a.live) || (b.at ?? 0) - (a.at ?? 0));
      return [...chats.slice(0, 7), ...all.filter((c) => c.kind === 'act' && c.label === 'Новый чат')];
    }
    const run = (qq: string) => {
      const words = qq.split(/s+/).filter(Boolean);
      return all.map((c) => ({ c, s: score(c, words) })).filter((x) => x.s !== null).sort((a, b) => a.s! - b.s! || (b.c.at ?? 0) - (a.c.at ?? 0)).map((x) => x.c);
    };
    const found = run(query);
    return (found.length ? found : run(swap(query))).slice(0, 12);
  });

  $effect(() => {
    q;
    sel = 0;
  });
  $effect(() => input?.focus());

  function run(c?: Cmd) {
    if (!c) return;
    app.palette = false;
    c.run();
  }
  function onKey(e: KeyboardEvent) {
    if (e.key === 'Escape') app.palette = false;
    else if (e.key === 'ArrowDown') {
      e.preventDefault();
      sel = (sel + 1) % Math.max(1, list.length);
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      sel = (sel - 1 + list.length) % Math.max(1, list.length);
    } else if (e.key === 'Enter') {
      e.preventDefault();
      run(list[sel]);
    }
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="scrim" onclick={() => (app.palette = false)}>
  <div class="pal" use:focusScope={() => (app.palette = false)} role="dialog" tabindex="-1" aria-modal="true" aria-label="Поиск и команды" onclick={(e) => e.stopPropagation()}>
    <div class="in"><Icon name="search" size={16} /><input bind:this={input} bind:value={q} onkeydown={onKey} aria-label="Поиск команд" placeholder="Чат, проект, специалист или раздел" /></div>
    <ul role="listbox">
      {#each list as c, i (c.label + i)}
        <li>
          <button class:on={i === sel} role="option" aria-selected={i === sel} onmousemove={() => (sel = i)} onclick={() => run(c)}>
            <Icon name={c.icon} size={15} /><span>{c.label}</span>{#if c.hint}<span class="faint">{c.hint}</span>{/if}
          </button>
        </li>
      {:else}
        <li class="faint none">Ничего не найдено</li>
      {/each}
    </ul>
  </div>
</div>

<style>
  .scrim {
    position: fixed;
    inset: 0;
    z-index: 100;
    background: var(--scrim);
    backdrop-filter: blur(6px);
    display: flex;
    justify-content: center;
    padding-top: 14vh;
  }
  .pal {
    width: 560px;
    max-width: calc(100vw - 32px);
    max-height: 60vh;
    display: flex;
    flex-direction: column;
    background: var(--bg-raised);
    border-radius: var(--radius-l);
    box-shadow: var(--shadow-2);
    overflow: hidden;
    align-self: flex-start;
    animation: fadeIn 0.12s var(--ease);
  }
  .in {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 12px 16px;
    border-bottom: 1px solid var(--line);
    color: var(--fg-3);
  }
  .in input {
    flex: 1;
    min-width: 0;
    border: 0;
    outline: 0;
    background: none;
    font-size: 15px;
    color: var(--fg);
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 6px;
    overflow-y: auto;
  }
  li button {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    padding: 7px 10px;
    border-radius: var(--radius-s);
    text-align: left;
  }
  li button span:nth-child(2) {
    flex: 1;
  }
  li button.on {
    background: var(--accent-soft);
  }
  .none {
    padding: 12px;
    text-align: center;
  }
</style>
