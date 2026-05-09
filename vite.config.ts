import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import path from 'path'

const host = process.env.TAURI_DEV_HOST;

const stripCrossorigin = () => ({
  name: 'strip-crossorigin-from-built-html',
  enforce: 'post' as const,
  transformIndexHtml(html: string) {
    return html.replace(/\s+crossorigin(?=[\s>])/g, '')
  },
})

export default defineConfig({
  plugins: [react(), stripCrossorigin()],
  resolve: {
    alias: {
      '@': path.resolve(__dirname, 'src'),
    },
  },
  base: './',
  build: {
    modulePreload: false,
    cssCodeSplit: false,
  },
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
    host: host || false,
    hmr: host ? {
      protocol: "ws",
      host,
      port: 5174,
    } : undefined,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
})
