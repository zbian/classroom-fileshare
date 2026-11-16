import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import { fileURLToPath, URL } from 'node:url'

// 三入口：icon 浮动图标 / main 主界面 / tips 气泡（lead 2026-11-13 修订采纳）
// 端口 1517：1420 落入 Windows WinNAT 动态保留段（本机实测 1348–1447）导致
// EACCES 无法监听且非管理员不可释放；1517 避开常见保留段，与 tauri.conf.json devUrl 对齐
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  envPrefix: 'VITE_',
  server: {
    port: 1517,
    strictPort: true,
  },
  build: {
    // WebView2 Evergreen 内核，无需兼容旧浏览器
    target: 'chrome110',
    rollupOptions: {
      input: {
        icon: fileURLToPath(new URL('./icon.html', import.meta.url)),
        main: fileURLToPath(new URL('./main.html', import.meta.url)),
        tips: fileURLToPath(new URL('./tips.html', import.meta.url)),
      },
    },
  },
})
