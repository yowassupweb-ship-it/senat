import { test, expect, type Page } from '@playwright/test';
const at = Date.now();
const agents = [
  { id: 'a', slug: 'author', name: 'Исполнитель', role: 'developer', model_pref: 'mock' },
  { id: 'b', slug: 'reviewer', name: 'Проверяющий', role: 'analyst', model_pref: 'mock' },
];
const project = { id: 'p', name: 'Командный проект', goal: 'Выпустить страницу', criteria: 'Все проверки проходят', constraints: '', root_path: '', mode: 'edit', status: 'active', members: agents };
const task = (id: string, title: string, state: string, extra = {}) => ({ id, title, state, owner_id: 'a', project_id: 'p', result: 'Готовый материал', criteria: 'Проверено', dependencies: [], reviewer_id: null, next_step: '', blocker: '', outcome: '', created_at: at, updated_at: at, ...extra });
async function setup(page: Page) {
  const tasks = [task('text', 'Подготовить текст', 'running'), task('page', 'Собрать страницу', 'waiting', { dependencies: ['text'], wait_condition: { type: 'obligations', ids: ['text'] } }), task('review', 'Проверить макет', 'review', { reviewer_id: 'b', outcome: 'Макет готов', evidence: 'layout.png; проверены размеры' })];
  const writes: any[] = [];
  page.on('pageerror', e => { throw e; });
  await page.addInitScript(() => {
    localStorage.setItem('senate.token', 'test');
    (window as any).EventSource = class { onopen: any; onmessage: any; constructor() { (window as any).teamEvents = this; setTimeout(() => this.onopen?.(), 0); } close() {} };
  });
  await page.route('**/api/**', async route => {
    const path = new URL(route.request().url()).pathname;
    let data: any = [];
    if (path === '/api/state') data = { agents, projects: [project], companies: [], threads: [], accounts: [], backends: [], node_id: 'team-test', attention: { approvals: [], needs_user: [], running: [], overdue: [], queued_sessions: 0, proposed_decisions: [] } };
    if (path === '/api/projects/p') data = { project, members: agents, obligations: tasks, decisions: [], threads: [], memory: [] };
    if (path === '/api/attention') data = { approvals: [], needs_user: [], running: [], overdue: [], queued_sessions: 0, proposed_decisions: [] };
    if (path === '/api/obligations') data = tasks;
    const match = path.match(/^\/api\/obligations\/([^/]+)(?:\/(transition))?$/);
    if (match) {
      const current = tasks.find((t) => t.id === match[1])!;
      if (route.request().method() === 'POST') {
        const body = route.request().postDataJSON(); writes.push(body); current.state = body.to; data = current;
      } else if (route.request().method() === 'PATCH') {
        const body = route.request().postDataJSON(); writes.push(body); Object.assign(current, body); data = current;
      } else data = { obligation: current, dependencies: tasks.filter((t) => current.dependencies.includes(t.id)), children: [], journal: [], sessions: [], thread_id: null };
    }
    await route.fulfill({ json: data });
  });
  return writes;
}
test('project shows owners, prerequisites, reviewer and updates without overwriting a draft', async ({ page }) => {
  await setup(page);
  await page.goto('/#/projects/p');
  const team = page.getByRole('region', { name: 'Работа команды' });
  await expect(team).toContainText('Ждёт: Подготовить текст — Исполнитель');
  await expect(team).toContainText('Проверяет: Проверяющий');
  await expect(team).toContainText('От результата зависят задач: 1');
  await page.getByLabel('Цель', { exact: true }).fill('Несохранённая цель');
  await page.evaluate(() => (window as any).teamEvents.onmessage?.({ data: JSON.stringify({ type: 'attention', data: {} }) }));
  await expect(page.getByLabel('Цель', { exact: true })).toHaveValue('Несохранённая цель');
  await page.screenshot({ path: 'test-results/team-light.png', fullPage: true, animations: 'disabled' });
  await page.evaluate(() => document.documentElement.dataset.theme = 'dark');
  await page.setViewportSize({ width: 414, height: 896 });
  await page.screenshot({ path: 'test-results/team-dark-mobile.png', fullPage: true, animations: 'disabled' });
  await team.getByRole('button', { name: /Собрать страницу/ }).click();
  await expect(page).toHaveURL(/#\/obligations\/page$/);
  await expect(page.getByRole('heading', { name: 'Зависимости', exact: true })).toBeVisible();
  await expect(page.getByRole('button', { name: /Подготовить текст/ }).first()).toBeVisible();
});
test('returning a result requires concrete remarks and sends them', async ({ page }) => {
  const writes = await setup(page);
  await page.goto('/#/obligations/review');
  const button = page.getByRole('button', { name: 'Вернуть в работу', exact: true });
  await expect(button).toBeDisabled();
  await page.getByLabel('Замечания для доработки').fill('Исправить переполнение на телефоне');
  await button.click();
  await expect.poll(() => writes).toContainEqual({ to: 'accepted', note: 'Исправить переполнение на телефоне' });
});
test('task dependencies can be explicitly changed', async ({ page }) => {
  const writes = await setup(page);
  await page.goto('/#/obligations/page');
  const dependency = page.getByRole('checkbox', { name: 'Подготовить текст' });
  await expect(dependency).toBeChecked();
  await dependency.uncheck();
  await expect.poll(() => writes).toContainEqual({ dependencies: [] });
});
