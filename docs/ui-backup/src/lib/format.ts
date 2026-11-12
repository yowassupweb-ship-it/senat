import { marked } from 'marked';
import DOMPurify from 'dompurify';

export function timeAgo(ms: number): string {
  const s = Math.max(0, Math.round((Date.now() - ms) / 1000));
  if (s < 45) return 'сейчас';
  if (s < 3600) return `${Math.round(s / 60)} мин`;
  if (s < 86400) return `${Math.round(s / 3600)} ч`;
  if (s < 86400 * 7) return `${Math.round(s / 86400)} дн`;
  return new Date(ms).toLocaleDateString('ru-RU', { day: 'numeric', month: 'short' });
}

export function clock(ms: number): string {
  return new Date(ms).toLocaleTimeString('ru-RU', { hour: '2-digit', minute: '2-digit' });
}

export function dateTime(ms: number): string {
  return new Date(ms).toLocaleString('ru-RU', { day: 'numeric', month: 'short', hour: '2-digit', minute: '2-digit' });
}

export function dur(ms?: number): string {
  const s = Math.round((ms || 0) / 1000);
  if (!s) return '';
  return s >= 60 ? `${Math.floor(s / 60)} мин ${s % 60} с` : `${s} с`;
}

export function tokens(n?: number): string {
  if (!n) return '0';
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1).replace('.', ',')} млн`;
  if (n >= 1000) return `${(n / 1000).toFixed(n >= 10_000 ? 0 : 1).replace('.', ',')} тыс.`;
  return String(n);
}

export function baseName(p: string): string {
  return p.replace(/[\\/]+$/, '').split(/[\\/]/).pop() || p;
}

/** "claude-sonnet-5-5" → "Sonnet 5.5". The user never needs an API identifier. */
export function modelLabel(model?: string): string {
  if (!model) return '';
  if (model === 'demo') return 'Демо';
  const words = model.replace(/^claude-/i, '').replace(/-\d{8}$/, '').split('-');
  const out: string[] = [];
  for (const w of words) {
    if (/^\d+$/.test(w) && /^\d+$/.test(out[out.length - 1] || '')) out[out.length - 1] += `.${w}`;
    else out.push(/^\d/.test(w) ? w : w.charAt(0).toUpperCase() + w.slice(1));
  }
  return out.join(' ');
}

marked.setOptions({ gfm: true, breaks: true });

const renderer = new marked.Renderer();
renderer.link = ({ href, text }) => `<a href="${href}" target="_blank" rel="noreferrer noopener">${text}</a>`;
marked.use({ renderer });

/** Markdown → sanitized HTML. Agent output is untrusted: always sanitized. */
export function md(text: string): string {
  const html = marked.parse(text || '', { async: false }) as string;
  return DOMPurify.sanitize(html, { ADD_ATTR: ['target'] });
}

export const ROLE_TITLE: Record<string, string> = {
  architect: 'Архитектор',
  developer: 'Разработчик',
  designer: 'Дизайнер',
  analyst: 'Аналитик',
  business: 'Бизнес',
  assistant: 'Ассистент',
  custom: 'Специалист',
};

/** Role under a name — only when it says something the name does not ("Дизайнер / Дизайнер" is noise). */
export function roleLabel(a?: { name: string; role: string } | null): string {
  if (!a) return '';
  const r = ROLE_TITLE[a.role] ?? a.role;
  return r.trim().toLowerCase() === a.name.trim().toLowerCase() || a.role === 'custom' ? '' : r;
}

export const STATE_LABEL: Record<string, string> = {
  accepted: 'Принята',
  running: 'Выполняется',
  waiting: 'Ожидает',
  blocked: 'Заблокирована',
  review: 'На проверке',
  done: 'Завершена',
  cancelled: 'Отменена',
};

export const MODE_LABEL: Record<string, { label: string; hint: string }> = {
  plan: { label: 'План', hint: 'Только изучает и предлагает план. Файлы не меняет.' },
  ask: { label: 'Спрашивать', hint: 'Каждое изменение — с вашего разрешения.' },
  edit: { label: 'Правки', hint: 'Правки в папке проекта — сразу; команды — с разрешения.' },
  auto: { label: 'Под ключ', hint: 'Работает сам. Опасное (удаление, push) — всё равно спросит.' },
};
