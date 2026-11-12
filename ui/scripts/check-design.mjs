// The owner's standing design rules, checked on every build instead of remembered:
//   no native <select> (dropdowns are Dropdown.svelte), no emoji, no gradients, no blue.
// A line that must break a rule on purpose says so with a `design-ok: <why>` comment on it.
import { readFileSync, readdirSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';

const root = new URL('../src/', import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, '$1');
const files = [];
(function walk(d) {
  for (const n of readdirSync(d)) {
    const p = join(d, n);
    if (statSync(p).isDirectory()) walk(p);
    else if (/\.(svelte|ts|css)$/.test(n) && n !== 'theme.css') files.push(p);
  }
})(root);

const emoji = /[\u{1F300}-\u{1FAFF}\u{2600}-\u{27BF}\u{1F000}-\u{1F2FF}\u{1F900}-\u{1F9FF}\u{FE0F}]/u;
const hexes = /#([0-9a-f]{6}|[0-9a-f]{3})\b/gi;
function blue(hex) {
  let h = hex.slice(1);
  if (h.length === 3) h = [...h].map((c) => c + c).join('');
  const [r, g, b] = [0, 2, 4].map((i) => parseInt(h.slice(i, i + 2), 16) / 255);
  const max = Math.max(r, g, b), min = Math.min(r, g, b), l = (max + min) / 2, d = max - min;
  if (d < 0.08) return false; // grey
  const s = d / (1 - Math.abs(2 * l - 1));
  let hue = max === r ? ((g - b) / d) % 6 : max === g ? (b - r) / d + 2 : (r - g) / d + 4;
  hue = (hue * 60 + 360) % 360;
  return hue >= 195 && hue <= 250 && s > 0.45;
}

// The forum (the hall on the new-chat screen and its dialog) is left exactly as the owner approved it:
// it is not edited, even to annotate it.
const untouchable = new Set(['components/Chamber.svelte', 'components/ForumDialog.svelte']);

const problems = [];
for (const f of files) {
  if (untouchable.has(relative(root, f).replace(/\\/g, '/'))) continue;
  const lines = readFileSync(f, 'utf8').split(/\r?\n/);
  lines.forEach((line, i) => {
    if (line.includes('design-ok')) return;
    const at = `${relative(root, f)}:${i + 1}`;
    if (/<select[\s>]/.test(line)) problems.push(`${at}  нативный <select>: используйте Dropdown`);
    if (emoji.test(line)) problems.push(`${at}  эмодзи`);
    if (/(linear|radial|conic)-gradient\(/.test(line)) problems.push(`${at}  градиент`);
    for (const m of line.matchAll(hexes)) if (blue(m[0])) problems.push(`${at}  синий цвет ${m[0]}`);
  });
}

if (problems.length) {
  console.error(`Правила дизайна нарушены (${problems.length}):\n` + problems.map((p) => '  ' + p).join('\n'));
  process.exit(1);
}
const n = files.length, m10 = n % 10, m100 = n % 100;
const word = m10 === 1 && m100 !== 11 ? 'файл' : m10 >= 2 && m10 <= 4 && (m100 < 12 || m100 > 14) ? 'файла' : 'файлов';
console.log(`Правила дизайна: ${n} ${word}, нарушений нет`);
