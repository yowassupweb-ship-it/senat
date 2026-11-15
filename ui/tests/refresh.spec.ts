import { test, expect } from '@playwright/test';
import { coalescedRefresh } from '../src/lib/refresh';

test('continuous events cannot starve refreshes', async () => {
  let calls = 0;
  const refresh = coalescedRefresh(async () => { calls++; }, 20);
  const stream = setInterval(() => refresh(), 2);
  try {
    await expect.poll(() => calls, { timeout: 2000 }).toBeGreaterThan(2);
  } finally { clearInterval(stream); }
});

test('slow requests never overlap and retain one trailing invalidation', async () => {
  let calls = 0;
  let finish: (() => void) | undefined;
  const refresh = coalescedRefresh(async () => {
    calls++;
    await new Promise<void>(resolve => { finish = resolve; });
  }, 5);
  refresh(true);
  await expect.poll(() => calls).toBe(1);
  for (let i = 0; i < 100; i++) refresh(true);
  await new Promise(resolve => setTimeout(resolve, 30));
  expect(calls).toBe(1);
  finish!();
  await expect.poll(() => calls).toBe(2);
  finish!();
  await new Promise(resolve => setTimeout(resolve, 30));
  expect(calls).toBe(2);
});

test('a failed request does not disable future refreshes', async () => {
  let calls = 0;
  const refresh = coalescedRefresh(async () => { if (++calls === 1) throw new Error('offline'); }, 5);
  refresh(true);
  await expect.poll(() => calls).toBe(1);
  refresh(true);
  await expect.poll(() => calls).toBe(2);
});
