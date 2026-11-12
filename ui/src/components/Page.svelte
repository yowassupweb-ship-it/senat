<script lang="ts">
  import type { Snippet } from 'svelte';
  let { title, sub = '', actions, children }: { title: string; sub?: string; actions?: Snippet; children: Snippet } = $props();
</script>

<section class="page">
  <header>
    <div class="t"><h1>{title}</h1>{#if sub}<p class="muted">{sub}</p>{/if}</div>
    {#if actions}<div class="acts">{@render actions()}</div>{/if}
  </header>
  <div class="body"><div class="in">{@render children()}</div></div>
</section>

<style>
  .page {
    height: 100%;
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  header {
    flex: none;
    min-height: 58px;
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 8px 28px 0;
  }
  .t {
    flex: 1;
    min-width: 0;
  }
  h1 {
    margin: 0;
    font-size: 20px;
    font-weight: 650;
    letter-spacing: -0.01em;
  }
  .t p {
    margin: 0;
    font-size: 13px;
  }
  .acts {
    display: flex;
    gap: 8px;
  }
  .body {
    flex: 1;
    overflow-y: auto;
    padding: 12px 28px 40px;
  }
  .in {
    max-width: 980px;
    margin: 0 auto;
  }
  .page { background: transparent; }
  header {
    position: relative;
    padding: 18px 28px 16px;
    min-height: 74px;
    border-bottom: 1px solid var(--line);
    background: var(--toolbar);
    backdrop-filter: blur(20px) saturate(1.4);
  }
  h1 { font-size: 26px; line-height: 1.15; font-weight: 650; letter-spacing: -0.03em; }
  .t p { margin-top: 4px; font-size: 13px; }
  .body { padding: 22px 28px 44px; }
  .acts { flex-wrap: wrap; }
  @media (max-width: 760px) {
    header { padding: 16px 16px 16px 52px; flex-wrap: wrap; }
    h1 { font-size: 23px; }
    .body { padding: 20px 16px; }
  }
</style>
