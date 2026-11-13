// How hard a specialist thinks, in the terms of the CLI that runs them: each provider has its own control, and the
// node maps the same three steps onto it (claude --effort, codex model_reasoning_effort, kimi thinking).
type Option = { id: string; label: string; hint?: string };

export type Provider = 'claude' | 'codex' | 'kimi' | '';

export function providerOf(backend?: string | null, fallback = ''): Provider {
  const p = (backend || fallback || '').split(':')[0];
  return p === 'claude' || p === 'codex' || p === 'kimi' ? p : '';
}

const BY: Record<Exclude<Provider, ''>, { title: string; def: string; low: string; medium: string; high: string }> = {
  claude: { title: 'Claude · effort', def: 'Claude решает сам: глубже на трудном', low: 'effort low · быстро и дёшево', medium: 'effort medium', high: 'effort high · глубоко, дороже по лимиту' },
  codex: { title: 'GPT · reasoning effort', def: 'по умолчанию модели', low: 'reasoning low · быстро', medium: 'reasoning medium', high: 'reasoning high · дольше и дороже' },
  kimi: { title: 'Kimi · thinking', def: 'по умолчанию Kimi (max)', low: 'thinking low · быстро', medium: 'thinking high', high: 'thinking max · самый глубокий' },
};

export function effortTitle(p: Provider): string {
  return p ? BY[p].title : 'Тщательность';
}

export function effortOptions(p: Provider): Option[] {
  if (!p) {
    return [
      { id: '', label: 'Обычное', hint: 'Каждый специалист — как настроено у него' },
      { id: 'low', label: 'Быстро', hint: 'У каждого провайдера — его самый быстрый режим' },
      { id: 'medium', label: 'Средне' },
      { id: 'high', label: 'Тщательно', hint: 'Дольше думает, дороже по лимитам' },
    ];
  }
  const b = BY[p];
  return [
    { id: '', label: 'По умолчанию', hint: b.def },
    { id: 'low', label: 'Быстро', hint: b.low },
    { id: 'medium', label: 'Средне', hint: b.medium },
    { id: 'high', label: 'Глубоко', hint: b.high },
  ];
}
