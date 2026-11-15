import { test, expect } from '@playwright/test';

test('long chat exposes context, compact controls and workspace binding', async ({ page }) => {
  const at = Date.now();
  const project = { id: 'p', name: 'Проект', root_path: '', mode: 'edit' };
  const thread = { id: 't', kind: 'chat', project_id: 'p', title: 'Длинный чат проекта', model: 'mock', mode: 'edit', effort: '', updated_at: at };
  let context = { auto: true, pending: 0, participants: [{ id: 'a', name: 'Автор', tokens: 75000, window: 100000, compacted: false }] };
  const writes: any[] = [];
  page.on('pageerror', e => { throw e; });
  await page.addInitScript(() => {
    localStorage.setItem('senate.token', 'test');
    (window as any).EventSource = class { onopen: any; onmessage: any; constructor() { setTimeout(() => this.onopen?.(), 0); } close() {} };
  });
  await page.route('**/api/**', async route => {
    const path = new URL(route.request().url()).pathname;
    const method = route.request().method();
    let data: any = [];
    if (path === '/api/state') data = { agents: [{ id: 'a', slug: 'author', name: 'Автор', model_pref: 'mock' }], projects: [project], companies: [], threads: [thread], accounts: [], backends: [], node_id: 'context-test', attention: { approvals: [], needs_user: [], running: [], overdue: [], queued_sessions: 0, proposed_decisions: [] } };
    if (path === '/api/live') data = [{ session_id: 's', since: at, agent_id: 'a', thread_id: 't', status: 'running', phase: 'работает', hint: 'Читает README.md', workspace: { project_id: 'p', machine: '', path: '/work/original', captured: true } }];
    if (path.endsWith('/tree')) data = { entries: [{ name: 'README.md', path: 'README.md', dir: false, size: 24 }] };
    if (path.endsWith('/workspace') && method === 'GET') data = { locations: [{ machine: '', path: '/work/project' }] };
    if (path === '/api/projects') data = [project];
    if (path === '/api/threads') data = [thread];
    if (path === '/api/machines') data = { machines: [] };
    if (path.endsWith('/messages')) data = { messages: [{ id: 'm', thread_id: 't', author_kind: 'user', body: 'Исходное поручение', props: {}, status: 'done', created_at: at }], has_more: false };
    if (path.endsWith('/context')) {
      if (method === 'POST') { const b = route.request().postDataJSON(); writes.push(b); if ('auto' in b) context.auto = b.auto; if (b.compact) context.pending = 1; }
      data = context;
    }
    if (path.endsWith('/workspace') && method === 'POST') { const b = route.request().postDataJSON(); writes.push(b); project.root_path = b.path; data = { status: 'bound' }; }
    await route.fulfill({ json: data });
  });
  await page.goto('/#/chat/t');
  const bar = page.getByRole('region', { name: 'Контекст чата' });
  await expect(bar.getByRole('button', { name: /Контекст 75%/ })).toBeVisible();
  await bar.getByRole('button', { name: /Контекст 75%/ }).click();
  await expect(bar).toContainText('75 000 / 100 000');
  await bar.getByRole('checkbox').uncheck();
  await expect.poll(() => writes).toContainEqual({ auto: false });
  await bar.getByLabel('Что сохранить в сводке').fill('Незавершённые задачи');
  await bar.getByRole('button', { name: 'Сжать контекст участников' }).click();
  await expect.poll(() => writes).toContainEqual({ compact: true, instructions: 'Незавершённые задачи' });
  await expect(bar.getByRole('button', { name: 'Сжать контекст участников' })).toBeDisabled();
  await expect(page.getByText('Исходное поручение')).toBeVisible();
  await page.getByRole('button', { name: 'Привязать проект / Git' }).click();
  const workspace = page.getByRole('region', { name: 'Настройки проекта чата' });
  await workspace.getByLabel('Папка проекта').fill('/work/project');
  await workspace.getByRole('button', { name: 'Сохранить привязку' }).click();
  await expect.poll(() => writes).toContainEqual({ path: '/work/project', machine: '' });
  await expect(workspace).not.toBeVisible();
  await page.getByRole('navigation').getByRole('button', { name: 'Папка проекта', exact: true }).click();
  await expect(page.getByRole('region', { name: 'Кто работает в проекте' })).toContainText('/work/original');
  await expect(page.getByRole('complementary', { name: 'Проект' }).getByText('README.md', { exact: true })).toBeVisible();
  await page.screenshot({ path: 'test-results/project-workspace.png', animations: 'disabled', fullPage: true });
  await page.getByRole('button', { name: 'Закрыть панель', exact: true }).click();
  await page.screenshot({ path: 'test-results/context-desktop.png', animations: 'disabled', fullPage: true });
  await page.setViewportSize({ width: 414, height: 896 });
  await page.evaluate(() => document.documentElement.dataset.theme = 'dark');
  await page.screenshot({ path: 'test-results/context-mobile.png', animations: 'disabled', fullPage: true });
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
});
