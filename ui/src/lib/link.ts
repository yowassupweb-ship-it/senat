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
