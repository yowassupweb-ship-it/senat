import { test, expect, type Page } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';
const agents = [
  { id: 'a1', slug: 'analyst', name: 'Аналитик', role: 'Исследования и решения', accent: '#0064d9', model_pref: 'mock', profile: '' },
  { id: 'a2', slug: 'editor', name: 'Редактор', role: 'Тексты и коммуникации', accent: '#925700', model_pref: 'mock', profile: '' },
];
const thread = { id: 't1', kind: 'chat', title: 'План на неделю', model: 'mock', mode: 'edit', effort: '', updated_at: Date.now(), pinned: true };
async function setup(page: Page, theme = 'light') {
  await page.addInitScript(theme => { localStorage.setItem('senate.token', 'test-only'); localStorage.setItem('senate.theme', theme); }, theme);
  await page.route('**/api/**', async route => {
    const path = new URL(route.request().url()).pathname;
    if (path === '/api/events') return route.fulfill({ contentType: 'text/event-stream', body: ': connected\n\n' });
    let data: unknown = [];
    if (path === '/api/state') data = { agents, projects: [], companies: [], accounts: [], threads: [thread], backends: [], default_backend: 'mock', version: '0.1.0', node_id: 'test', attention: { approvals: [], needs_user: [], running: [], overdue: [], queued_sessions: 0, proposed_decisions: [] } };
    if (path === '/api/threads') data = route.request().method() === 'POST' ? thread : [thread];
    if (path.endsWith('/messages')) {
      if (route.request().method() === 'POST') return route.fulfill({ status: 503, json: { error: 'Тест: соединение прервано' } });
      data = { messages: [], has_more: false };
    }
    if (path === '/api/settings') data = { effective_backend: 'mock', auto_learn: false };
    if (path === '/api/grants') data = { grants: [], actions: [] };
    await route.fulfill({ json: data });
  });
  await page.goto('/');
  await expect(page.getByRole('heading', { name: 'Над чем работаем?' })).toBeVisible();
}
for (const theme of ['light', 'dark']) {
  test(`${theme}: screens, accessibility and responsive layouts`, async ({ page }) => {
    const errors: string[] = [];
    page.on('pageerror', e => errors.push(e.message));
    await setup(page, theme);
    await page.setViewportSize({ width: 1440, height: 960 });
    await page.screenshot({ path: `test-results/${theme}-chat.png` });
    for (const route of ['/', '/agents', '/projects', '/companies', '/obligations', '/memory', '/mcp', '/api', '/settings']) {
      await page.goto('/#' + route);
      await page.waitForTimeout(100);
      const audit = await new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa', 'wcag21aa']).analyze();
      expect(audit.violations.map(v => ({ id: v.id, nodes: v.nodes.map(n => n.target) })), route).toEqual([]);
      for (const width of [280, 320, 414, 1440]) {
        await page.setViewportSize({ width, height: 960 });
        const overflow = await page.evaluate(() => document.documentElement.scrollWidth > innerWidth);
        expect(overflow, `${route} @ ${width}`).toBe(false);
        const outside = await page.locator('main').evaluate(el => [...el.querySelectorAll('button,input,textarea,select')].filter(el => {
          const r = el.getBoundingClientRect(); return r.width && (r.right > innerWidth + 1 || r.left < -1);
        }).map(el => el.textContent?.slice(0, 80)));
        expect(outside, `${route} controls @ ${width}`).toEqual([]);
      }
    }
    await page.screenshot({ path: `test-results/${theme}-settings.png` });
    expect(errors).toEqual([]);
  });
}
test('keyboard menus, dialogs, drafts and mobile navigation', async ({ page }) => {
  await setup(page);
  const trigger = page.getByRole('button', { name: 'Специалист', exact: true });
  await trigger.focus(); await page.keyboard.press('ArrowDown');
  await expect(page.getByRole('listbox')).toBeVisible();
  await page.keyboard.press('ArrowDown'); await page.keyboard.press('Enter');
  await expect(trigger).toContainText('Аналитик');
  await trigger.click(); await page.keyboard.press('Escape'); await expect(trigger).toBeFocused();
  const search = page.getByRole('button', { name: 'Поиск и команды' });
  await search.click(); await expect(page.getByRole('dialog')).toBeVisible();
  await page.keyboard.press('Escape'); await expect(search).toBeFocused();
  await page.getByRole('button', { name: 'Форум', exact: true }).click();
  await expect(page.getByRole('dialog')).toBeVisible();
  await page.keyboard.press('Escape'); await expect(page.getByRole('dialog')).toHaveCount(0);
  await page.getByRole('textbox', { name: 'Сообщение' }).fill('Не потерять этот черновик');
  await page.getByRole('button', { name: 'Отправить', exact: true }).click();
  await expect(page.getByRole('alert')).toContainText('соединение прервано');
  await expect(page.getByRole('textbox', { name: 'Сообщение' })).toHaveValue('Не потерять этот черновик');
  await page.setViewportSize({ width: 320, height: 800 });
  await page.getByRole('button', { name: 'Открыть навигацию' }).click();
  await page.getByRole('button', { name: 'Настройки', exact: true }).click();
  await expect(page.getByRole('heading', { name: 'Настройки', exact: true })).toBeVisible();
  await expect(page.getByRole('navigation')).toBeHidden();
  await page.screenshot({ path: 'test-results/mobile-settings.png' });
});
test('successful send clears draft and prevents duplicate submission', async ({ page }) => {
  await setup(page);
  let sent = 0;
  await page.route('**/api/threads/t1/messages', async route => {
    if (route.request().method() === 'POST') {
      sent++;
      await new Promise(resolve => setTimeout(resolve, 200));
      return route.fulfill({ json: { user_message: { id: 'm1', thread_id: 't1', author_kind: 'user', body: 'Готово', props: {}, created_at: Date.now(), status: 'done' }, agent_messages: [] } });
    }
    return route.fulfill({ json: { messages: [], has_more: false } });
  });
  await page.getByRole('textbox', { name: 'Сообщение' }).fill('Готово');
  await page.keyboard.press('Enter');
  await expect(page.getByRole('button', { name: 'Отправка…' })).toBeDisabled();
  await page.keyboard.press('Enter');
  await expect(page).toHaveURL(/chat\/t1/);
  await expect(page.getByRole('textbox', { name: 'Сообщение' })).toHaveValue('');
  expect(sent).toBe(1);
});
test('connection can retry after failure', async ({ page }) => {
  await page.route('**/api/state', route => route.fulfill({ status: 401, json: { error: 'Unauthorized' } }));
  await page.goto('/');
  await page.getByLabel('Токен', { exact: true }).fill('invalid-test-token');
  await page.getByRole('button', { name: 'Подключиться', exact: true }).click();
  await expect(page.getByRole('alert')).toContainText('Токен не подошёл');
  await expect(page.getByRole('button', { name: 'Подключиться', exact: true })).toBeEnabled();
});
test('interactive states and overlays stay accessible in both themes', async ({ page }) => {
  await setup(page);
  for (const theme of ['light', 'dark']) {
    await page.evaluate(theme => document.documentElement.dataset.theme = theme, theme);
    const primary = page.getByRole('button', { name: 'Новый чат', exact: true });
    await primary.hover(); await primary.focus();
    expect((await new AxeBuilder({ page }).withTags(['wcag2aa', 'wcag21aa']).analyze()).violations).toEqual([]);
    await page.getByRole('button', { name: 'Специалист', exact: true }).click();
    await expect(page.getByRole('listbox')).toBeVisible();
    await page.evaluate(() => Promise.all(document.getAnimations().map(a => a.finished)));
    expect((await new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa']).analyze()).violations).toEqual([]);
    await page.keyboard.press('Escape');
    await page.getByRole('button', { name: 'Форум', exact: true }).click();
    await page.evaluate(() => Promise.all(document.getAnimations().map(a => a.finished)));
    expect((await new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa']).analyze()).violations).toEqual([]);
    await page.keyboard.press('Escape');
  }
});
