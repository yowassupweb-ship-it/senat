import { get, openEvents } from './api';
import { readLink } from './link';
import { coalescedRefresh } from './refresh';
import type { Account, Agent, Attention, Backend, Company, GitStatus, LiveRun, Machine, Message, Project, Thread } from './types';

export type View =
  | { kind: 'new'; project?: string }
  | { kind: 'chat'; id: string }
  | { kind: 'agents'; id?: string }
  | { kind: 'projects'; id?: string }
  | { kind: 'companies'; id?: string }
  | { kind: 'obligations'; id?: string }
  | { kind: 'scheduled' }
  | { kind: 'cloud' }
  | { kind: 'memory' }
  | { kind: 'mcp' }
  | { kind: 'api' }
  | { kind: 'access' }
  | { kind: 'settings' };

const emptyAttention: Attention = { approvals: [], needs_user: [], running: [], overdue: [], queued_sessions: 0, proposed_decisions: [] };

export const app = $state({
  ready: false,
  online: true,
  /** Why the node is not reachable, when the desktop knows (it owns the tunnel to a server node). */
  linkDetail: '',
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
  /** Which side of the project panel is shown: the folder's files or what changed in it. */
  panelTab: (() => {
    try {
      return localStorage.getItem('senate.panelTab') === 'changes' ? 'changes' : 'files';
    } catch {
      return 'files';
    }
  })() as 'files' | 'changes',
  /** The person's computers that run the Senate app (where project folders live). */
  machines: [] as Machine[],
  nodeMachine: '',
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
    case 'scheduled':
    case 'cloud':
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
    case 'scheduled':
    case 'cloud':
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
  loadMachines();
}

export async function loadMachines() {
  try {
    const r = await get('/api/machines');
    app.machines = r.machines ?? [];
    app.nodeMachine = r.node_machine ?? '';
  } catch {
    // an older node has no computers list: the panel then reads folders on the node
  }
}

/** The computer a project's folder is on (null: on the node itself). */
export const machineOf = (p?: Project | null) => (p?.node_id ? app.machines.find((m) => m.id === p.node_id) ?? { id: p.node_id, name: 'Компьютер', os: '', online: false, env: null, last_seen: null } : null);

/** Re-read who is working. Events come in dozens per second while someone writes: at most one read per 600 ms, replaced only on change. */
export const refreshLive = coalescedRefresh(async () => {
      const next: LiveRun[] = await get('/api/live');
      const sig = (l: LiveRun[]) => JSON.stringify(l);
      if (sig(next) !== sig(app.live)) app.live = next;
}, 600);

export const refreshAgents = coalescedRefresh(async () => {
  app.agents = await get('/api/agents');
}, 1500);

export const refreshThreads = coalescedRefresh(async () => {
  app.threads = await get('/api/threads');
}, 150);
const refreshAttention = coalescedRefresh(async () => {
    app.attention = await get('/api/attention');
}, 150);

export async function loadMessages(threadId: string, before?: string) {
  const r = await get(`/api/threads/${threadId}/messages?limit=30${before ? `&before=${encodeURIComponent(before)}` : ''}`);
  const cur = app.messages[threadId] ?? [];
  const merged = new Map(cur.map(m => [m.id, m]));
  for (const m of r.messages as Message[]) {
    const existing = merged.get(m.id);
    if (!existing || m.updated_at >= existing.updated_at) merged.set(m.id, m);
  }
  app.messages[threadId] = [...merged.values()].sort((a, b) => a.created_at - b.created_at);
  // Refreshing the latest page must not reset the cursor of older pages already loaded.
  if (before || !cur.length || app.hasMore[threadId] === undefined) app.hasMore[threadId] = r.has_more;
}

// While the person has text selected inside a message, updates to that message wait: a specialist streaming
// several times a second would otherwise redraw the text under the selection and drop it.
const held = new Map<string, Message>();
function selectingIn(id: string): boolean {
  if (typeof document === 'undefined') return false;
  const sel = document.getSelection();
  if (!sel || sel.isCollapsed || !sel.anchorNode) return false;
  const el = (sel.anchorNode instanceof Element ? sel.anchorNode : sel.anchorNode.parentElement)?.closest('[data-mid]');
  return el?.getAttribute('data-mid') === id;
}
if (typeof document !== 'undefined') {
  document.addEventListener('selectionchange', () => {
    for (const [id, m] of [...held]) {
      if (!selectingIn(id)) {
        held.delete(id);
        upsertMessage(m);
      }
    }
  });
}

export function upsertMessage(m: Message) {
  const list = app.messages[m.thread_id];
  if (!list) return;
  const i = list.findIndex((x) => x.id === m.id);
  if (i >= 0 && list[i].updated_at > m.updated_at) return;
  if (i >= 0 && m.status !== 'done' && m.status !== 'failed' && selectingIn(m.id)) {
    held.set(m.id, m);
    return;
  }
  held.delete(m.id);
  // a finished answer may move to the end of the chat (the node gives it its finishing time): keep the order
  if (i >= 0 && list[i].created_at === m.created_at) Object.assign(list[i], m);
  else {
    if (i >= 0) list.splice(i, 1);
    let at = list.length;
    while (at > 0 && list[at - 1].created_at > m.created_at) at--;
    list.splice(at, 0, m);
  }
}

export async function loadGit(projectId: string) {
  try {
    app.git[projectId] = await get(`/api/projects/${projectId}/git`);
  } catch {
    app.git[projectId] = { available: false, reason: 'не удалось получить' };
  }
}

/** Git state of whatever folder a chat works in (its project's folder or the specialist's own workspace). */
// A chat whose folder is not a repository can look at one inside it (the specialists' own folders hold the code a
// level down). The choice belongs to the chat and to this browser.
export function threadRepo(threadId: string): string {
  try {
    return localStorage.getItem(`senate.repo.${threadId}`) || '';
  } catch {
    return '';
  }
}
export function pickThreadRepo(threadId: string, repo: string): Promise<void> {
  try {
    if (repo) localStorage.setItem(`senate.repo.${threadId}`, repo);
    else localStorage.removeItem(`senate.repo.${threadId}`);
  } catch {
    // without storage the choice lasts until the page is reloaded
  }
  return loadThreadGit(threadId, repo);
}
export async function loadThreadGit(threadId: string, picked?: string): Promise<void> {
  try {
    const repo = picked ?? threadRepo(threadId);
    const r = await get(`/api/threads/${threadId}/git${repo ? `?repo=${encodeURIComponent(repo)}` : ''}`);
    // the repository picked earlier is gone: back to the folder itself and what it offers now
    if (repo && !r.available && picked === undefined) return pickThreadRepo(threadId, '');
    app.git['t:' + threadId] = { ...r, repo: r.available ? repo : '' };
  } catch {
    // keep the last known state: a failed refresh must not make the git bar vanish
  }
}

// ---------- live events ----------
let stop: (() => void) | null = null;
const listeners = new Set<(type: string, data: any) => void>();
export function onEvent(fn: (type: string, data: any) => void) {
  listeners.add(fn);
  return () => listeners.delete(fn);
}

// many events in a row (a council of several colleagues) cost one fetch of the chat
const syncTimers = new Map<string, ReturnType<typeof setTimeout>>();
function syncThread(id: string) {
  if (syncTimers.has(id)) return;
  syncTimers.set(
    id,
    setTimeout(() => {
      syncTimers.delete(id);
      loadMessages(id).catch(() => {});
    }, 400),
  );
}

export function startEvents() {
  stop?.();
  stop = openEvents(
    (type, data) => {
      if (type === 'message') {
        const m = data as Message;
        const known = app.messages[m.thread_id];
        const previous = known?.find((x) => x.id === m.id);
        const changedStatus = !previous || previous.status !== m.status;
        // a colleague's answer or a council turn whose call was written on the server without an event of its own:
        // fetch the chat, so the answer lands under its call and not on its own
        const p = (m.props ?? {}) as any;
        if (known && !known.some((x) => x.id === m.id) && (p.asked_by || p.reply_from || p.council || p.council_reply)) syncThread(m.thread_id);
        upsertMessage(m);
        if (changedStatus) refreshThreads();
        if (data.status === 'done' || data.status === 'failed') {
          refreshAgents(); // finished work changes level and open tasks
          const t = threadById(data.thread_id);
          if (t?.project_id) loadGit(t.project_id);
          loadThreadGit(data.thread_id);
        }
      } else if (type === 'thread') {
        refreshThreads();
        // something changed inside an open chat (a call, a note): bring it up to date
        if (data?.id && app.messages[data.id]) syncThread(data.id);
      }
      else if (type === 'projects') {
        get('/api/projects').then((p) => (app.projects = p)).catch(() => {});
        get('/api/companies').then((c) => (app.companies = c)).catch(() => {});
      } else if (type === 'accounts') app.accounts = data;
      else if (type === 'agents') get('/api/agents').then((a) => (app.agents = a)).catch(() => {});
      else if (type === 'attention' || type === 'session') refreshAttention();
      else if (type === 'machines') loadMachines();
      if (type === 'message' || type === 'session' || type === 'thread') refreshLive();
      listeners.forEach((f) => f(type, data));
    },
    (online) => {
      const was = app.online;
      app.online = online;
      if (online) app.linkDetail = '';
      else readLink().then((l) => (app.linkDetail = l?.detail || ''));
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
