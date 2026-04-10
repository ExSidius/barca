import path from "path"
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'

export default defineConfig({
  base: '/ui/',
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
  server: {
    proxy: {
      '/api': 'http://localhost:8401',
      '/sse': 'http://localhost:8401',
    }
  },
  build: {
    outDir: 'dist',
    // Put built assets under "static/" to avoid collision with /ui/assets/:id route
    assetsDir: 'static',
  }
})
