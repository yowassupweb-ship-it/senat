<script lang="ts">
  // What the app is connected to and what both ends are built from. The person should not learn about a stale
  // server or a dropped link from a failing specialist.
  import { get } from '../lib/api';
  import { app } from '../lib/store.svelte';
  import { managedLink, readLink, type LinkState } from '../lib/link';
  import { count, dateTime } from '../lib/format';

  let health = $state<{ version?: string; build?: string } | null>(null);
  let live = $state<number | null>(null);
  let link = $state<LinkState | null>(null);
  let failed = $state(false);

  async function load() {
    try {
      health = await get('/api/health');
      const l = await get('/api/live');
      live = Array.isArray(l) ? l.length : null;
      failed = false;
    } catch {
      failed = true;
    }
    link = await readLink();
  }
  $effect(() => {
    load();
    const t = setInterval(load, 10000);
    return () => clearInterval(t);
  });

  const where = (window as any).__SENATE__?.name || (managedLink() ? 'Сервер' : 'Этот компьютер');
  // stamps look like "1198485+edits 2026-11-12 18:30"
  const commitOf = (s?: string) => (s || '').split(/[+\s]/)[0];
  const human = (s?: string) => (s || '').replace('+edits', ' с локальными правками');
  const uiBuild = __UI_BUILD__;
  const nodeBuild = $derived(health?.build);
  const behind = $derived(!!health && !nodeBuild);
  const differs = $derived(!!nodeBuild && commitOf(nodeBuild) !== commitOf(uiBuild));
</script>

<section class="status">
  <div class="row1">
    <span class="dot" class:ok={app.online && !failed} class:bad={!app.online || failed}></span>
    <b>{app.online && !failed ? 'Подключено' : 'Нет связи'}</b>
    <span class="muted">{where}</span>
    {#if live}<span class="chip">{count(live, ['работа идёт', 'работы идут', 'работ идёт'])}</span>{/if}
  </div>
  <dl class="kv">
    {#if link}
      <dt>Туннель</dt>
      <dd>{link.up ? 'работает' : link.detail || 'восстанавливается'}{link.restarts ? ` · перезапусков: ${link.restarts}` : ''}</dd>
    {/if}
    <dt>Нода</dt>
    <dd>
      {#if !health}—{:else if nodeBuild}<span class="mono">{human(nodeBuild)}</span>{:else}версия {health.version}, без отметки сборки{/if}
    </dd>
    <dt>Приложение</dt>
    <dd><span class="mono">{human(uiBuild)}</span> <span class="faint">· {dateTime(Date.parse(__UI_BUILT_AT__))}</span></dd>
  </dl>
  {#if behind}
    <p class="warn">Нода на сервере собрана до того, как появились отметки версий, то есть она старше этого приложения. Исправления, которые работают на ноде (например, восстановление потерянной сессии специалиста), заработают после её обновления.</p>
  {:else if differs}
    <p class="note">Нода и приложение собраны из разных версий кода. Обычно это не мешает, но если специалисты ведут себя не так, как ожидается, обновите ноду.</p>
  {/if}
</section>

<style>
  .status { background: var(--bg-raised); border-radius: var(--radius); box-shadow: var(--shadow-1); padding: 14px 16px; margin-bottom: 28px; }
  .row1 { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; margin-bottom: 10px; }
  .dot { width: 8px; height: 8px; border-radius: 50%; background: var(--fg-3); flex: none; }
  .dot.ok { background: var(--ok); }
  .dot.bad { background: var(--bad); }
  .kv { margin: 0; font-size: 13px; }
  .mono { font-size: 12px; }
  .warn, .note { margin: 12px 0 0; padding: 9px 12px; border-radius: var(--radius-s); font-size: 12.5px; line-height: 1.45; }
  .warn { background: var(--warn-soft); color: var(--warn); }
  .note { background: var(--bg-hover); color: var(--fg-2); }
</style>
