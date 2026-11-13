export interface Agent {
  id: string;
  slug: string;
  name: string;
  role: string;
  profile: string;
  accent: string;
  model_pref: string;
  open_obligations?: number;
  running?: number;
  blocked?: number;
  level?: number;
  xp?: number;
  next_level_xp?: number;
  /** Address of the picture ('' = none: the first letter on the accent colour). */
  avatar?: string;
  /** 'm' | 'f' | '' (not set): how the specialist speaks about themselves. */
  gender?: string;
}

export interface Company {
  id: string;
  name: string;
  brief: string;
  color: string;
  status: string;
  projects?: number;
  open_obligations?: number;
}

export interface Account {
  provider: 'claude' | 'codex' | 'kimi';
  label: string;
  installed: boolean;
  version: string;
  state: 'none' | 'pending' | 'ready';
  account: string;
  models: { id: string; label: string }[];
  login: { state: string; url: string; code: string; needs_code: boolean; message: string } | null;
  limits: { windows: Record<string, { used: number; resets_at: number }>; at: number; plan?: string } | null;
  latest_version: string;
  update_available: boolean;
  checked_at: number;
}

export interface McpServer {
  id: string;
  name: string;
  kind: 'stdio' | 'http';
  command: string;
  args: string[];
  url: string;
  note: string;
  enabled: boolean;
  trusted: boolean;
  shared: boolean;
  agents: string[];
  env_keys: string[];
  header_keys: string[];
  missing: string[];
}

export interface Integration {
  id: string;
  name: string;
  label: string;
  base_url: string;
  auth_type: 'none' | 'bearer' | 'header' | 'basic' | 'query';
  auth_name: string;
  has_secret: boolean;
  docs: string;
  write_mode: 'deny' | 'approve' | 'auto';
  enabled: boolean;
  shared: boolean;
  agents: string[];
}

/** Work a specialist does on a schedule. */
export interface Routine {
  id: string;
  agent_id: string;
  agent_name?: string;
  project_id: string | null;
  name: string;
  cron: string;
  title: string;
  prompt: string;
  enabled: boolean;
  /** '' = made by the person; 'skill:<name>' = declared by a skill */
  source: string;
  last_run_at: number | null;
  next_run_at: number | null;
}

/** One line of the calendar: what a schedule will do, what it made, or a deadline. */
export interface CalendarItem {
  kind: 'planned' | 'run' | 'due';
  at: number;
  agent_id: string | null;
  title: string;
  routine_id?: string;
  obligation_id?: string;
  state?: string;
  project_id?: string | null;
}

/** A server, database or site the person lets specialists use. The secret never comes to the UI. */
export interface Access {
  id: string;
  name: string;
  label: string;
  kind: 'server' | 'database' | 'site' | 'service' | 'other';
  host: string;
  login: string;
  has_secret: boolean;
  note: string;
  enabled: boolean;
  /** specialist id → 'allow' | 'ask' (absent: no access) */
  grants: Record<string, 'allow' | 'ask'>;
}

export interface ForumMeta {
  topic?: string;
  outcome?: 'plan' | 'decision' | 'analysis';
  participants?: string[];
  rounds?: number;
  round?: number;
  status?: 'open' | 'summarizing' | 'concluded' | 'stopped';
  leader?: string;
  plan?: any;
  plan_applied?: boolean;
  /** Set by the specialist (`senate_preview`): the person's local dev server, shown as a button under the composer. */
  /** `on: 'node'` — the dev server runs on the server: opened through the node, not on this computer. */
  preview?: { url: string; label: string; on?: 'node' | 'machine' };
  /** The person stopped the work because they disagree with the plan; their next message is the objection. */
  plan_hold?: { todos: { content: string; status: string }[]; at: number };
}

export interface Project {
  id: string;
  company_id?: string | null;
  name: string;
  goal: string;
  criteria: string;
  constraints: string;
  status: string;
  root_path: string;
  /** The computer the folder is on; null: the node itself. */
  node_id?: string | null;
  mode: string;
  open_obligations?: number;
  members?: Agent[];
}

/** One of the person's computers that runs the Senate app. */
export interface Machine {
  id: string;
  name: string;
  os: string;
  online: boolean;
  env: { tools?: Record<string, string>; missing?: string[]; os?: string } | null;
  last_seen: number | null;
  this_node?: boolean;
}

/** One entry of a project folder. */
export interface TreeEntry {
  name: string;
  path: string;
  dir: boolean;
  size: number;
  heavy: boolean;
  hidden: boolean;
}

export type Mode = 'plan' | 'ask' | 'edit' | 'auto';

export interface Thread {
  id: string;
  kind: 'lounge' | 'chat' | 'forum';
  company_id?: string | null;
  meta?: ForumMeta;
  project_id: string | null;
  agent_id: string | null;
  title: string;
  archived: boolean;
  pinned: boolean;
  model: string;
  effort: string;
  mode: string;
  created_at: number;
  updated_at: number;
  last_text?: string;
  last_role?: string;
  working?: boolean;
  awaiting?: boolean;
}

export interface Step {
  kind: 'tool' | 'text';
  id?: string;
  name?: string;
  hint?: string;
  input?: any;
  output?: string;
  is_error?: boolean;
  ms?: number;
  status?: 'running' | 'done' | 'denied' | 'awaiting';
  patch?: any;
  text?: string;
}

export interface Work {
  input_tokens?: number;
  cached_input_tokens?: number;
  output_tokens?: number;
  thinking_tokens?: number;
  context_tokens?: number;
  context_window?: number;
  duration_ms?: number;
  turns?: number;
}

export interface Ask {
  kind: 'permission';
  toolUseId: string;
  tool?: string;
  input?: any;
  question?: string;
  /** What "Allow always" would remember for this question; null/absent: it is asked about every time. */
  always?: string | null;
  /** "Allow work in this folder": the folder and what it covers; null/absent when the question is not about one. */
  dir?: { label: string; path: string } | null;
  state: 'pending' | 'allowed' | 'denied';
}

/** "Allow always": a decision the person made not to be asked again. */
export interface PermissionRule {
  id: string;
  kind: 'path' | 'cmd' | 'tool' | 'dir';
  value: string;
  label: string;
  created_at: number;
}

export interface MessageProps {
  /** This failed turn was run again: the new message. */
  retried_by?: string;
  /** A question the specialist asked the person mid-work (senate_ask_person). */
  question?: { id: string; text: string; options?: string[]; state: 'pending' | 'answered' | 'expired'; answer?: string };
  steps?: Step[];
  phase?: string | null;
  model?: string;
  backend?: string;
  work?: Work;
  ask?: Ask | null;
  error?: string;
  rateLimit?: any;
  attachments?: { id?: string; name: string; url?: string; size?: number; type?: string }[];
  replyTo?: { id: string; actor: string; text: string };
  agent?: string;
  sessionId?: string;
  reason?: string;
  learning?: boolean;
  learned?: { id: string; body: string; kind: string; level: string; auto: boolean; company?: string | null }[];
  learn_error?: string;
  fallback?: string;
  forum?: { round: number; synthesis: boolean };
  plan?: any;
  summary?: boolean;
  /** This message is one specialist calling a colleague: who was called. */
  call?: { to: string };
  /** This run answers a colleague's call: who asked, and what they asked. */
  asked_by?: string;
  request?: string;
  /** This run continues after a colleague answered. */
  reply_from?: string;
}

export interface Message {
  id: string;
  thread_id: string;
  author_kind: 'user' | 'agent' | 'system';
  author_id: string | null;
  body: string;
  props: MessageProps;
  status: 'queued' | 'working' | 'done' | 'failed' | 'cancelled';
  created_at: number;
  updated_at: number;
}

export interface Obligation {
  id: string;
  title: string;
  result: string;
  criteria: string;
  owner_id: string | null;
  project_id: string | null;
  parent_id: string | null;
  requested_by: string;
  state: 'accepted' | 'running' | 'waiting' | 'blocked' | 'review' | 'done' | 'cancelled';
  next_step: string;
  wait_condition: any;
  blocker: string;
  outcome: string;
  due_at: number | null;
  spent_tokens: number;
  created_at: number;
  updated_at: number;
}

export interface Memory {
  id: string;
  level: 'personal' | 'project' | 'company' | 'collective';
  agent_id: string | null;
  project_id: string | null;
  kind: 'observation' | 'hypothesis' | 'verified' | 'decision' | 'preference' | 'error';
  company_id?: string | null;
  body: string;
  source: string;
  scope_note: string;
  verification: string;
  status: string;
  created_at: number;
  updated_at: number;
}

export interface Decision {
  id: string;
  company_id?: string | null;
  project_id: string | null;
  title: string;
  body: string;
  status: 'proposed' | 'accepted' | 'rejected' | 'superseded';
  decided_by: string;
  created_at: number;
}

export interface Approval {
  id: string;
  agent_id: string;
  action: string;
  detail: any;
  status: string;
  created_at: number;
}

export interface Backend {
  backend: string;
  label: string;
  version: string;
  ready: boolean;
  models: { id: string; label: string }[];
}

export interface Attention {
  approvals: Approval[];
  needs_user: Obligation[];
  running: Obligation[];
  overdue: Obligation[];
  queued_sessions: number;
  proposed_decisions: Decision[];
}

/** One running or waiting session, from /api/live: what the right-hand panel shows. */
export interface LiveRun {
  session_id: string;
  agent_id: string;
  kind: 'chat' | 'task' | 'run';
  title: string;
  thread_id: string | null;
  obligation_id: string | null;
  message_id: string | null;
  status: 'running' | 'queued';
  since: number;
  phase: string;
  hint: string;
  awaiting: boolean;
  /** A permission they wait for: answered right in the panel. */
  ask?: { question?: string; tool?: string; command?: string; file?: string; always?: string | null; dir?: { label: string; path: string } | null } | null;
  /** A question to the person (senate_ask_person). */
  question?: { text: string; options?: string[] } | null;
  plan: { done: number; total: number; current: string } | null;
}

export interface GitStatus {
  available: boolean;
  reason?: string;
  branch?: string;
  changes?: { path: string; status: 'new' | 'modified' | 'deleted' | 'added' | 'renamed' }[];
  stat?: string;
  last_commit?: string;
  /** commits not pushed / not pulled yet; null without an upstream branch */
  ahead?: number | null;
  behind?: number | null;
}
