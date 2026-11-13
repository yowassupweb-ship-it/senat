<script lang="ts">
  // Load of the person's computer(s) and of the node, next to the search: one number each (the busiest of processor,
  // memory and video card), the details on hover or focus.
  import { get } from '../lib/api';
  import Icon from './Icon.svelte';

  type M = {
    cpu: number;
    cores: number;
    ram: { used: number; total: number };
    gpu?: { name: string; util: number | null; mem_used_mb: number | null; mem_total_mb: number | null; temp: number | null } | null;
    disk?: { used: number; total: number } | null;
  } | null;
  type Host = { key: string; name: string; icon: 'monitor' | 'server'; m: M };

  let hosts = $state<Host[]>([]);
  let open = $state(false);
  let timer: ReturnType<typeof setTimeout> | undefined;

  async function load() {
    if (document.visibilityState !== 'visible') return;
    try {
      const r = await get('/api/metrics');
      const list: Host[] = (r.machines ?? []).map((x: any) => ({ key: x.id, name: x.name, icon: 'monitor', m: x.metrics }));
      list.push({ key: 'node', name: r.node?.name ?? 'Сервер', icon: r.node?.server ? 'server' : 'monitor', m: r.node?.metrics });
      hosts = list.filter((h) => h.m);
    } catch {
      // an older node has no metrics: the strip simply stays empty
    }
  }
  $effect(() => {
    load();
    const t = setInterval(load, 3000);
    return () => clearInterval(t);
  });

  const pct = (u: number, t: number) => (t ? Math.round((u / t) * 100) : 0);
  const gb = (b: number) => (b / 1024 ** 3).toFixed(b >= 10 * 1024 ** 3 ? 0 : 1);
  const ram = (m: NonNullable<M>) => pct(m.ram.used, m.ram.total);
  const vram = (m: NonNullable<M>) => (m.gpu?.mem_total_mb ? pct(m.gpu.mem_used_mb ?? 0, m.gpu.mem_total_mb) : 0);
  // the one number: whatever is closest to its limit
  const overall = (m: NonNullable<M>) => Math.round(Math.max(m.cpu, ram(m), m.gpu?.util ?? 0, vram(m)));
  const tone = (v: number) => (v >= 95 ? 'bad' : v >= 80 ? 'warn' : '');
  const show = () => { clearTimeout(timer); open = true; };
  const hide = () => { clearTimeout(timer); timer = setTimeout(() => (open = false), 120); };
</script>

{#if hosts.length}
  <div class="load" role="group" aria-label="Загрузка компьютера и сервера" onmouseenter={show} onmouseleave={hide}>
    <button class="sum" onfocus={show} onblur={hide} aria-expanded={open} aria-label={hosts.map((h) => `${h.name}: ${overall(h.m!)}%`).join(', ')}>
      {#each hosts as h (h.key)}
        {@const v = overall(h.m!)}
        <span class="h {tone(v)}"><Icon name={h.icon} size={11} />{v}<small>%</small></span>
      {/each}
    </button>
    {#if open}
      <div class="card" role="tooltip">
        {#each hosts as h (h.key)}
          {@const m = h.m!}
          <section>
            <header><Icon name={h.icon} size={12} /><b>{h.name}</b><span class="faint">{overall(m)}%</span></header>
            <div class="row"><span>Процессор</span><span class="bar"><i class={tone(m.cpu)} style="width:{Math.min(100, m.cpu)}%"></i></span><span class="v">{Math.round(m.cpu)}%</span><span class="d faint">{m.cores} ядер</span></div>
            <div class="row"><span>Память</span><span class="bar"><i class={tone(ram(m))} style="width:{ram(m)}%"></i></span><span class="v">{ram(m)}%</span><span class="d faint">{gb(m.ram.used)} из {gb(m.ram.total)} ГБ</span></div>
            {#if m.gpu}
              <div class="row"><span>Видеокарта</span><span class="bar"><i class={tone(m.gpu.util ?? 0)} style="width:{m.gpu.util ?? 0}%"></i></span><span class="v">{Math.round(m.gpu.util ?? 0)}%</span><span class="d faint">{m.gpu.temp != null ? `${m.gpu.temp}°` : ''}</span></div>
              <div class="row"><span>Видеопамять</span><span class="bar"><i class={tone(vram(m))} style="width:{vram(m)}%"></i></span><span class="v">{vram(m)}%</span><span class="d faint">{((m.gpu.mem_used_mb ?? 0) / 1024).toFixed(1)} из {((m.gpu.mem_total_mb ?? 0) / 1024).toFixed(0)} ГБ</span></div>
              <p class="gname faint">{m.gpu.name}</p>
            {/if}
            {#if m.disk}
              <div class="row"><span>Диск</span><span class="bar"><i class={tone(pct(m.disk.used, m.disk.total))} style="width:{pct(m.disk.used, m.disk.total)}%"></i></span><span class="v">{pct(m.disk.used, m.disk.total)}%</span><span class="d faint">свободно {gb(m.disk.total - m.disk.used)} ГБ</span></div>
            {/if}
          </section>
        {/each}
      </div>
    {/if}
  </div>
{/if}

<style>
  .load {
    position: relative;
    -webkit-app-region: no-drag;
  }
  .sum {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 26px;
    padding: 0 6px;
    border-radius: var(--radius-s);
    color: var(--fg-3);
    font-size: 11.5px;
    font-variant-numeric: tabular-nums;
  }
  .sum:hover,
  .sum:focus-visible {
    background: var(--bg-hover);
    color: var(--fg-2);
  }
  .h {
    display: inline-flex;
    align-items: center;
    gap: 3px;
  }
  .h small {
    font-size: 9.5px;
    opacity: 0.8;
  }
  .h.warn {
    color: var(--warn);
  }
  .h.bad {
    color: var(--bad);
  }
  .card {
    position: absolute;
    top: calc(100% + 6px);
    right: 0;
    z-index: 70;
    width: 300px;
    padding: 10px 12px;
    border: 1px solid var(--line);
    border-radius: 10px;
    background: var(--bg-elev, var(--bg));
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.28);
    color: var(--fg);
    font-size: 12px;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  header {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-bottom: 6px;
  }
  header .faint {
    margin-left: auto;
    font-variant-numeric: tabular-nums;
  }
  .row {
    display: grid;
    grid-template-columns: 74px 1fr 34px 96px;
    align-items: center;
    gap: 6px;
    margin-top: 4px;
    color: var(--fg-2);
  }
  .bar {
    height: 4px;
    border-radius: 2px;
    background: var(--line);
    overflow: hidden;
  }
  .bar i {
    display: block;
    height: 100%;
    background: var(--fg-2);
    border-radius: 2px;
  }
  .bar i.warn {
    background: var(--warn);
  }
  .bar i.bad {
    background: var(--bad);
  }
  .v {
    text-align: right;
    font-variant-numeric: tabular-nums;
    color: var(--fg);
  }
  .d {
    font-size: 11px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .gname {
    margin: 4px 0 0;
    font-size: 11px;
  }
</style>
