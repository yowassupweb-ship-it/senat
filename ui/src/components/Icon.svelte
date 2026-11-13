<script lang="ts">
  // One icon family (Lucide-style strokes), no emoji anywhere in the product.
  const paths: Record<string, string> = {
    plus: 'M5 12h14M12 5v14',
    search: 'M21 21l-4.3-4.3M11 19a8 8 0 1 0 0-16 8 8 0 0 0 0 16z',
    folder: 'M4 20h16a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.7-.9l-.8-1.2A2 2 0 0 0 7.9 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2z',
    chat: 'M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z',
    users: 'M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2M9 11a4 4 0 1 0 0-8 4 4 0 0 0 0 8zM22 21v-2a4 4 0 0 0-3-3.87M16 3.13a4 4 0 0 1 0 7.75',
    checks: 'M10 6h11M10 12h11M10 18h11M3 6l1.5 1.5L7 5M3 12l1.5 1.5L7 11M3 18l1.5 1.5L7 17',
    book: 'M4 19.5A2.5 2.5 0 0 1 6.5 17H20M4 19.5A2.5 2.5 0 0 0 6.5 22H20V2H6.5A2.5 2.5 0 0 0 4 4.5z',
    sliders: 'M21 4h-7M10 4H3M21 12h-9M8 12H3M21 20h-5M12 20H3M14 2v4M8 10v4M16 18v4',
    up: 'M12 19V5M5 12l7-7 7 7',
    stop: 'M7 7h10v10H7z',
    check: 'M20 6L9 17l-5-5',
    x: 'M18 6L6 18M6 6l12 12',
    right: 'M9 18l6-6-6-6',
    left: 'M15 18l-6-6 6-6',
    down: 'M6 9l6 6 6-6',
    file: 'M14.5 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7.5zM14 2v6h6',
    terminal: 'M4 17l6-6-6-6M12 19h8',
    pencil: 'M17 3a2.85 2.83 0 1 1 4 4L7.5 20.5 2 22l1.5-5.5z',
    globe: 'M12 22a10 10 0 1 0 0-20 10 10 0 0 0 0 20zM2 12h20M12 2a15.3 15.3 0 0 1 4 10 15.3 15.3 0 0 1-4 10 15.3 15.3 0 0 1-4-10 15.3 15.3 0 0 1 4-10z',
    branch: 'M6 3v12M18 9a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM6 21a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM18 9a9 9 0 0 1-9 9',
    copy: 'M8 8h12v12H8zM4 16V4h12',
    shield: 'M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z',
    panel: 'M3 5h18v14H3zM15 5v14',
    trash: 'M3 6h18M8 6V4h8v2M19 6l-1 14H6L5 6',
    refresh: 'M21 12a9 9 0 1 1-3-6.7L21 8M21 3v5h-5',
    clock: 'M12 22a10 10 0 1 0 0-20 10 10 0 0 0 0 20zM12 6v6l4 2',
    alert: 'M10.3 3.9L1.8 18a2 2 0 0 0 1.7 3h17a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0zM12 9v4M12 17h.01',
    clip: 'M21.4 11.1l-9.2 9.2a6 6 0 0 1-8.5-8.5l9.2-9.2a4 4 0 0 1 5.7 5.7l-9.2 9.2a2 2 0 0 1-2.8-2.8l8.5-8.5',
    more: 'M12 12h.01M19 12h.01M5 12h.01',
    pin: 'M12 17v5M9 3h6l-1 7 3 3H7l3-3z',
    archive: 'M3 4h18v4H3zM5 8v12h14V8M10 12h4',
    home: 'M3 11l9-8 9 8M5 10v10h14V10',
    sparkle: 'M12 3l1.9 5.1L19 10l-5.1 1.9L12 17l-1.9-5.1L5 10l5.1-1.9z',
    eye: 'M2 12s4-7 10-7 10 7 10 7-4 7-10 7S2 12 2 12zM12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6z',
    list: 'M8 6h13M8 12h13M8 18h13M3 6h.01M3 12h.01M3 18h.01',
    lock: 'M5 11h14v10H5zM8 11V7a4 4 0 0 1 8 0v4',
    server: 'M3 4h18v6H3zM3 14h18v6H3zM7 7h.01M7 17h.01',
    monitor: 'M3 4h18v12H3zM8 20h8M12 16v4',
    flag: 'M4 22V4M4 4h13l-2 4 2 4H4',
    brain: 'M9.5 4A3.5 3.5 0 0 0 6 7.5v.3A3.5 3.5 0 0 0 4 11a3.5 3.5 0 0 0 1.5 2.9A3.5 3.5 0 0 0 9 19.5 2.5 2.5 0 0 0 12 17V6.5A2.5 2.5 0 0 0 9.5 4zM14.5 4A3.5 3.5 0 0 1 18 7.5v.3A3.5 3.5 0 0 1 20 11a3.5 3.5 0 0 1-1.5 2.9 3.5 3.5 0 0 1-3.5 5.6A2.5 2.5 0 0 1 12 17',
    diff: 'M12 3v10M7 8h10M7 20h10',
    undo: 'M3 7v6h6M3 13a9 9 0 1 0 3-7.7L3 8',
    reply: 'M9 17l-5-5 5-5M4 12h11a5 5 0 0 1 5 5v2',
    plug: 'M9 2v6M15 2v6M6 8h12v4a6 6 0 0 1-12 0zM12 18v4',
    link: 'M10 13a5 5 0 0 0 7.5.5l3-3a5 5 0 0 0-7-7l-1.7 1.7M14 11a5 5 0 0 0-7.5-.5l-3 3a5 5 0 0 0 7 7l1.7-1.7',
    building: 'M4 21V5l8-3 8 3v16M9 21v-4h6v4M8 9h.01M12 9h.01M16 9h.01M8 13h.01M12 13h.01M16 13h.01',
    forum: 'M17 8h3a2 2 0 0 1 2 2v7l-3-2h-5a2 2 0 0 1-2-2v-1M13 3H4a2 2 0 0 0-2 2v8l3-2h8a2 2 0 0 0 2-2V5a2 2 0 0 0-2-2z',
    key: 'M15 7a5 5 0 1 1-4.6 6.9L3 21v-3h3v-3h3l1.5-1.5A5 5 0 0 1 15 7zM17 9h.01',
    star: 'M12 2l3 6.9 7.5.7-5.7 5 1.7 7.4L12 18l-6.5 4 1.7-7.4-5.7-5 7.5-.7z',
    bolt: 'M13 2L4 14h7l-1 8 9-12h-7z',
    cloud: 'M17.5 19H9a7 7 0 1 1 6.71-9h1.79a4.5 4.5 0 1 1 0 9z',
    upload: 'M12 16V4M7 9l5-5 5 5M4 20h16',
    download: 'M12 4v12M7 11l5 5 5-5M4 20h16',
    foldernew: 'M4 20h16a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.7-.9l-.8-1.2A2 2 0 0 0 7.9 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2zM12 11v6M9 14h6',
  };
  // Hand-drawn glyphs (PNG masks in /icons/ui) take the text colour like the strokes below.
  const GLYPHS = new Set(['new-chat', 'forum', 'permissions', 'effort', 'tool', 'memory-write', 'bash', 'browser', 'read', 'web-search']);
  let { name, size = 16, filled = false }: { name: string; size?: number; filled?: boolean } = $props();
</script>

{#if name.startsWith('img:')}
  <img class="pic" src="/icons/{name.slice(4)}.png" width={Math.round(size * 1.25)} height={Math.round(size * 1.25)} alt="" />
{:else if GLYPHS.has(name)}
  <span class="glyph" aria-hidden="true" style="--u:url('/icons/ui/{name}.png');width:{Math.round(size * 1.25)}px;height:{Math.round(size * 1.25)}px"></span>
{:else}
<svg
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill={filled ? 'currentColor' : 'none'}
  stroke="currentColor"
  stroke-width="1.8"
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"
>
  <path d={paths[name] ?? ''} />
</svg>
{/if}

<style>
  .pic { display: inline-block; flex: none; vertical-align: middle; margin: -3px; object-fit: contain; }
  :global(:root[data-theme='light']) .pic { filter: brightness(0.42) contrast(1.15); }
  @media (prefers-color-scheme: light) { :global(:root:not([data-theme='dark'])) .pic { filter: brightness(0.42) contrast(1.15); } }
  .glyph { display: inline-block; flex: none; vertical-align: middle; margin: -2px; background: currentColor; -webkit-mask: var(--u) center / contain no-repeat; mask: var(--u) center / contain no-repeat; }
</style>
