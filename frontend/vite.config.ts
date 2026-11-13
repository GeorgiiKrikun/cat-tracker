import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

// In dev, Vite serves the UI on :5173 and forwards /api to the Rust backend,
// so the browser sees one origin and the session cookie just works.
export default defineConfig({
  plugins: [svelte()],
  server: {
    host: true,
    proxy: { '/api': 'http://localhost:8080' },
  },
});
