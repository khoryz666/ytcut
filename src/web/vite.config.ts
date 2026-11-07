import { defineConfig, type ProxyOptions } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

/** Ask the Rust server for every API and media path while the page runs on Vite. */
const backend = 'http://127.0.0.1:8765';
const paths = ['alive', 'bye', 'jobs', 'files', 'info', 'download', 'rename', 'delete', 'media'];
const proxy: Record<string, ProxyOptions> = Object.fromEntries(
  paths.map((path) => [
    `/${path}`,
    {
      target: backend,
      changeOrigin: true,
      configure: (server) => {
        // Vite runs on another port; make the request look like it came from the page.
        // The http-proxy typings Vite ships hide ProxyServer.on, so name the one method used.
        const proxy = server as unknown as {
          on: (event: string, handler: (request: { setHeader: (name: string, value: string) => void }) => void) => void;
        };
        proxy.on('proxyReq', (request) => request.setHeader('origin', backend));
      },
    },
  ]),
);

export default defineConfig({
  plugins: [svelte()],
  server: { proxy },
  build: { outDir: 'dist', emptyOutDir: true },
});
