import { defineConfig } from 'vite';
import { resolve } from 'path';

export default defineConfig({
  resolve: {
    alias: {
      '@src': resolve(__dirname, 'src'),
      '@www': resolve(__dirname, 'www'),
    },
    extensions: ['.ts', '.tsx', '.js', '.jsx', '.json', '.wasm'],
  },
  root: 'www',
  server: {
    port: 3000,
    open: '/playground.html',
  },
  build: {
    outDir: '../dist',
    rollupOptions: {
      input: {
        playground: resolve(__dirname, 'www/playground.html'),
      },
    },
  },
});
