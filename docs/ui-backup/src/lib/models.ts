import type { Account } from './types';

export const PROVIDER_NAME: Record<string, string> = { claude: 'Claude', codex: 'GPT', kimi: 'Kimi', mock: 'Демо' };

export interface ModelOption {
  id: string; // "claude:opus" | "codex" | ""
  label: string;
  group: string;
  brand: string;
  disabled?: boolean;
  hint?: string;
}

/**
 * Every model the user can pick, grouped by agent. Lists come from the providers themselves (see the node's
 * account refresh), so they stay current; a provider that is not logged in is shown disabled with the reason.
 */
export function modelOptions(accounts: Account[], opts: { withDefault?: boolean; defaultLabel?: string; includeDemo?: boolean } = {}): ModelOption[] {
  const out: ModelOption[] = [];
  if (opts.withDefault) out.push({ id: '', label: opts.defaultLabel ?? 'Решает среда', group: '', brand: '' });
  for (const a of accounts) {
    if (!a.installed) continue;
    const group = PROVIDER_NAME[a.provider] ?? a.label;
    if (a.state !== 'ready') {
      out.push({ id: `~${a.provider}`, label: 'Нужен вход — Настройки → Агенты', group, brand: a.provider, disabled: true });
      continue;
    }
    out.push({ id: a.provider, label: 'По умолчанию у провайдера', group, brand: a.provider });
    for (const m of a.models) out.push({ id: `${a.provider}:${m.id}`, label: m.label, group, brand: a.provider });
  }
  if (opts.includeDemo) out.push({ id: 'mock', label: 'Демо (без модели)', group: 'Демо', brand: 'mock' });
  return out;
}

/** "claude:claude-opus-5-5" → "Claude · Opus 5.5" using the account's own model labels. */
export function modelTitle(accounts: Account[], backend: string): string {
  if (!backend) return 'Решает среда';
  const [provider, ...rest] = backend.split(':');
  const model = rest.join(':');
  const acc = accounts.find((a) => a.provider === provider);
  const name = PROVIDER_NAME[provider] ?? provider;
  if (!model) return name;
  return `${name} · ${acc?.models.find((m) => m.id === model)?.label ?? model}`;
}
