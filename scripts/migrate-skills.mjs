// Moves two skills from MBOX into Senate as skills of specialists:
//   seo-wizard      → «SEO специалист»          (with the schedule: Monday, Thursday, the 10th, 20th and 25th)
//   email-campaign  → «Специалист по рассылкам» (the letter kit: components, scripts, rules — no visual editor pages)
//
//   node scripts/migrate-skills.mjs [--dry] [--force] [--mbox E:/Projects/Mbox/mbox]
//   env: SENATE_HOME (default %APPDATA%/Senate)
//
// It only copies files (nothing in MBOX is changed, no token is needed). Skills land in
// <Senate data dir>/skills/<specialist slug>/<skill>/ ; the node reads them at once. An existing skill is not
// overwritten without --force, so edits made in Senate are safe.

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const args = process.argv.slice(2);
const DRY = args.includes('--dry');
const FORCE = args.includes('--force');
const MBOX = args.includes('--mbox') ? args[args.indexOf('--mbox') + 1] : 'E:/Projects/Mbox/mbox';
const dataDir = process.env.SENATE_HOME || path.join(process.env.APPDATA || path.join(os.homedir(), '.local/share'), 'Senate');
const out = (slug, skill) => path.join(dataDir, 'skills', slug, skill);

const SEO_DISCIPLINE = (file, what) =>
  `Выполни сценарий «${what}» из навыка seo-wizard: прочитай SKILL.md и ${file} и следуй им. Данные бери инструментами seo_* (MCP mbox-prod): сначала пакет и история, свежесть источников, потом находки. ` +
  `Задачи людям — только с цифрами, через senate_obligation_create (исполнитель — slug коллеги, например разработчика или копирайтера); решения человеку — вопросом в итоге. ` +
  `Закончи отчётом сессии (seo_record_session_report) и обнови эту задачу: done с описанием, что сделано, найдено и что ждёт решения человека.`;

const SEO_ROUTINES = [
  { name: 'SEO Wizard: понедельник', cron: '0 9 * * 1', title: 'SEO Wizard · понедельник', prompt: SEO_DISCIPLINE('monday.md', 'понедельник') },
  { name: 'SEO Wizard: четверг', cron: '0 9 * * 4', title: 'SEO Wizard · четверг', prompt: SEO_DISCIPLINE('thursday.md', 'четверг') },
  { name: 'SEO Wizard: 10 число — архитектура', cron: '0 9 10 * *', title: 'SEO Wizard · архитектура сайта', prompt: SEO_DISCIPLINE('architecture.md', '10 число: архитектура') },
  { name: 'SEO Wizard: 20 число — авторитет', cron: '0 9 20 * *', title: 'SEO Wizard · авторитет и ссылки', prompt: SEO_DISCIPLINE('authority.md', '20 число: авторитет') },
  { name: 'SEO Wizard: 25 число — итоги месяца', cron: '0 9 25 * *', title: 'SEO Wizard · итоги месяца', prompt: SEO_DISCIPLINE('monthly.md', '25 число: итоги месяца') },
];

const SEO_NOTE = `## Как это работает в Senate

Ты — специалист по SEO, и это твоя основная работа. Сценарии ниже запускаются **сами по расписанию** (понедельник и четверг в 09:00, 10-го, 20-го и 25-го числа): для тебя появляется задача с названием сценария. Расписание и включение видны и меняются у человека в «Задачи → Постоянные».

- **Данные.** Инструменты \`seo_*\` (\`seo_read_package\`, \`seo_read_history\`, \`seo_page_card\`, \`seo_run_collection\`, \`seo_check_topvisor\`, \`seo_create_task_from_issue\`, \`seo_update_issue\`, \`seo_record_session_report\` …) приходят из MCP-сервера MBOX (\`mbox-prod\`), к которому у тебя есть доступ. Сервер сам собирает данные из Topvisor, Wordstat, Яндекс Вебмастера и Метрики: ключи хранятся на нём, тебе они не нужны. В инструментах они называются \`mcp__mbox-prod__seo_…\`.
- **Задачи людям и коллегам.** Всё, что требует работы разработчика, копирайтера или других, ставь в Senate: \`senate_obligation_create\` с исполнителем (slug из \`senate_colleagues\`), с цифрами и критерием проверки. Параллельно создавай задачу в MBOX только через \`seo_create_task_from_issue\`, как требует сценарий.
- **Решения человека** (301, canonical, noindex, объединение страниц, новая страница, правила фильтров) — вопросом в итоговом отчёте задачи, не действием.
- **Экран MBOX.** Инструмента \`open_tab\` нет: человеку показывай результат отчётом.
- **Итог каждой задачи-сценария**: \`senate_obligation_update\` → \`done\` с описанием (что сделано, какие находки и цифры, что ждёт решения); незавершённое — \`waiting\` или \`blocked\` с причиной.
- **Знания.** Что подтвердилось и что нет — записывай через \`senate_remember\` (проверенное — с основанием), чтобы в следующий раз не проверять заново.
- Если MBOX недоступен или источник устарел — скажи об этом в отчёте и не придумывай данные.
`;

const MAIL_NOTE = `## Как это работает в Senate

Это твой основной навык: письма «Вокруг света» собираются из компонентов, а не рисуются заново. Визуального редактора (library.html, brief-builder.html) здесь нет и он не нужен: бриф — разговор в чате, письмо — файл \`letters/<id>.json\`, сборка — скрипт.

- **Папка навыка** — та, что указана в списке навыков. Скрипты запускай **из неё**: \`node scripts/letter.mjs …\` всегда с флагом \`--local\` (письма лежат в \`letters/\` этой папки, сервер MBOX не нужен).
- **Готовые письма** клади в рабочую папку проекта: перед запуском задай \`MAIL_READY_DIR\` (например, \`<папка проекта>/mail/ready\`) или передай \`--out\`.
- **MBOX-инструменты** \`open_tab\`, \`edit_skill_file\`, \`write_skill_files\` недоступны: править \`letters/<id>.json\` и компоненты нужно обычными инструментами Edit/Write прямо в папке навыка; сборка и проверка — \`node scripts/letter.mjs build <id> --local\`, затем покажи человеку путь к готовому HTML и номера использованных компонентов.
- **Контекст «Вокруг света»** бери из памяти Senate (\`senate_recall\`) и из редакционного контракта (\`references/editorial-policy.md\`). Правила, которые подтвердил человек, записывай через \`senate_remember\`.
- Рассылку **не отправляй** без отдельного разрешения человека; обязательная проверка — Email Checker (\`build\`/\`preflight\`).
`;

function withNote(text, note, { dropInvocation = false } = {}) {
  const m = /^(---\r?\n[\s\S]*?\r?\n---\r?\n)([\s\S]*)$/.exec(text.replace(/^\uFEFF/, ''));
  if (!m) return `${note}\n${text}`;
  let front = m[1];
  if (dropInvocation) front = front.replace(/^invocation:.*\r?\n/m, '');
  return `${front}\n${note}\n${m[2]}`;
}

function copyDir(from, to, keep = () => true) {
  fs.cpSync(from, to, { recursive: true, filter: (src) => keep(path.relative(from, src).replace(/\\/g, '/')) });
}

const SKILLS = [
  {
    slug: 'seo-spetsialist',
    name: 'seo-wizard',
    from: path.join(MBOX, 'skills', 'seo-wizard'),
    note: SEO_NOTE,
    dropInvocation: true,
    keep: (rel) => rel === '' || rel.endsWith('.md'),
    extra: (dest) => fs.writeFileSync(path.join(dest, 'routines.json'), JSON.stringify(SEO_ROUTINES, null, 2) + '\n'),
  },
  {
    slug: 'spetsialist-po-rassylkam',
    name: 'email-campaign',
    from: path.join(MBOX, 'skills', 'email-campaign'),
    note: MAIL_NOTE,
    // the editor pages and a second visual sample stay behind; the kit (components, scripts, rules) goes
    keep: (rel) => !['library.html', 'brief-builder.html'].includes(rel) && !/^письмо.*\.html$/i.test(rel),
  },
];

let failed = false;
for (const s of SKILLS) {
  const dest = out(s.slug, s.name);
  if (!fs.existsSync(path.join(s.from, 'SKILL.md'))) {
    console.log(`✗ нет исходника: ${s.from}`);
    failed = true;
    continue;
  }
  if (fs.existsSync(dest) && !FORCE) {
    console.log(`— уже есть, не трогаю: ${dest} (--force перезапишет)`);
    continue;
  }
  const files = [];
  (function walk(d) {
    for (const e of fs.readdirSync(d, { withFileTypes: true })) {
      const p = path.join(d, e.name);
      const rel = path.relative(s.from, p).replace(/\\/g, '/');
      if (!s.keep(rel)) continue;
      e.isDirectory() ? walk(p) : files.push(rel);
    }
  })(s.from);
  console.log(`${DRY ? '[dry] ' : ''}✓ ${s.name} → ${s.slug}: ${files.length} файлов → ${dest}`);
  if (DRY) continue;
  fs.rmSync(dest, { recursive: true, force: true });
  fs.mkdirSync(dest, { recursive: true });
  copyDir(s.from, dest, s.keep);
  const skillMd = path.join(dest, 'SKILL.md');
  fs.writeFileSync(skillMd, withNote(fs.readFileSync(skillMd, 'utf8'), s.note, { dropInvocation: s.dropInvocation }));
  s.extra?.(dest);
}
console.log(failed ? 'Есть ошибки.' : DRY ? 'Это пробный прогон: ничего не записано.' : 'Готово. Навыки видны специалистам со следующего запуска; расписание SEO появится в «Задачи → Постоянные» в течение минуты.');
process.exit(failed ? 1 : 0);
