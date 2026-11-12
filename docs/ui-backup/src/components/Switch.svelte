<script lang="ts">
  let { on, onchange, label = '', disabled = false }: { on: boolean; onchange: (v: boolean) => void; label?: string; disabled?: boolean } = $props();
</script>

<button class="sw" class:on role="switch" aria-checked={on} aria-label={label} {disabled} onclick={() => onchange(!on)}><i></i></button>

<style>
  .sw {
    width: 34px;
    height: 20px;
    border-radius: 10px;
    background: var(--line-strong);
    position: relative;
    flex: none;
    transition: background 0.15s var(--ease);
  }
  .sw i {
    position: absolute;
    top: 2px;
    left: 2px;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background: #fff;
    box-shadow: 0 1px 3px rgba(0, 0, 0, 0.3);
    transition: transform 0.15s var(--ease);
  }
  .sw.on {
    background: var(--accent);
  }
  .sw.on i {
    transform: translateX(14px);
  }
  .sw:disabled {
    opacity: 0.45;
  }
</style>
