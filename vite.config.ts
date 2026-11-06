import { svelte } from '@sveltejs/vite-plugin-svelte'
import tailwindcss from '@tailwindcss/vite'
import { svelteTesting } from '@testing-library/svelte/vite'
import { fileURLToPath, URL } from 'node:url'
import { coverageConfigDefaults, defineConfig } from 'vitest/config'

const host = process.env.TAURI_DEV_HOST

// https://v2.tauri.app/start/frontend/vite/
export default defineConfig({
  plugins: [tailwindcss(), svelte(), ...(process.env.VITEST ? [svelteTesting()] : [])],
  resolve: {
    alias: { $lib: fileURLToPath(new URL('./src/lib', import.meta.url)) },
  },
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: 'ws', host, port: 5174 } : undefined,
    watch: { ignored: ['**/src-tauri/**'] },
  },
  envPrefix: ['VITE_', 'TAURI_ENV_*'],
  build: {
    target: process.env.TAURI_ENV_PLATFORM === 'windows' ? 'chrome105' : 'safari15',
    minify: !process.env.TAURI_ENV_DEBUG,
    sourcemap: !!process.env.TAURI_ENV_DEBUG,
    // One page per window: the overlay must not load the Settings UI.
    rollupOptions: {
      input: {
        main: fileURLToPath(new URL('./index.html', import.meta.url)),
        overlay: fileURLToPath(new URL('./overlay.html', import.meta.url)),
        permission: fileURLToPath(new URL('./permission.html', import.meta.url)),
        notice: fileURLToPath(new URL('./notice.html', import.meta.url)),
      },
    },
  },
  test: {
    environment: 'jsdom',
    setupFiles: ['./src/test/setup.ts'],
    // scripts/ tests opt into the Node environment with a `@vitest-environment node` comment.
    include: ['src/**/*.test.ts', 'scripts/**/*.test.ts', 'site/src/**/*.test.ts'],
    coverage: {
      provider: 'v8',
      include: ['src/**/*.{ts,svelte}', 'scripts/**/*.ts', 'site/src/**/*.ts'],
      // Generated shadcn-svelte components, test helpers and the entry point.
      exclude: [
        ...coverageConfigDefaults.exclude,
        'src/lib/components/ui/**',
        'src/test/**',
        'src/main.ts',
        'src/overlay/main.ts',
        'src/permission/main.ts',
        'site/src/main.ts',
        'src/notice/main.ts',
        'scripts/**/*.test.ts',
        'site/src/**/*.test.ts',
        // Drives a browser; its pure parts are in scripts/media.ts.
        'scripts/readme-media.ts',
      ],
      thresholds: { lines: 90, functions: 90, branches: 90, statements: 90 },
      reporter: ['text'],
    },
  },
})
