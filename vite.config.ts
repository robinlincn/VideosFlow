import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

// Tauri 默认会接管 dev server，关闭清屏以保留日志
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: false,
    // 忽略 Rust target 目录（Tauri 二进制持有 .dll 文件锁，避免 EBUSY）
    watch: {
      ignored: [
        '**/src-tauri/target/**',
        '**/dist/**',
        '**/node_modules/**',
      ],
    },
  },
  // Tauri 期望相对路径资源
  base: './',
  build: {
    outDir: 'dist',
    sourcemap: false,
  },
});
