// A cron line as a person reads it. The common shapes get words; anything else is shown as it is.

const DOW_PLURAL = ['воскресеньям', 'понедельникам', 'вторникам', 'средам', 'четвергам', 'пятницам', 'субботам'];
const pad = (n: number) => String(n).padStart(2, '0');

function nums(field: string, min: number, max: number): number[] | null {
  const out: number[] = [];
  for (const part of field.split(',')) {
    const m = /^(\d+)(?:-(\d+))?$/.exec(part);
    if (!m) return null;
    const a = +m[1];
    const b = m[2] === undefined ? a : +m[2];
    if (a < min || b > max || a > b) return null;
    for (let i = a; i <= b; i++) out.push(i);
  }
  return out;
}

function joinRu(items: string[]): string {
  return items.length <= 1 ? (items[0] ?? '') : `${items.slice(0, -1).join(', ')} и ${items[items.length - 1]}`;
}

export function describeCron(expr: string): string {
  const f = expr.trim().split(/\s+/);
  if (f.length !== 5) return expr;
  const [mi, h, dom, mon, dow] = f;
  const step = /^\*\/(\d+)$/.exec(mi);
  if (step && h === '*' && dom === '*' && mon === '*' && dow === '*') return `каждые ${step[1]} мин`;
  if (/^\d+$/.test(mi) && h === '*' && dom === '*' && mon === '*' && dow === '*') return mi === '0' ? 'каждый час' : `каждый час в :${pad(+mi)}`;
  if (!/^\d+$/.test(mi) || !/^\d+$/.test(h) || mon !== '*') return expr;
  const at = `${pad(+h)}:${pad(+mi)}`;
  if (dom === '*' && dow === '*') return `каждый день в ${at}`;
  if (dom === '*') {
    const days = nums(dow, 0, 7)?.map((d) => d % 7);
    if (!days) return expr;
    const uniq = [...new Set(days)].sort((a, b) => ((a + 6) % 7) - ((b + 6) % 7));
    if (uniq.length === 5 && [1, 2, 3, 4, 5].every((d) => uniq.includes(d))) return `по будням в ${at}`;
    if (uniq.length === 2 && uniq.includes(0) && uniq.includes(6)) return `по выходным в ${at}`;
    return `по ${joinRu(uniq.map((d) => DOW_PLURAL[d]))} в ${at}`;
  }
  if (dow === '*') {
    const days = nums(dom, 1, 31);
    if (!days) return expr;
    return days.length === 1 ? `${days[0]}-го числа каждого месяца в ${at}` : `${joinRu(days.map(String))} числа в ${at}`;
  }
  return expr;
}

/** Ready-made schedules for the form. */
export const CRON_PRESETS: { cron: string; label: string }[] = [
  { cron: '0 9 * * *', label: 'Каждый день в 09:00' },
  { cron: '0 9 * * 1-5', label: 'По будням в 09:00' },
  { cron: '0 9 * * 1', label: 'По понедельникам в 09:00' },
  { cron: '0 9 * * 4', label: 'По четвергам в 09:00' },
  { cron: '0 9 * * 1,4', label: 'По понедельникам и четвергам в 09:00' },
  { cron: '0 9 1 * *', label: '1-го числа каждого месяца в 09:00' },
  { cron: '0 9 10,20,25 * *', label: '10, 20 и 25 числа в 09:00' },
  { cron: '0 * * * *', label: 'Каждый час' },
];
