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

// A local address written without a scheme (`localhost:3006`, `127.0.0.1:5173/app`) is still a link to the
// person's dev server: agents write it that way all the time and a plain text address cannot be clicked.
const LOCAL_ADDR = /(?<![\w./@:-])((?:localhost|127\.0\.0\.1|0\.0\.0\.0|\[::1\])(?::\d{2,5})(?:\/[^\s<>"')\]]*)?)/gi;
const LOCAL_WHOLE = /^(?:https?:\/\/)?(?:localhost|127\.0\.0\.1|0\.0\.0\.0|\[::1\])(?::\d{2,5})?(?:\/\S*)?$/i;

function localHref(addr: string): string {
  const a = addr.replace(/[.,;:!?]+$/, '');
  return (/^https?:\/\//i.test(a) ? a : `http://${a}`).replace('//0.0.0.0', '//localhost');
}

function linkLocal(html: string): string {
  if (!/localhost|127\.0\.0\.1|0\.0\.0\.0|\[::1\]/i.test(html)) return html;
  const doc = new DOMParser().parseFromString(`<body>${html}</body>`, 'text/html');
  const walker = doc.createTreeWalker(doc.body, NodeFilter.SHOW_TEXT);
  const texts: Text[] = [];
  for (let n = walker.nextNode(); n; n = walker.nextNode()) texts.push(n as Text);
  const mk = (href: string, label: string) => {
    const a = doc.createElement('a');
    a.href = href;
    a.target = '_blank';
    a.rel = 'noreferrer noopener';
    a.textContent = label;
    return a;
  };
  for (const t of texts) {
    const parent = t.parentElement;
    if (!parent || parent.closest('a, pre')) continue;
    const text = t.data;
    // inline code that is only an address: the whole chip becomes the link
    if (parent.tagName === 'CODE') {
      const whole = text.trim();
      if (LOCAL_WHOLE.test(whole)) {
        const a = mk(localHref(whole), '');
        parent.replaceWith(a);
        a.appendChild(parent);
      }
      continue;
    }
    LOCAL_ADDR.lastIndex = 0;
    if (!LOCAL_ADDR.test(text)) continue;
    LOCAL_ADDR.lastIndex = 0;
    const frag = doc.createDocumentFragment();
    let last = 0;
    for (const m of text.matchAll(LOCAL_ADDR)) {
      const start = m.index ?? 0;
      const trimmed = m[1].replace(/[.,;:!?]+$/, '');
      frag.append(text.slice(last, start));
      frag.append(mk(localHref(trimmed), trimmed));
      last = start + trimmed.length;
    }
    frag.append(text.slice(last));
    t.replaceWith(frag);
  }
  return doc.body.innerHTML;
}

/** Markdown → sanitized HTML. Agent output is untrusted: always sanitized. */
export function md(text: string): string {
  const html = marked.parse(text || '', { async: false }) as string;
  return DOMPurify.sanitize(linkLocal(DOMPurify.sanitize(html, { ADD_ATTR: ['target'] })), { ADD_ATTR: ['target'] });
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
  auto: { label: 'Под ключ', hint: 'Работает сам и ничего не спрашивает. Остановится только на том, что уничтожает систему (форматирование, выключение).' },
};

/** Russian plural: plural(3, ['проект', 'проекта', 'проектов']) → 'проекта'. */
export function plural(n: number, forms: [string, string, string]): string {
  const a = Math.abs(n) % 100;
  const b = a % 10;
  if (a > 10 && a < 20) return forms[2];
  if (b === 1) return forms[0];
  if (b >= 2 && b <= 4) return forms[1];
  return forms[2];
}
/** A number with its noun: count(3, ['проект', 'проекта', 'проектов']) → '3 проекта'. */
export const count = (n: number | undefined | null, forms: [string, string, string]) => `${n ?? 0} ${plural(n ?? 0, forms)}`;
export const N = {
  task: ['задача', 'задачи', 'задач'] as [string, string, string],
  project: ['проект', 'проекта', 'проектов'] as [string, string, string],
  model: ['модель', 'модели', 'моделей'] as [string, string, string],
  fact: ['знание', 'знания', 'знаний'] as [string, string, string],
  file: ['файл', 'файла', 'файлов'] as [string, string, string],
  step: ['шаг', 'шага', 'шагов'] as [string, string, string],
  error: ['ошибка', 'ошибки', 'ошибок'] as [string, string, string],
};
/**
 * Parts of a line joined by " · ", empty ones skipped. Use it instead of `{#if x} · {x}{/if}`: Svelte trims the
 * space at the start of a block, and the dot ends up glued to the word before it.
 */
export const dots = (...parts: (string | number | false | null | undefined)[]) =>
  parts.filter((p) => p !== false && p !== null && p !== undefined && String(p).trim() !== '').join(' · ');

/** "7:13 PM" / "19:13" in a provider's message → "19:13". */
function timeIn(text: string): string {
  const m = /(d{1,2}):(d{2})s*(am|pm)?/i.exec(text);
  if (!m) return '';
  let h = +m[1];
  const ap = (m[3] || '').toLowerCase();
  if (ap === 'pm' && h < 12) h += 12;
  if (ap === 'am' && h === 12) h = 0;
  return `${String(h).padStart(2, '0')}:${m[2]}`;
}

/**
 * A failed answer, said so a person knows what happened and what to do. The provider's own words are kept for the
 * "details" fold: they are what support needs, but not what the person should have to read first.
 */
export function explainFailure(raw?: string | null): { title: string; hint: string } {
  const t = (raw || '').trim();
  const l = t.toLowerCase();
  const has = (...w: string[]) => w.some((x) => l.includes(x));
  if (has('usage limit', 'rate limit', 'too many requests', 'quota', 'limit reached', 'out of credits', 'purchase more credits')) {
    const at = timeIn(t);
    return { title: 'Исчерпан лимит подписки', hint: at ? `Модель снова будет доступна после ${at}. Можно повторить позже или переключить специалиста на другую модель.` : 'Подождите, пока лимит обновится, или переключите специалиста на другую модель.' };
  }
  if (has('unknown sessionid', 'session not found', 'no conversation found')) {
    return { title: 'Сессия специалиста устарела', hint: 'Нажмите «Повторить»: он начнёт новую сессию и заново прочитает этот чат.' };
  }
  if (has('not logged in', 'unauthorized', 'invalid api key', 'authentication', 'please log in', 'login required', '401')) {
    return { title: 'Специалист не вошёл в аккаунт модели', hint: 'Войдите заново в «Настройки → Агенты», затем повторите.' };
  }
  if (has('prompt is too long', 'context length', 'maximum context', 'context window', 'too many tokens')) {
    return { title: 'Разговор стал слишком длинным', hint: 'Сожмите его командой /compact или начните новый чат: важное специалист возьмёт из памяти.' };
  }
  if (has('overloaded', '529', '503', 'service unavailable', 'server error', '500 internal')) {
    return { title: 'Сервис модели перегружен', hint: 'Это на стороне провайдера и обычно проходит за минуту. Повторите чуть позже.' };
  }
  if (has('timed out', 'timeout', 'deadline')) {
    return { title: 'Не дождались ответа', hint: 'Модель отвечала слишком долго. Повторите: обычно со второго раза получается.' };
  }
  if (has('econnreset', 'econnrefused', 'network', 'fetch failed', 'socket', 'dns', 'getaddrinfo')) {
    return { title: 'Нет связи с сервисом модели', hint: 'Похоже на сбой сети. Повторите, когда связь вернётся.' };
  }
  if (has('не найден на этой машине', 'not found on this machine', 'is not recognized', 'enoent')) {
    return { title: 'Программа специалиста не установлена', hint: t.length < 200 ? t : 'Установите её на компьютере, где он работает, и войдите в Настройках.' };
  }
  if (has('рабочая папка недоступна')) {
    return { title: 'Рабочая папка недоступна', hint: 'Проверьте, что папка проекта существует, или выберите другую в настройках проекта.' };
  }
  return { title: 'Ответ не получен', hint: t ? '' : 'Причина неизвестна. Повторите — если не поможет, загляните в подробности.' };
}
