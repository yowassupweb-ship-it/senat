<script lang="ts">
  // The person's computers with the Senate app: which are connected, what they run, which development tools they have.
  import { app, loadMachines } from '../lib/store.svelte';
  import { timeAgo } from '../lib/format';

  $effect(() => {
    loadMachines();
  });
  const OS: Record<string, string> = { windows: 'Windows', macos: 'macOS', linux: 'Linux' };
  const projectsOn = (id: string) => app.projects.filter((p) => p.node_id === id && p.root_path).map((p) => p.name);
</script>

{#if app.machines.length}
  <section class="block">
    <h2>Компьютеры</h2>
    <ul class="list">
      {#each app.machines as m (m.id)}
        {@const ps = projectsOn(m.id)}
        <li class="m">
          <div class="top">
            <span class="dot" class:on={m.online} aria-hidden="true"></span>
            <b>{m.name}</b>
            <span class="faint">{OS[m.os] ?? m.os}</span>
            <span class="grow"></span>
            <span class="faint st">{m.online ? 'в сети' : m.last_seen ? `был ${timeAgo(m.last_seen)} назад` : 'не в сети'}</span>
          </div>
          {#if m.env?.tools}
            <div class="tools">
              {#each Object.entries(m.env.tools) as [k, v] (k)}<span class="chip" title={v}>{k}</span>{/each}
              {#each m.env.missing ?? [] as k (k)}<span class="chip off" title="Не найден на этом компьютере">{k}</span>{/each}
            </div>
          {/if}
          {#if ps.length}<div class="faint ps">Папки проектов: {ps.join(', ')}</div>{/if}
        </li>
      {/each}
    </ul>
  </section>
{/if}

<style>
  .block {
    margin-bottom: 28px;
  }
  .list {
    list-style: none;
    margin: 0;
    padding: 0;
    border-radius: var(--radius);
    background: var(--bg-raised);
  }
  .list li + li {
    border-top: 1px solid var(--line);
  }
  .m {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 10px 12px;
  }
  .top {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }
  .grow {
    flex: 1;
  }
  .st {
    font-size: 12px;
    white-space: nowrap;
  }
  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--fg-3);
    flex: none;
  }
  .dot.on {
    background: var(--ok);
  }
  .tools {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }
  .chip.off {
    opacity: 0.5;
    text-decoration: line-through;
  }
  .ps {
    font-size: 12px;
  }
</style>
