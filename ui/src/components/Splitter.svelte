<script lang="ts">
  // A handle on a pane's edge: drag to resize, arrows to nudge (Shift for bigger steps), double-click to restore.
  //   edge: which edge of the pane it sits on — 'right' (the pane grows to the right), 'left', or 'bottom'.
  import { PANES, applySize, sizeOf } from '../lib/layout';

  let { pane, edge, label }: { pane: keyof typeof PANES; edge: 'left' | 'right' | 'bottom'; label: string } = $props();
  // which pane is fixed for the life of the handle
  const p = PANES[(() => pane)()];
  let value = $state(sizeOf(p));
  let dragging = $state(false);
  let el: HTMLDivElement;

  function down(e: PointerEvent) {
    if (e.button !== 0) return;
    e.preventDefault();
    el.setPointerCapture(e.pointerId);
    dragging = true;
    document.documentElement.dataset.resizing = edge === 'bottom' ? 'y' : 'x';
    const start = edge === 'bottom' ? e.clientY : e.clientX;
    const from = value;
    // the tree's share is measured against its container's height
    const box = edge === 'bottom' ? (el.parentElement?.getBoundingClientRect().height ?? 1) : 1;
    const move = (ev: PointerEvent) => {
      const d = (edge === 'bottom' ? ev.clientY : ev.clientX) - start;
      const delta = edge === 'bottom' ? (d / box) * 100 : edge === 'right' ? d : -d;
      value = applySize(p, from + delta, false);
    };
    const up = () => {
      dragging = false;
      delete document.documentElement.dataset.resizing;
      value = applySize(p, value);
      el.removeEventListener('pointermove', move);
      el.removeEventListener('pointerup', up);
      el.removeEventListener('pointercancel', up);
    };
    el.addEventListener('pointermove', move);
    el.addEventListener('pointerup', up);
    el.addEventListener('pointercancel', up);
  }

  function key(e: KeyboardEvent) {
    const step = (e.shiftKey ? 5 : 1) * (pane === 'tree' ? 2 : 16);
    const grow = edge === 'bottom' ? 'ArrowDown' : edge === 'right' ? 'ArrowRight' : 'ArrowLeft';
    const shrink = edge === 'bottom' ? 'ArrowUp' : edge === 'right' ? 'ArrowLeft' : 'ArrowRight';
    if (e.key === grow) value = applySize(p, value + step);
    else if (e.key === shrink) value = applySize(p, value - step);
    else if (e.key === 'Home') value = applySize(p, p.min);
    else if (e.key === 'End') value = applySize(p, p.max);
    else return;
    e.preventDefault();
  }
</script>

<!-- a focusable separator is an interactive control (WAI-ARIA window splitter) -->
<!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
<div
  bind:this={el}
  class="split {edge}"
  class:on={dragging}
  role="separator"
  tabindex="0"
  aria-label={label}
  aria-orientation={edge === 'bottom' ? 'horizontal' : 'vertical'}
  aria-valuemin={p.min}
  aria-valuemax={p.max}
  aria-valuenow={value}
  title="{label}: перетащите; двойной щелчок — исходный размер"
  onpointerdown={down}
  ondblclick={() => (value = applySize(p, p.def))}
  onkeydown={key}
></div>

<style>
  .split {
    position: absolute;
    z-index: 30;
    touch-action: none;
    -webkit-app-region: no-drag;
  }
  .split.right,
  .split.left {
    top: 0;
    bottom: 0;
    width: 7px;
    cursor: col-resize;
  }
  .split.right {
    right: -4px;
  }
  .split.left {
    left: -4px;
  }
  .split.bottom {
    left: 0;
    right: 0;
    height: 7px;
    bottom: -4px;
    cursor: row-resize;
  }
  /* the line shows on hover, while dragging and on focus */
  .split::after {
    content: '';
    position: absolute;
    background: var(--line-strong);
    opacity: 0;
    transition: opacity 0.12s var(--ease);
  }
  .split.right::after,
  .split.left::after {
    top: 0;
    bottom: 0;
    left: 3px;
    width: 1px;
  }
  .split.bottom::after {
    left: 0;
    right: 0;
    top: 3px;
    height: 1px;
  }
  .split:hover::after,
  .split.on::after,
  .split:focus-visible::after {
    opacity: 1;
    background: var(--fg-3);
  }
  .split:focus-visible {
    outline: none;
  }
</style>
