import { api } from './api';
import { app, refreshThreads, upsertMessage } from './store.svelte';
import type { Uploaded } from './api';

export interface Outgoing {
  id: string;
  group: string;
  threadId?: string;
  projectId?: string;
  agentId?: string;
  text: string;
  attachments: Uploaded[];
  mode?: string;
  effort?: string;
  state: 'queued' | 'sending' | 'failed';
  error?: string;
}

export const outbox = $state({ items: [] as Outgoing[] });
let storageKey = '';
const active = new Set<string>();
const created = new Map<string, (id: string) => void>();
function persist() {
  if (!storageKey) return;
  localStorage.setItem(storageKey, JSON.stringify(outbox.items));
}
export function initOutbox() {
  const key = `senate.outbox:${app.nodeId}`;
  if (!app.nodeId || storageKey === key) return;
  storageKey = key;
  try {
    const saved = JSON.parse(localStorage.getItem(key) || '[]');
    outbox.items = Array.isArray(saved) ? saved.filter(i => i?.id && i?.group && typeof i.text === 'string').map(i => ({ ...i, state: i.state === 'sending' ? 'failed' : i.state, error: i.state === 'sending' ? 'Не получено подтверждение. Повторите отправку — дубликата не будет.' : i.error })) : [];
  } catch { outbox.items = []; }
  for (const group of new Set(outbox.items.map(i => i.group))) void pump(group);
}
export function enqueue(input: Omit<Outgoing, 'id' | 'state'>, onCreated?: (id: string) => void) {
  initOutbox();
  const group = input.threadId ? outbox.items.find(i => i.threadId === input.threadId)?.group ?? input.threadId : outbox.items.find(i => !i.threadId && i.projectId === input.projectId)?.group ?? input.group;
  const id = typeof crypto.randomUUID === 'function' ? crypto.randomUUID() : Array.from(crypto.getRandomValues(new Uint8Array(16)), n => n.toString(16).padStart(2, '0')).join('');
  const item: Outgoing = { ...input, group, id, state: 'queued' };
  outbox.items.push(item);
  // If storage is full, do not clear the composer's only copy of the text.
  try { persist(); } catch { outbox.items = outbox.items.filter(i => i.id !== item.id); throw new Error('Не удалось сохранить очередь на этом устройстве. Освободите место и повторите.'); }
  if (onCreated) created.set(item.group, onCreated);
  void pump(item.group);
}
export function retryOutgoing(id: string) {
  const item = outbox.items.find(i => i.id === id);
  if (!item || item.state !== 'failed') return;
  item.state = 'queued'; item.error = undefined;
  persist(); void pump(item.group);
}
export function removeOutgoing(id: string) {
  const item = outbox.items.find(i => i.id === id);
  if (!item || item.state === 'sending') return;
  outbox.items = outbox.items.filter(i => i.id !== id);
  persist(); void pump(item.group);
}
async function pump(group: string) {
  if (active.has(group)) return;
  active.add(group);
  try {
    while (true) {
      const item = outbox.items.find(i => i.group === group);
      if (!item || item.state !== 'queued') break;
      item.state = 'sending';
      try {
        persist();
        if (!item.threadId) {
          const t = await api('/api/threads', { method: 'POST', body: { reuse_project: true, project_id: item.projectId ?? null, agent_id: item.agentId || null }, timeoutMs: 30000 });
          for (const pending of outbox.items.filter(i => i.group === group)) pending.threadId = t.id;
          app.messages[t.id] ??= [];
          if (!app.threads.some(x => x.id === t.id)) app.threads.unshift(t);
          persist();
          created.get(group)?.(t.id);
          created.delete(group);
        }
        const d = await api(`/api/threads/${item.threadId}/messages`, { method: 'POST', timeoutMs: 30000, body: {
          client_id: item.id, text: item.text,
          attachments: item.attachments.filter(a => a.id).map(a => ({ id: a.id })),
          mode: item.mode || undefined, effort: item.effort || undefined,
        } });
        app.messages[item.threadId!] ??= [];
        upsertMessage(d.user_message);
        d.agent_messages.forEach(upsertMessage);
        outbox.items = outbox.items.filter(i => i.id !== item.id);
        persist(); refreshThreads();
      } catch (error) {
        item.state = 'failed';
        item.error = error instanceof Error ? error.message : 'Не удалось отправить';
        try { persist(); } catch { /* keep the in-memory copy */ }
        break;
      }
    }
  } finally { active.delete(group); }
}
