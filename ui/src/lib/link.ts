// The desktop keeps the connection to a server node alive by itself (an SSH tunnel it owns). This reads what it knows,
// so the screen can say what is happening instead of "no connection". Outside the desktop there is nothing to read.

export interface LinkState {
  up: boolean;
  detail: string;
  restarts: number;
}

/** True when this window's app owns a tunnel to the node. */
export const managedLink = () => !!(window as any).__SENATE__?.link;

export async function readLink(): Promise<LinkState | null> {
  const internals = (window as any).__TAURI_INTERNALS__;
  if (!managedLink() || !internals?.invoke) return null;
  try {
    return (await internals.invoke('link_state')) as LinkState;
  } catch {
    return null;
  }
}

/** The computer this window is on, as the node knows it. Null in a browser: there the window is on no computer of the person's. */
export async function readThisMachine(): Promise<{ id: string; name: string } | null> {
  const internals = (window as any).__TAURI_INTERNALS__;
  if (!internals?.invoke) return null;
  try {
    return ((await internals.invoke('this_machine')) as { id: string; name: string } | null) ?? null;
  } catch {
    return null;
  }
}
