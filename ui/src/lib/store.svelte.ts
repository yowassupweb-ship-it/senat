import { get, openEvents } from './api';
import type { Account, Agent, Attention, Backend, Company, GitStatus, LiveRun, Message, Project, Thread } from './types';

export type View =
  | { kind: 'new'; project?: string }
  | { kind: 'chat'; id: string }
  | { kind: 'agents'; id?: string }
  | { kind: 'projects'; id?: string }
  | { kind: 'companies'; id?: string }
  | { kind: 'obligations'; id?: string }
  | { kind: 'memory' }
  | { kind: 'mcp' }
  | { kind: 'api' }
  | { kind: 'access' }
  | { kind: 'settings' };

const emptyAttention: Attention = { approvals: [], needs_user: [], running: [], overdue: [], queued_sessions: 0, proposed_decisions: [] };

export const app = $state({
  ready: false,
  online: true,
  agents: [] as Agent[],
  projects: [] as Project[],
  companies: [] as Company[],
  accounts: [] as Account[],
  threads: [] as Thread[],
  backends: [] as Backend[],
  defaultBackend: 'mock',
  nodeId: '',
  version: '',
  attention: emptyAttention,
  /** Running and waiting sessions: who is working right now (drives the lit rings and the right panel). */
  live: [] as LiveRun[],
  view: { kind: 'new' } as View,
  messages: {} as Record<string, Message[]>,
  hasMore: {} as Record<string, boolean>,
  git: {} as Record<string, GitStatus>,
  sidebar: false,
  panel: false,
  palette: false,
  theme: 'auto' as 'auto' | 'light' | 'dark',
  drafts: {} as Record<string, string>,
});

export const attentionCount = () =>
  app.attention.approvals.length + app.attention.needs_user.length + app.attention.proposed_decisions.length;

export const agentById = (id?: string | null) => app.agents.find((a) => a.id === id);
export const projectById = (id?: string | null) => app.projects.find((p) => p.id === id);
export const threadById = (id?: string | null) => app.threads.find((t) => t.id === id);

// ---------- routing (hash) ----------
export function viewToHash(v: View): string {
  switch (v.kind) {
    case 'new':
      return v.project ? `#/new/${v.project}` : '#/';
    case 'chat':
      return `#/chat/${v.id}`;
    case 'memory':
    case 'mcp':
    case 'api':
    case 'access':
    case 'settings':
      return `#/${v.kind}`;
    default:
      return v.id ? `#/${v.kind}/${v.id}` : `#/${v.kind}`;
  }
}
export function hashToView(h: string): View {
  const [, kind, id] = h.replace(/^#/, '').split('/');
  switch (kind) {
    case 'chat':
      return id ? { kind: 'chat', id } : { kind: 'new' };
    case 'new':
      return { kind: 'new', project: id };
    case 'agents':
    case 'projects':
    case 'companies':
    case 'obligations':
      return { kind, id } as View;
    case 'memory':
    case 'mcp':
    case 'api':
    case 'access':
    case 'settings':
      return { kind };
    default:
      return { kind: 'new' };
  }
}
export function go(v: View) {
  app.sidebar = false;
  location.hash = viewToHash(v);
}

// ---------- loading ----------
export async function loadState() {
  const s = await get('/api/state');
  app.agents = s.agents;
  app.projects = s.projects;
  app.companies = s.companies ?? [];
  app.accounts = s.accounts ?? [];
  app.threads = s.threads;
  app.backends = s.backends;
  app.defaultBackend = s.default_backend;
  app.nodeId = s.node_id;
  app.version = s.version;
  app.attention = s.attention;
  app.ready = true;
  refreshLive(true);
}

let tLive: ReturnType<typeof setTimeout> | undefined;
/** Re-read who is working. Events come in dozens per second while someone writes: at most one read per 600 ms, replaced only on change. */
export function refreshLive(now = false) {
  clearTimeout(tLive);
  tLive = setTimeout(async () => {
    try {
      const next: LiveRun[] = await get('/api/live');
      const sig = (l: LiveRun[]) => JSON.stringify(l.map((r) => [r.session_id, r.status, r.phase, r.hint, r.awaiting, r.plan]));
      if (sig(next) !== sig(app.live)) app.live = next;
    } catch {
      /* offline: keep what is shown */
    }
  }, now ? 0 : 600);
}

let tThreads: ReturnType<typeof setTimeout> | undefined;
let tAgents: ReturnType<typeof setTimeout> | undefined;
export function refreshAgents() {
  clearTimeout(tAgents);
  tAgents = setTimeout(() => {
    get('/api/agents').then((a) => (app.agents = a)).catch(() => {});
  }, 1500);
}

export function refreshThreads() {
  clearTimeout(tThreads);
  tThreads = setTimeout(async () => {
    try {
      app.threads = await get('/api/threads');
    } catch {
      /* offline */
    }
  }, 150);
}
async function refreshAttention() {
  try {
    app.attention = await get('/api/attention');
  } catch {
    /* offline */
  }
}

export async function loadMessages(threadId: string, before?: string) {
  const r = await get(`/api/threads/${threadId}/messages${before ? `?before=${before}` : ''}`);
  const cur = app.messages[threadId] ?? [];
  app.messages[threadId] = before ? [...r.messages, ...cur] : r.messages;
  if (!before) app.hasMore[threadId] = r.has_more;
  else app.hasMore[threadId] = r.has_more;
}

export function upsertMessage(m: Message) {
  const list = app.messages[m.thread_id];
  if (!list) return;
  const i = list.findIndex((x) => x.id === m.id);
  if (i >= 0) list[i] = m;
  else list.push(m);
}

export async function loadGit(projectId: string) {
  try {
    app.git[projectId] = await get(`/api/projects/${projectId}/git`);
  } catch {
    app.git[projectId] = { available: false, reason: 'не удалось получить' };
  }
}

// ---------- live events ----------
let stop: (() => void) | null = null;
const listeners = new Set<(type: string, data: any) => void>();
export function onEvent(fn: (type: string, data: any) => void) {
  listeners.add(fn);
  return () => listeners.delete(fn);
}

export function startEvents() {
  stop?.();
  stop = openEvents(
    (type, data) => {
      if (type === 'message') {
        upsertMessage(data as Message);
        refreshThreads();
        if (data.status === 'done' || data.status === 'failed') {
          refreshAgents(); // finished work changes level and open tasks
          const t = threadById(data.thread_id);
          if (t?.project_id) loadGit(t.project_id);
        }
      } else if (type === 'thread') refreshThreads();
      else if (type === 'projects') {
        get('/api/projects').then((p) => (app.projects = p)).catch(() => {});
        get('/api/companies').then((c) => (app.companies = c)).catch(() => {});
      } else if (type === 'accounts') app.accounts = data;
      else if (type === 'agents') get('/api/agents').then((a) => (app.agents = a)).catch(() => {});
      else if (type === 'attention' || type === 'session') refreshAttention();
      if (type === 'message' || type === 'session' || type === 'thread') refreshLive();
      listeners.forEach((f) => f(type, data));
    },
    (online) => {
      const was = app.online;
      app.online = online;
      // After a reconnect, catch up on whatever was missed.
      if (online && !was && app.ready) {
        loadState().catch(() => {});
        const v = app.view;
        if (v.kind === 'chat') loadMessages(v.id).catch(() => {});
      }
    },
  );
}

export function applyTheme(t: 'auto' | 'light' | 'dark') {
  app.theme = t;
  if (t === 'auto') document.documentElement.removeAttribute('data-theme');
  else document.documentElement.setAttribute('data-theme', t);
  try {
    localStorage.setItem('senate.theme', t);
  } catch {
    /* storage unavailable */
  }
}
