<script lang="ts">
  import { focusScope } from "../lib/focus";
  import { app, go, applyTheme, type View } from '../lib/store.svelte';
  import { post } from '../lib/api';
  import Icon from './Icon.svelte';

  let q = $state('');
  let sel = $state(0);
  let input: HTMLInputElement;

  type Cmd = { label: string; hint?: string; icon: string; run: () => void };

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
    for (const t of app.threads) out.push({ label: t.title || 'Новый чат', hint: 'чат', icon: 'chat', run: () => go({ kind: 'chat', id: t.id }) });
    for (const p of app.projects) {
      out.push({ label: p.name, hint: 'проект', icon: 'img:projects', run: () => go({ kind: 'projects', id: p.id }) });
      out.push({ label: `Новый чат в «${p.name}»`, icon: 'new-chat', run: () => go({ kind: 'new', project: p.id }) });
    }
    for (const a of app.agents) {
      out.push({
        label: `Написать: ${a.name}`,
        icon: 'img:agents',
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

  const list = $derived(
    (q.trim() ? all.filter((c) => c.label.toLowerCase().includes(q.trim().toLowerCase())) : all.slice(0, 9)).slice(0, 12),
  );

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
    <div class="in"><Icon name="search" size={16} /><input bind:this={input} bind:value={q} onkeydown={onKey} aria-label="Поиск команд" placeholder="Чат, проект, специалист или команда" /></div>
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
