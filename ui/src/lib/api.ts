// Talks to a Senate node: the one that serves this page, or a remote one chosen in settings.
// The desktop shell injects `window.__SENATE__ = { token }` for its embedded node.

const LS_TOKEN = 'senate.token';
const LS_HUB = 'senate.hub';

let token = '';
let base = '';

function safeGet(k: string): string {
  try {
    return localStorage.getItem(k) || '';
  } catch {
    return '';
  }
}
function safeSet(k: string, v: string) {
  try {
    if (v) localStorage.setItem(k, v);
    else localStorage.removeItem(k);
  } catch {
    /* storage can be unavailable */
  }
}

export function bootstrapConnection(): boolean {
  const injected = (window as any).__SENATE__;
  const m = /[#&]token=([A-Za-z0-9_-]+)/.exec(location.hash);
  if (injected?.token) token = injected.token;
  else if (m) token = m[1];
  else token = safeGet(LS_TOKEN);
  if (m) {
    history.replaceState(null, '', location.pathname + location.search + location.hash.replace(/[#&]?token=[A-Za-z0-9_-]+/, '').replace(/^&/, '#'));
  }
  if (token && !injected?.token) safeSet(LS_TOKEN, token);
  base = injected?.base || safeGet(LS_HUB) || '';
  return !!token;
}

export function setConnection(hub: string, tok: string) {
  token = tok;
  base = hub.replace(/\/+$/, '');
  safeSet(LS_TOKEN, tok);
  safeSet(LS_HUB, base);
}
export function disconnect() {
  safeSet(LS_TOKEN, '');
  safeSet(LS_HUB, '');
  token = '';
  base = '';
}
export const hubUrl = () => base || location.origin;

export class ApiError extends Error {
  constructor(
    public status: number,
    message: string,
  ) {
    super(message);
  }
}

export async function api<T = any>(path: string, init: { method?: string; body?: unknown } = {}): Promise<T> {
  let res: Response;
  try {
    res = await fetch(base + path, {
      method: init.method || (init.body !== undefined ? 'POST' : 'GET'),
      headers: { Authorization: `Bearer ${token}`, ...(init.body !== undefined ? { 'Content-Type': 'application/json' } : {}) },
      body: init.body !== undefined ? JSON.stringify(init.body) : undefined,
      cache: 'no-store',
    });
  } catch {
    throw new ApiError(0, 'Нет связи с Senate');
  }
  if (!res.ok) {
    let msg = res.statusText;
    try {
      msg = (await res.json()).error || msg;
    } catch {
      /* not json */
    }
    throw new ApiError(res.status, msg);
  }
  return res.json();
}

export const get = <T = any>(p: string) => api<T>(p);
export const post = <T = any>(p: string, body: unknown = {}) => api<T>(p, { method: 'POST', body });
export const patch = <T = any>(p: string, body: unknown) => api<T>(p, { method: 'PATCH', body });
export const put = <T = any>(p: string, body: unknown) => api<T>(p, { method: 'PUT', body });
export const del = <T = any>(p: string) => api<T>(p, { method: 'DELETE' });

/** What the node returns for a stored file; messages refer to it by `id`. */
export interface Uploaded {
  id: string;
  name: string;
  type: string;
  size: number;
  url: string;
}

/** Send one file (a screenshot from the clipboard, a picked file). The body is the file itself. */
export async function upload(file: File | Blob, name?: string): Promise<Uploaded> {
  const fileName = name || (file as File).name || 'file';
  let res: Response;
  try {
    res = await fetch(base + '/api/uploads', {
      method: 'POST',
      headers: { Authorization: `Bearer ${token}`, 'X-Filename': encodeURIComponent(fileName), 'Content-Type': file.type || 'application/octet-stream' },
      body: file,
    });
  } catch {
    throw new ApiError(0, 'Нет связи с Senate');
  }
  if (!res.ok) {
    let msg = res.statusText;
    try {
      msg = (await res.json()).error || msg;
    } catch {
      /* not json */
    }
    throw new ApiError(res.status, msg);
  }
  return res.json();
}

/** Address of a stored file for <img> and links: they cannot send the Authorization header, so the token rides in the query. */
export const fileUrl = (url: string) => `${base}${url}?token=${encodeURIComponent(token)}`;

/** Server-sent events with automatic reconnect. */
export function openEvents(onEvent: (type: string, data: any) => void, onState: (online: boolean) => void): () => void {
  let es: EventSource | null = null;
  let closed = false;
  let retry: ReturnType<typeof setTimeout> | undefined;
  const connect = () => {
    if (closed) return;
    es = new EventSource(`${base}/api/events?token=${encodeURIComponent(token)}`);
    es.onopen = () => onState(true);
    es.onmessage = (e) => {
      try {
        const m = JSON.parse(e.data);
        onEvent(m.type, m.data);
      } catch {
        /* ignore malformed */
      }
    };
    es.onerror = () => {
      onState(false);
      es?.close();
      retry = setTimeout(connect, 2000);
    };
  };
  connect();
  return () => {
    closed = true;
    clearTimeout(retry);
    es?.close();
  };
}
