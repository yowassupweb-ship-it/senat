import { test, expect, type Page } from '@playwright/test';

const at = Date.now();
const agents = [
  { id: 'a1', slug: 'analyst', name: 'Аналитик', role: 'analyst', gender: 'm', model_pref: 'mock' },
  { id: 'a2', slug: 'editor', name: 'Редактор', role: 'custom', gender: 'f', model_pref: 'mock' },
];
const thread = { id: 't1', kind: 'chat', title: 'Проверка очереди', model: 'mock', mode: 'edit', effort: '', updated_at: at };
async function setup(page: Page, send: (body: any) => Promise<any>, history?: (url: URL) => any) {
  await page.addInitScript(() => {
    localStorage.setItem('senate.token', 'test-only');
    (window as any).EventSource = class { onopen: any; constructor() { setTimeout(() => this.onopen?.(), 0); } close() {} };
  });
  await page.route('**/api/**', async route => {
    const url = new URL(route.request().url());
    const path = url.pathname;
    let data: any = [];
    if (path === '/api/state') data = { agents, projects: [], companies: [], accounts: [], threads: [thread], backends: [], default_backend: 'mock', node_id: 'outbox-test', attention: { approvals: [], needs_user: [], running: [], overdue: [], queued_sessions: 0, proposed_decisions: [] } };
    if (path === '/api/threads') data = route.request().method() === 'POST' ? thread : [thread];
    if (path.endsWith('/messages')) {
      if (route.request().method() === 'POST') {
        const result = await send(route.request().postDataJSON());
        if (result === false) return route.abort('failed');
        return route.fulfill({ json: result });
      }
      data = history?.(url) ?? { messages: agents.map((a, i) => ({ id: `m${i}`, thread_id: 't1', author_id: a.id, author_kind: 'agent', body: `Ответ ${a.name}`, status: 'done', props: {}, created_at: at + i, updated_at: at + i })), has_more: false };
    }
    await route.fulfill({ json: data });
  });
  await page.goto('/#/chat/t1');
  await expect(page.getByRole('textbox', { name: 'Сообщение' })).toBeVisible();
}
function receipt(body: any) {
  return { user_message: { id: body.client_id, thread_id: 't1', author_id: null, author_kind: 'user', body: body.text, status: 'done', props: { client_id: body.client_id }, created_at: Date.now(), updated_at: Date.now() }, agent_messages: [] };
}
async function typeAndSend(page: Page, text: string) {
  await page.getByRole('textbox', { name: 'Сообщение' }).fill(text);
  await page.getByRole('button', { name: 'Отправить', exact: true }).click();
}

test('slow send frees the composer, queues in order and lets an unsent message be removed', async ({ page }) => {
  const sent: string[] = [];
  let release!: () => void;
  const gate = new Promise<void>(r => release = r);
  await setup(page, async body => {
    sent.push(body.text);
    if (sent.length === 1) await gate;
    return receipt(body);
  });
  await expect(page.getByRole('img', { name: 'Мужской пол', exact: true }).first()).toBeVisible();
  await expect(page.getByRole('img', { name: 'Женский пол', exact: true }).first()).toBeVisible();
  await typeAndSend(page, 'Первое');
  await expect(page.getByRole('textbox', { name: 'Сообщение' })).toHaveValue('');
  await expect(page.getByText('Отправляется…', { exact: true })).toBeVisible();
  await typeAndSend(page, 'Убрать до отправки');
  expect(sent).toEqual(['Первое']);
  await page.getByRole('button', { name: 'Убрать сообщение из очереди', exact: true }).click();
  await typeAndSend(page, 'Следующее');
  await page.screenshot({ path: 'test-results/outbox-light.png' });
  await page.evaluate(() => document.documentElement.dataset.theme = 'dark');
  await page.setViewportSize({ width: 414, height: 896 });
  await page.screenshot({ path: 'test-results/outbox-dark-mobile.png' });
  release();
  await expect.poll(() => sent).toEqual(['Первое', 'Следующее']);
  await expect(page.getByRole('region', { name: 'Очередь сообщений' })).toHaveCount(0);
});

test('failed send survives reload and retry keeps the same delivery id', async ({ page }) => {
  const ids: string[] = [];
  await setup(page, async body => {
    ids.push(body.client_id);
    return ids.length === 1 ? false : receipt(body);
  });
  await typeAndSend(page, 'Не потерять');
  await expect(page.getByRole('button', { name: 'Повторить', exact: true })).toBeVisible();
  await page.reload();
  await expect(page.getByRole('button', { name: 'Повторить', exact: true })).toBeVisible();
  await page.getByRole('button', { name: 'Повторить', exact: true }).click();
  await expect(page.locator('.utext')).toHaveText('Не потерять');
  expect(ids).toHaveLength(2);
  expect(ids[0]).toBe(ids[1]);
});

test('large chat requests a small page and prepends history without losing its scroll anchor', async ({ page }) => {
  const requested: URL[] = [];
  const messages = Array.from({ length: 60 }, (_, n) => ({ id: `h${n}`, thread_id: 't1', author_kind: 'user', author_id: null, body: `Сообщение ${n}\n${'Строка истории\n'.repeat(5)}`, status: 'done', props: {}, created_at: at + n, updated_at: at + n }));
  await setup(page, async body => receipt(body), url => {
    requested.push(url);
    return { messages: url.searchParams.has('before') ? messages.slice(0, 30) : messages.slice(30), has_more: !url.searchParams.has('before') };
  });
  await expect(page.locator('.utext')).toHaveCount(30);
  expect(requested[0].searchParams.get('limit')).toBe('30');
  await page.locator('.scroll').evaluate(el => el.scrollTop = 0);
  await page.getByRole('button', { name: 'Показать раньше', exact: true }).click();
  await expect(page.locator('.utext')).toHaveCount(60);
  expect(requested[1].searchParams.get('before')).toBe('h30');
  const position = await page.locator('.scroll').evaluate(el => ({ top: el.scrollTop, max: el.scrollHeight - el.clientHeight }));
  expect(position.top).toBeGreaterThan(0);
  expect(position.top).toBeLessThan(position.max - 100);
});
