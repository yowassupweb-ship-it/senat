// Registers MCP servers in a running Senate node and hands them to a specialist.
// The API token is read here, from the node's data dir, and is never printed.
//
//   node scripts/install-mcp.mjs --agent razrabotchik [--dry]     developer set
//   node scripts/install-mcp.mjs --agent dizayner [--dry]         designer set
//   node scripts/install-mcp.mjs --disable mbox-prod [--dry]      switch a server off for everyone (--enable turns it back on)
//   node scripts/install-mcp.mjs --remove mbox-prod [--dry]       delete a server from Senate altogether (with its secrets)
//   node scripts/install-mcp.mjs --agent seo-spetsialist --exclusive mbox-prod
//        the SEO specialist's set: MBOX's SEO tools, and MBOX for nobody else (--exclusive: "only this specialist")
//   env: SENATE_URL (default http://127.0.0.1:7790 = the desktop app), SENATE_TOKEN, SENATE_HOME
//
// Idempotent: a server that already exists is updated, not duplicated; people who already have it keep it.
// `existing: true` entries are only handed to the specialist (their command and secrets stay untouched).

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const args = process.argv.slice(2);
const DRY = args.includes('--dry');
const slug = args.includes('--agent') ? args[args.indexOf('--agent') + 1] : 'razrabotchik';
const SENATE = (process.env.SENATE_URL || 'http://127.0.0.1:7790').replace(/\/$/, '');
const dataDir = process.env.SENATE_HOME || path.join(process.env.APPDATA || path.join(os.homedir(), '.local/share'), 'Senate');
const token = process.env.SENATE_TOKEN || fs.readFileSync(path.join(dataDir, 'token'), 'utf8').trim();

// Pinned versions (checked on npm 2026-11-11). `trusted` = its tools run without asking in ask/edit modes:
// only for read-only servers.
const CONTEXT7 = { name: 'context7', command: 'npx', args: ['-y', '@upstash/context7-mcp@4.6.0'], trusted: true, note: 'Актуальная документация библиотек и фреймворков' };
const PLAYWRIGHT = { name: 'playwright', command: 'npx', args: ['-y', '@playwright/mcp@0.0.85'], trusted: false, note: 'Браузер: открыть страницу, кликнуть, снять скриншот, проверить вёрстку' };
const DEVTOOLS = { name: 'chrome-devtools', command: 'npx', args: ['-y', 'chrome-devtools-mcp@1.12.0'], trusted: false, note: 'DevTools: консоль, сеть, производительность, DOM страницы' };

// From the ux-ui-agent-skills plugin (.mcp.example.json). Needs the person's Figma token: the key is created
// empty, Senate shows it as "missing" in the MCP tab and the person pastes the value there (never through chat).
const FIGMA = { name: 'figma', command: 'npx', args: ['-y', 'figma-developer-mcp@0.13.2', '--stdio'], env: { FIGMA_API_KEY: '' }, trusted: false, note: 'Figma: макеты, компоненты, переменные и стили по ссылке на файл' };

// The SEO specialist works on a schedule, nobody at the screen: his SEO tools must not wait for a click.
const SEO_RULES = [{ kind: 'tool', value: 'mcp__mbox-prod__seo_*', label: 'Инструменты SEO Wizard (MBOX): seo_*' }];

const PRESETS = {
  'seo-spetsialist': [{ name: 'mbox-prod', existing: true, rules: SEO_RULES }],
  // developer: docs for libraries + a browser to check what was built
  razrabotchik: [CONTEXT7, PLAYWRIGHT, DEVTOOLS],
  // designer: a browser and DevTools to look at the rendered result, and Figma. MBOX is switched off: Senate replaces it.
  dizayner: [PLAYWRIGHT, DEVTOOLS, FIGMA],
};

const call = async (method, p, body) => {
  const r = await fetch(SENATE + p, { method, headers: { authorization: `Bearer ${token}`, 'content-type': 'application/json' }, body: body ? JSON.stringify(body) : undefined });
  const text = await r.text();
  let json;
  try { json = JSON.parse(text); } catch { json = text; }
  if (!r.ok) throw new Error(`${method} ${p}: ${r.status} ${typeof json === 'string' ? json.slice(0, 200) : JSON.stringify(json).slice(0, 200)}`);
  return json;
};

// --disable / --enable: switch servers on or off. Nothing is deleted: the server, its secrets and who has it stay.
const toggle = args.includes('--remove') ? 'remove' : args.includes('--disable') ? 'disable' : args.includes('--enable') ? 'enable' : null;
if (toggle === 'remove') {
  const names = args[args.indexOf('--remove') + 1]?.split(',').filter(Boolean) ?? [];
  if (!names.length) throw new Error('укажите имя сервера: --remove mbox-prod');
  const all = await call('GET', '/api/mcp');
  for (const n of names) {
    const cur = all.find((m) => m.name === n);
    if (!cur) { console.log(`нет такого сервера: ${n}`); continue; }
    if (DRY) { console.log(`[dry] удалить: ${n} (у него ${(cur.agents ?? []).length} специалистов)`); continue; }
    await call('DELETE', `/api/mcp/${cur.id}`);
    console.log(`удалён: ${n}`);
  }
  console.log('Применится со следующего запуска специалистов.');
  process.exit(0);
}
if (toggle) {
  const names = args[args.indexOf('--' + toggle) + 1]?.split(',').filter(Boolean) ?? [];
  if (!names.length) throw new Error('укажите имя сервера: --' + toggle + ' mbox-prod');
  const all = await call('GET', '/api/mcp');
  for (const n of names) {
    const cur = all.find((m) => m.name === n);
    if (!cur) { console.log(`нет такого сервера: ${n}`); continue; }
    if (cur.enabled === (toggle === 'enable')) { console.log(`уже ${toggle === 'enable' ? 'включён' : 'отключён'}: ${n}`); continue; }
    if (DRY) { console.log(`[dry] ${toggle === 'enable' ? 'включить' : 'отключить'}: ${n}`); continue; }
    await call('PATCH', `/api/mcp/${cur.id}`, { enabled: toggle === 'enable' });
    console.log(`${toggle === 'enable' ? 'включён' : 'отключён'}: ${n}`);
  }
  console.log('Применится со следующего запуска специалистов.');
  process.exit(0);
}

const SERVERS = PRESETS[slug];
if (!SERVERS) throw new Error(`для «${slug}» нет набора; есть: ${Object.keys(PRESETS).join(', ')}`);

const agents = await call('GET', '/api/agents');
const list = Array.isArray(agents) ? agents : agents.agents ?? [];
const agent = list.find((a) => a.slug === slug);
if (!agent) throw new Error(`специалист «${slug}» не найден; есть: ${list.map((a) => a.slug).join(', ')}`);

const exclusive = args.includes('--exclusive') ? args[args.indexOf('--exclusive') + 1] : null;
const existing = await call('GET', '/api/mcp');
for (const s of SERVERS) {
  const cur = existing.find((m) => m.name === s.name);
  if (s.existing && !cur) throw new Error(`сервер «${s.name}» должен уже быть в Senate, но его нет`);
  // keep the specialists who already have it, add this one
  const agentIds = exclusive === s.name ? [agent.id] : [...new Set([...(cur?.agents ?? []), agent.id])];
  const body = s.existing
    ? { agents: agentIds }
    : { kind: 'stdio', command: s.command, args: s.args, ...(s.env ? { env: s.env } : {}), trusted: s.trusted, enabled: true, note: s.note, agents: agentIds };
  const verb = s.existing ? (exclusive === s.name ? 'выдан только ему' : 'выдан') : cur ? 'обновлён' : 'создан';
  if (DRY) { console.log(`[dry] ${verb}: ${s.name} → ${slug}${s.rules ? ' + правила: ' + s.rules.map((r) => r.value).join(', ') : ''}`); continue; }
  const saved = cur ? await call('PATCH', `/api/mcp/${cur.id}`, body) : await call('POST', '/api/mcp', { name: s.name, ...body });
  console.log(`${verb}: ${s.name} → ${slug}${saved?.missing?.length ? ' (нужны секреты: ' + saved.missing.join(', ') + ')' : ''}`);
  for (const r of s.rules ?? []) {
    await call('POST', '/api/permission-rules', r);
    console.log(`  без вопросов: ${r.label}`);
  }
}
console.log('Готово. Серверы подключатся со следующего запуска специалиста.');
