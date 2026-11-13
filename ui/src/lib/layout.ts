// Sizes of the app's areas the person drags to their liking: kept as CSS variables on the page and remembered.
export type Pane = { key: string; cssVar: string; def: number; min: number; max: number };

export const PANES: Record<string, Pane> = {
  sidebar: { key: 'sidebar', cssVar: '--sidebar-w', def: 260, min: 200, max: 460 },
  panel: { key: 'panel', cssVar: '--panel-w', def: 400, min: 300, max: 900 },
  live: { key: 'live', cssVar: '--live-w', def: 300, min: 240, max: 560 },
  tree: { key: 'tree', cssVar: '--tree-h', def: 42, min: 15, max: 85 }, // percent of the files panel height
};

const KEY = (p: Pane) => `senate.size.${p.key}`;

export function sizeOf(p: Pane): number {
  try {
    const v = Number(localStorage.getItem(KEY(p)));
    if (v >= p.min && v <= p.max) return v;
  } catch {
    // storage may be unavailable: defaults then
  }
  return p.def;
}

export function applySize(p: Pane, v: number, remember = true) {
  const n = Math.round(Math.min(p.max, Math.max(p.min, v)));
  document.documentElement.style.setProperty(p.cssVar, p.key === 'tree' ? `${n}%` : `${n}px`);
  if (remember) {
    try {
      if (n === p.def) localStorage.removeItem(KEY(p));
      else localStorage.setItem(KEY(p), String(n));
    } catch {
      // not remembered, still applied
    }
  }
  return n;
}

/** Put every remembered size in place at start. */
export function restoreSizes() {
  for (const p of Object.values(PANES)) applySize(p, sizeOf(p), false);
}
