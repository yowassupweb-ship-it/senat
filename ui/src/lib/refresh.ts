/** Coalesce invalidations without postponing them indefinitely during a stream.
 * Only one request runs at a time; changes during it cause one trailing refresh. */
export function coalescedRefresh(run: () => Promise<unknown>, delay: number) {
  let timer: ReturnType<typeof setTimeout> | undefined;
  let running = false;
  let dirty = false;
  const schedule = (now = false) => {
    dirty = true;
    if (running) return;
    if (timer !== undefined) {
      if (!now) return;
      clearTimeout(timer);
    }
    timer = setTimeout(async () => {
      timer = undefined;
      running = true;
      dirty = false;
      try { await run(); } catch { /* retain the last successful snapshot */ }
      finally {
        running = false;
        if (dirty) schedule();
      }
    }, now ? 0 : delay);
  };
  return schedule;
}
