import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { execSync } from 'node:child_process';

// What this UI was built from, shown next to the node's own stamp: a mismatch is visible, not guessed.
function stamp(): string {
  try {
    const commit = execSync('git rev-parse --short HEAD', { stdio: ['ignore', 'pipe', 'ignore'] }).toString().trim();
    const dirty = execSync('git status --porcelain', { stdio: ['ignore', 'pipe', 'ignore'] }).toString().trim() !== '';
    return commit + (dirty ? '+edits' : '');
  } catch {
    return 'unknown';
  }
}

// Dev: the UI runs on :5173 and talks to a node on :7777 (senate-node serve).
export default defineConfig({
  plugins: [svelte()],
  define: { __UI_BUILD__: JSON.stringify(stamp()), __UI_BUILT_AT__: JSON.stringify(new Date().toISOString()) },
  server: { port: 5173, proxy: { '/api': { target: 'http://127.0.0.1:7777', changeOrigin: true } } },
  build: { target: 'es2022', sourcemap: false, chunkSizeWarningLimit: 700 },
});
