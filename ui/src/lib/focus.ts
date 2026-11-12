/** Keep keyboard focus inside a sheet and restore its opener on dismissal. */
export function focusScope(node: HTMLElement, close: () => void) {
  const previous = document.activeElement as HTMLElement | null;
  const items = () => Array.from(node.querySelectorAll<HTMLElement>('button:not(:disabled), input:not(:disabled), textarea:not(:disabled), select:not(:disabled), [tabindex="0"]')).filter(el => el.getClientRects().length);
  queueMicrotask(() => (items()[0] ?? node).focus());
  const keydown = (e: KeyboardEvent) => {
    if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); close(); }
    if (e.key !== 'Tab') return;
    const all = items();
    const first = all[0];
    const last = all[all.length - 1];
    if (!first) { e.preventDefault(); node.focus(); }
    else if (e.shiftKey && (document.activeElement === first || document.activeElement === node)) { e.preventDefault(); last.focus(); }
    else if (!e.shiftKey && document.activeElement === last) { e.preventDefault(); first.focus(); }
  };
  node.addEventListener('keydown', keydown);
  return { destroy() { node.removeEventListener('keydown', keydown); if (previous?.isConnected) previous.focus(); } };
}
