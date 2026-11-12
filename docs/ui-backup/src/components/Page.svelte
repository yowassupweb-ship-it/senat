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
</style>
