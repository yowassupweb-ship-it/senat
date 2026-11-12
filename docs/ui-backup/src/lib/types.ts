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
  limits: { windows: Record<string, { used: number; resets_at: number }>; at: number } | null;
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
  mode: string;
  open_obligations?: number;
  members?: Agent[];
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
  state: 'pending' | 'allowed' | 'denied';
}

export interface MessageProps {
  steps?: Step[];
  phase?: string | null;
  model?: string;
  backend?: string;
  work?: Work;
  ask?: Ask | null;
  error?: string;
  rateLimit?: any;
  attachments?: { name: string; url?: string; size?: number; type?: string }[];
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

export interface GitStatus {
  available: boolean;
  reason?: string;
  branch?: string;
  changes?: { path: string; status: 'new' | 'modified' | 'deleted' | 'added' | 'renamed' }[];
  stat?: string;
  last_commit?: string;
}
