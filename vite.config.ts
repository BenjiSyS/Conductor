/// <reference types="vitest/config" />
import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

export default defineConfig({
  root: 'apps/desktop/ui',
  plugins: [svelte()],
  clearScreen: false,
  build: { outDir: '../dist', emptyOutDir: true, target: 'es2022', chunkSizeWarningLimit: 900 },
  server: { strictPort: true, port: 1420, host: '127.0.0.1' },
  test: { include: ['../../../tests/unit/**/*.test.ts'], environment: 'node' },
});
