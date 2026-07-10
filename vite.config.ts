import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

// Tauri 默认会接管 dev server，关闭清屏以保留日志
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: false,
  },
  // Tauri 期望相对路径资源
  base: './',
  build: {
    outDir: 'dist',
    sourcemap: false,
  },
});
