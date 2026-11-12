// One-way import of the OWNER's MBOX data into a running Senate node. Idempotent: safe to run again.
//
//   MBOX_PASSWORD=...  node scripts/import-mbox.mjs [--dry]
//   env: MBOX_URL (required: the address of your MBOX), MBOX_USERNAME (default Admin),
//        SENATE_URL (default http://127.0.0.1:7790), SENATE_TOKEN (default: <data dir>/token)
//
// What moves (and why):
//   companies, projects (goal / constraints / stack / repo), project decisions, finished tasks (ready and
//   closed only; open ones stay in MBOX), project knowledge (facts and bugs), and the MCP servers from the owner's Claude configuration, handed to specialists by role.
// What does NOT move:
//   * agent-run logs and activity journals (thousands of "Итог запуска…" entries): noise, not knowledge;
//   * site logins/passwords: the owner locked them from agents in MBOX; Senate keeps that decision;
//   * integration keys: MBOX never returns them. Agents reach those APIs through the mbox-prod MCP instead,
//     so the keys stay in MBOX.
// Secret values are never printed.

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const DRY = process.argv.includes('--dry');
if (!process.env.MBOX_URL) throw new Error('MBOX_URL is not set');
const MBOX = process.env.MBOX_URL.replace(/\/$/, '');
const SENATE = (process.env.SENATE_URL || 'http://127.0.0.1:7790').replace(/\/$/, '');
const dataDir = process.env.SENATE_HOME || path.join(process.env.APPDATA || path.join(os.homedir(), '.local/share'), 'Senate');
const token = process.env.SENATE_TOKEN || fs.readFileSync(path.join(dataDir, 'token'), 'utf8').trim();
const password = process.env.MBOX_PASSWORD;
if (!password) throw new Error('MBOX_PASSWORD is not set');

// ---------- MBOX (read only) ----------
let cookie = '';
{
  const r = await fetch(`${MBOX}/api/mbox/auth/login`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ username: process.env.MBOX_USERNAME || 'Admin', password }) });
  if (!r.ok) throw new Error('MBOX login failed: ' + r.status);
  cookie = (r.headers.getSetCookie?.() ?? []).map((c) => c.split(';')[0]).join('; ');
}
const mget = async (p) => {
  const r = await fetch(MBOX + p, { headers: { cookie, 'x-mbox-agent': encodeURIComponent('Claude') } });
  if (!r.ok) throw new Error(`MBOX ${p}: ${r.status}`);
  return r.json();
};

// ---------- Senate ----------
async function s(method, p, body) {
  const r = await fetch(SENATE + p, { method, headers: { Authorization: `Bearer ${token}`, 'content-type': 'application/json' }, body: body === undefined ? undefined : JSON.stringify(body) });
  const text = await r.text();
  let json;
  try { json = JSON.parse(text); } catch { json = text; }
  if (!r.ok) { const e = new Error(`${method} ${p}: ${r.status} ${json?.error ?? text}`); e.status = r.status; throw e; }
  return json;
}
const clip = (t, n) => { const x = String(t ?? '').replace(/\s+\n/g, '\n').trim(); return x.length <= n ? x : x.slice(0, n - 1) + '…'; };
const log = (...a) => console.log(...a);

const agents = await s('GET', '/api/agents');
const byRole = (...roles) => agents.filter((a) => roles.includes(a.role)).map((a) => a.id);

// ---------- companies ----------
const mcompanies = (await mget('/api/mbox/companies')).companies;
const have = await s('GET', '/api/companies');
const companyId = new Map(); // mbox id -> senate id
let nCompanies = 0;
for (const c of mcompanies) {
  let hit = have.find((x) => x.name.toLowerCase() === c.name.toLowerCase());
  if (!hit && !DRY) {
    const p = c.props || {};
    const brief = clip([p.profile, p.domain ? `Сайт: ${p.domain}` : ''].filter(Boolean).join('\n\n'), 2000);
    hit = await s('POST', '/api/companies', { name: c.name, brief, color: /^#[0-9a-f]{6}$/i.test(c.color || '') ? c.color : '' });
    nCompanies++;
  }
  if (hit) companyId.set(String(c.id), hit.id);
}

// ---------- projects ----------
const KNOWN_FOLDERS = { 'MBOX': 'E:/Projects/Mbox/mbox', 'shar-messenger': 'E:/Projects/shar-2/shar', 'vs-works': 'E:/Projects/vs-works', 'Дегма': 'E:/Projects/Degma' };
const mprojects = (await mget('/api/mbox/projects')).projects;
const haveProjects = await s('GET', '/api/projects');
const projectId = new Map();
let nProjects = 0;
for (const p of mprojects) {
  let hit = haveProjects.find((x) => x.name.toLowerCase() === p.name.toLowerCase());
  if (!hit && !DRY) {
    const pr = p.props || {};
    const stack = Array.isArray(p.stack) ? p.stack.join(', ') : '';
    const goal = clip([pr['контекст'] || pr['роль'] || '', stack ? `Стек: ${stack}` : '', p.git_url ? `Репозиторий: ${p.git_url}` : ''].filter(Boolean).join('\n\n'), 2500);
    const constraints = clip([pr['агентам'], pr.principles, pr.philosophy].filter((x) => typeof x === 'string' && x).join('\n\n'), 2500);
    const folder = KNOWN_FOLDERS[p.name];
    hit = await s('POST', '/api/projects', {
      name: p.name, goal, constraints,
      company_id: p.company_id ? companyId.get(String(p.company_id)) ?? null : null,
      root_path: folder && fs.existsSync(folder) ? folder : '',
    });
    // Imported projects start careful: every change is confirmed until the owner relaxes it.
    await s('PATCH', `/api/projects/${hit.id}`, { mode: 'ask', status: p.status === 'paused' ? 'paused' : 'active' });
    nProjects++;
  }
  if (hit) projectId.set(String(p.id), hit.id);
}

// ---------- decisions ----------
const mdecisions = (await mget('/api/mbox/decisions')).decisions;
let nDecisions = 0;
for (const pid of new Set(mdecisions.map((d) => String(d.project_id)))) {
  const sid = projectId.get(pid);
  if (!sid || DRY) continue;
  const existing = new Set((await s('GET', `/api/projects/${sid}`)).decisions.map((d) => d.title));
  for (const d of mdecisions.filter((x) => String(x.project_id) === pid)) {
    const title = clip(d.title, 200);
    if (!title || existing.has(title)) continue;
    const body = clip([d.decision, d.rationale ? `Почему: ${d.rationale}` : '', d.impact ? `Влияние: ${d.impact}` : ''].filter(Boolean).join('\n'), 1500);
    const made = await s('POST', `/api/projects/${sid}/decisions`, { title, body });
    await s('POST', `/api/decisions/${made.id}/status`, { status: 'accepted' });
    existing.add(title);
    nDecisions++;
  }
}

// ---------- tasks: only the finished ones ----------
// Ready (waiting for the owner's check) and closed tasks come over as history; open, next and in-progress
// ones stay in MBOX. Imported tasks have no owner, so nothing is scheduled and nothing starts running.
let nTasks = 0;
for (const p of mprojects) {
  const sid = projectId.get(String(p.id));
  if (!sid || DRY) continue;
  const ctx = await mget(`/api/mbox/agent/context?project=${encodeURIComponent(p.name)}&detail=full`);
  const finished = (ctx.todos || []).filter((t) => ['review', 'done', 'archived'].includes(t.status));
  if (!finished.length) continue;
  const known = new Set((await s('GET', `/api/obligations?all=1&project=${sid}`)).map((o) => o.title));
  for (const t of finished) {
    const title = clip(t.title, 200);
    if (!title || known.has(title)) continue;
    const note = clip(t.note, 1200);
    const made = await s('POST', '/api/obligations', { title, result: clip(t.note, 600), project_id: sid, requested_by: 'mbox' });
    if (t.status === 'review') {
      await s('POST', `/api/obligations/${made.id}/transition`, { to: 'running', note: 'перенос из MBOX' });
      await s('POST', `/api/obligations/${made.id}/transition`, { to: 'review', outcome: note || 'Готово в MBOX, ждёт проверки', note: 'перенос из MBOX' });
    } else {
      await s('POST', `/api/obligations/${made.id}/transition`, { to: 'done', outcome: note || 'Закрыта в MBOX', note: 'перенос из MBOX' });
    }
    known.add(title);
    nTasks++;
  }
}

// ---------- knowledge ----------
// MBOX's memory list is capped at 300 per request and has no paging, but it searches. So knowledge is
// gathered with a spread of searches (plus the newest and oldest pages) and de-duplicated by id.
const SEARCHES = ['', 'sort=oldest', ...['факт', 'правило', 'решение', 'архитектура', 'сведения', 'компания', 'проект', 'клиент', 'деплой', 'сервер', 'настройка',
  'схема', 'источник', 'данные', 'процесс', 'договор', 'сайт', 'бот', 'доступ', 'формат', 'стек', 'релиз', 'интеграция', 'API', 'баг', 'bug-report',
  'ошибка', 'требования', 'контакт', 'тариф', 'цена', 'тур', 'дизайн', 'макет', 'запуск', 'старт', 'концепция', 'риски', 'политика',
  ...mprojects.map((p) => p.name)].map((q) => 'q=' + encodeURIComponent(q))];
const uniq = new Map();
for (const qs of SEARCHES) {
  try {
    const d = await mget('/api/mbox/memories' + (qs ? '?' + qs : ''));
    for (const m of d.memories || []) uniq.set(String(m.id), m);
  } catch { /* a failed search only narrows the net */ }
}
const isRunLog = (m) => /^Итог запуска/i.test(m.title || '') || (m.tags || []).some((t) => ['agent-work', 'auto', 'finish-task'].includes(t));
const keep = [...uniq.values()].filter((m) => (m.entity_type === 'fact' || m.entity_type === 'bug') && !isRunLog(m));
log(`MBOX memories reached: ${uniq.size}; carrying over ${keep.length} facts and bugs; leaving ${uniq.size - keep.length} logs, run summaries, posts and activity entries`);
let nMemory = 0;
const knownSources = new Set();
for (const sid of projectId.values()) {
  for (const m of await s('GET', `/api/memory?level=project&project=${sid}`)) knownSources.add(m.source);
}
for (const m of keep) {
  const sid = projectId.get(String(m.project_id));
  const source = `mbox:memory:${m.id}`;
  if (!sid || knownSources.has(source) || DRY) continue;
  const prefix = m.entity_type === 'bug' ? 'Баг: ' : '';
  const body = clip(`${prefix}${m.title}\n${m.content || ''}`, 1100);
  if (body.length < 12) continue;
  await s('POST', '/api/memory', { level: 'project', project_id: sid, kind: 'observation', body, source, scope_note: (m.tags || []).filter((t) => !t.startsWith('severity') ).slice(0, 4).join(', ') });
  nMemory++;
}

// ---------- MCP servers from the owner's Claude configuration ----------
const cfg = JSON.parse(fs.readFileSync(path.join(os.homedir(), '.claude.json'), 'utf8'));
const servers = { ...(cfg.mcpServers || {}) };
for (const [proj, pd] of Object.entries(cfg.projects || {})) {
  if (/shar-2/i.test(proj)) for (const [k, v] of Object.entries(pd.mcpServers || {})) servers[k] = v;
}
// who gets what (by role): tools follow the craft
const GIVE = {
  // 'mbox-prod' is deliberately not here: Senate has its own tasks (obligations), MBOX is no longer plugged into it.
  obscura: byRole('developer', 'designer', 'analyst'),
  powerpoint: byRole('designer'),
  marp: byRole('designer'),
  'google-slides': byRole('designer'),
  markitdown: byRole('assistant', 'analyst'),
  firecrawl: byRole('analyst', 'assistant'),
  shar: byRole('developer'),
};
const haveMcp = new Set((await s('GET', '/api/mcp')).map((m) => m.name));
let nMcp = 0;
for (const [name, v] of Object.entries(servers)) {
  if (name === 'apple-docs' || !GIVE[name] || haveMcp.has(name) || DRY) continue;
  const http = (v.type || 'stdio') === 'http' || !!v.url;
  const env = { ...(v.env || {}) };
  const cmdLine = [v.command, ...(v.args || [])];
  // "cmd /c npx …" wrappers are Windows-only noise: the node runs the real command itself.
  const [command, ...args] = cmdLine[0]?.toLowerCase() === 'cmd' && cmdLine[1] === '/c' ? cmdLine.slice(2) : cmdLine;
  await s('POST', '/api/mcp', http
    ? { name, kind: 'http', url: v.url, headers: v.headers || {}, note: 'Перенесено из конфигурации Claude', agents: GIVE[name] }
    : { name, kind: 'stdio', command, args, env, note: 'Перенесено из конфигурации Claude', agents: GIVE[name] });
  nMcp++;
}

log(DRY ? 'DRY RUN — nothing written' : 'Imported:');
log(`  companies ${nCompanies}, projects ${nProjects}, decisions ${nDecisions}, finished tasks ${nTasks}, knowledge ${nMemory}, MCP servers ${nMcp}`);
