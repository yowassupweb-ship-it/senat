// Registry of Senate nodes this device knows: the desktop's own node, servers, a colleague's node.
// A node is identified by address + token; a connect link carries both, so moving to another
// computer is "open the link" (or paste it into the connection screen).

export interface SavedNode {
  id: string;
  name: string;
  /** Empty = the origin that serves this page. */
  url: string;
  token: string;
}

const LS = 'senate.nodes';
const LS_ACTIVE = 'senate.active';

function read(): SavedNode[] {
  try {
    const v = JSON.parse(localStorage.getItem(LS) || '[]');
    return Array.isArray(v) ? v.filter((n) => n && typeof n.id === 'string' && typeof n.token === 'string') : [];
  } catch {
    return [];
  }
}
function write(list: SavedNode[]) {
  try {
    localStorage.setItem(LS, JSON.stringify(list));
  } catch {
    /* storage unavailable */
  }
}

export const savedNodes = read;
export const activeNodeId = () => {
  try {
    return localStorage.getItem(LS_ACTIVE) || '';
  } catch {
    return '';
  }
};
export function setActiveNode(id: string) {
  try {
    if (id) localStorage.setItem(LS_ACTIVE, id);
    else localStorage.removeItem(LS_ACTIVE);
  } catch {
    /* storage unavailable */
  }
}

const norm = (u: string) => u.trim().replace(/\/+$/, '');
const rid = () => Math.random().toString(36).slice(2, 10);

/** Adds a node (or updates the one with the same address) and returns it. */
export function upsertNode(n: { name?: string; url: string; token: string }): SavedNode {
  const url = norm(n.url);
  const list = read();
  const found = list.find((x) => x.url === url);
  if (found) {
    found.token = n.token;
    if (n.name) found.name = n.name;
    write(list);
    return found;
  }
  const node: SavedNode = { id: rid(), name: n.name || (url ? url.replace(/^https?:\/\//, '') : 'Эта нода'), url, token: n.token };
  write([...list, node]);
  return node;
}
export function removeNode(id: string) {
  write(read().filter((n) => n.id !== id));
  if (activeNodeId() === id) setActiveNode('');
}
export function renameNode(id: string, name: string) {
  write(read().map((n) => (n.id === id ? { ...n, name } : n)));
}

const b64 = (s: string) => btoa(String.fromCharCode(...new TextEncoder().encode(s))).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
const unb64 = (s: string) => {
  const p = s.replace(/-/g, '+').replace(/_/g, '/');
  const bin = atob(p + '='.repeat((4 - (p.length % 4)) % 4));
  return new TextDecoder().decode(Uint8Array.from(bin, (c) => c.charCodeAt(0)));
};

/** A link that connects any device to this node: https://node/#connect=… */
export function makeLink(n: { name: string; url: string; token: string }, origin: string): string {
  const url = norm(n.url) || origin;
  return `${url}/#connect=${b64(JSON.stringify({ u: url, t: n.token, n: n.name }))}`;
}

/** Accepts a connect link, `senate://connect/<payload>`, or the bare payload. */
export function parseLink(text: string): { name: string; url: string; token: string } | null {
  const m = /connect[=/]([A-Za-z0-9_-]+)/.exec(text.trim()) || /^([A-Za-z0-9_-]{20,})$/.exec(text.trim());
  if (!m) return null;
  try {
    const v = JSON.parse(unb64(m[1]));
    if (typeof v.t !== 'string' || !v.t) return null;
    return { name: String(v.n || ''), url: typeof v.u === 'string' ? v.u : '', token: v.t };
  } catch {
    return null;
  }
}

export type Reach = 'ok' | 'auth' | 'down';
/** Is the node reachable, and does it accept the token. */
export async function pingNode(n: { url: string; token: string }): Promise<Reach> {
  try {
    const r = await fetch(norm(n.url) + '/api/state', { headers: { Authorization: `Bearer ${n.token}` }, cache: 'no-store', signal: AbortSignal.timeout(6000) });
    return r.ok ? 'ok' : r.status === 401 ? 'auth' : 'down';
  } catch {
    return 'down';
  }
}
