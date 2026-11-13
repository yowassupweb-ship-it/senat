// Moves chats that exist only in a local Senate database to the server node (through the tunnel the desktop
// keeps, or any URL). Shows the plan by default; --apply sends it. Additive and repeatable on the node side.
//
//   node scripts/push-local-chats.mjs [path\to\senate.db] [--apply] [--merge] [--archived]
//     --merge     also add messages missing from chats that exist on both sides
//     --archived  include archived chats
// Read the database from a copy, not from under a running node: copy senate.db, -wal and -shm together.
import { DatabaseSync } from 'node:sqlite';
import { readFileSync, existsSync } from 'node:fs';

const args = process.argv.slice(2);
const flag = (f) => args.includes(f);
const dbPath = args.find((a) => !a.startsWith('--')) || `${process.env.APPDATA}/Senate/senate.db`;
if (!existsSync(dbPath)) throw new Error(`нет базы ${dbPath}`);
const remote = JSON.parse(readFileSync(`${process.env.APPDATA}/Senate/remote.json`, 'utf8').replace(/^﻿/, ''));
const base = (process.env.SENATE_URL || remote.url || 'http://127.0.0.1:7777').replace(/\/$/, '');
const H = { Authorization: `Bearer ${remote.token}`, 'Content-Type': 'application/json' };

const db = new DatabaseSync(dbPath, { readOnly: true });
const threads = db.prepare(`SELECT * FROM threads WHERE kind IN ('chat','forum') ${flag('--archived') ? '' : 'AND archived=0'} ORDER BY updated_at`).all();
const msgsOf = db.prepare('SELECT * FROM messages WHERE thread_id=? ORDER BY created_at');

const res = await fetch(`${base}/api/state`, { headers: H });
if (!res.ok) throw new Error(`сервер не ответил: ${res.status}`);
const onServer = new Set((await res.json()).threads.map((t) => t.id));
// archived chats are not in /api/state; the node skips what it already has anyway

const plan = threads.filter((t) => flag('--merge') || !onServer.has(t.id)).map((t) => ({ thread: t, messages: msgsOf.all(t.id) }));
if (!plan.length) {
  console.log('Переносить нечего: все чаты уже на сервере.');
  process.exit(0);
}
for (const p of plan) console.log(`${onServer.has(p.thread.id) ? 'дополнить' : 'перенести'}  ${String(p.messages.length).padStart(4)} сообщ.  ${p.thread.title.slice(0, 70)}`);
if (!flag('--apply')) {
  console.log('\nЭто план. Перенести: добавьте --apply');
  process.exit(0);
}
let created = 0, added = 0;
// The node takes requests up to 2 MB: a long chat goes in parts (the import adds what is missing, so parts are safe).
const LIMIT = 1_500_000;
function parts(p) {
  const out = [];
  let cur = [], size = 0;
  for (const m of p.messages) {
    const s = Buffer.byteLength(JSON.stringify(m));
    if (cur.length && size + s > LIMIT) { out.push(cur); cur = []; size = 0; }
    cur.push(m);
    size += s;
  }
  out.push(cur);
  return out;
}
for (const p of plan) {
  for (const messages of parts(p)) {
    const r = await fetch(`${base}/api/import/threads`, { method: 'POST', headers: H, body: JSON.stringify({ threads: [{ thread: p.thread, messages }] }) });
    if (r.status === 404 || r.status === 405) throw new Error('на ноде нет импорта: сначала обновите ноду (deploy\\push.ps1)');
    if (!r.ok) throw new Error(`${p.thread.title}: ${r.status} ${await r.text()}`);
    const [x] = await r.json();
    created += x.created ? 1 : 0;
    added += x.messages_added;
  }
}
console.log(`\nГотово: чатов перенесено ${created}, сообщений добавлено ${added}.`);
