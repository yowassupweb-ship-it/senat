import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

// Dev: the UI runs on :5173 and talks to a node on :7777 (senate-node serve).
export default defineConfig({
  plugins: [svelte()],
  server: { port: 5173, proxy: { '/api': { target: 'http://127.0.0.1:7777', changeOrigin: true } } },
  build: { target: 'es2022', sourcemap: false, chunkSizeWarningLimit: 700 },
});
