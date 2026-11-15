import { test, expect } from '@playwright/test';

test('chat keeps rendering and refreshing status during a continuous answer', async ({ page }) => {
  let liveReads = 0;
  let threadReads = 0;
  const at = Date.now();
  const thread = { id: 't1', kind: 'chat', title: 'Поток', model: 'mock', mode: 'edit', effort: '', updated_at: at };
  const message = { id: 'm1', thread_id: 't1', author_kind: 'agent', author_id: 'a1', body: 'Начало ответа', status: 'working', props: {}, created_at: at, updated_at: at };
  await page.addInitScript(() => {
    localStorage.setItem('senate.token', 'test-only');
    (window as any).EventSource = class {
      onmessage: any;
      onopen: any;
      constructor() { (window as any).testEvents = this; setTimeout(() => this.onopen?.(), 0); }
      close() {}
    };
  });
  await page.route('**/api/**', async route => {
    const path = new URL(route.request().url()).pathname;
    let data: unknown = [];
    if (path === '/api/state') data = {
      agents: [{ id: 'a1', slug: 'analyst', name: 'Аналитик', role: 'analyst', model_pref: 'mock' }],
      projects: [], companies: [], accounts: [], threads: [thread], backends: [], default_backend: 'mock', node_id: 'test',
      attention: { approvals: [], needs_user: [], running: [], overdue: [], queued_sessions: 0, proposed_decisions: [] },
    };
    if (path === '/api/live') liveReads++;
    if (path === '/api/threads') { threadReads++; data = [thread]; }
    if (path.endsWith('/messages')) data = { messages: [message], has_more: false };
    await route.fulfill({ json: data });
  });
  await page.goto('/#/chat/t1');
  await expect(page.getByText('Начало ответа', { exact: true })).toBeVisible();
  await page.evaluate(message => {
    let n = 0;
    (window as any).testStream = setInterval(() => {
      (window as any).testEvents.onmessage({ data: JSON.stringify({ type: 'message', data: { ...message, body: `Ответ ${++n}`, updated_at: Date.now() } }) });
    }, 30);
  }, message);
  await expect.poll(() => liveReads).toBeGreaterThanOrEqual(3);
  expect(threadReads).toBeLessThanOrEqual(1);
  await page.evaluate(message => {
    clearInterval((window as any).testStream);
    (window as any).testEvents.onmessage({ data: JSON.stringify({ type: 'message', data: { ...message, body: 'Ответ завершён', status: 'done', updated_at: Date.now() } }) });
  }, message);
  await expect(page.getByText('Ответ завершён', { exact: true })).toBeVisible();
  await expect.poll(() => threadReads).toBeGreaterThan(0);
  await page.screenshot({ path: 'test-results/streaming-chat.png' });
});
