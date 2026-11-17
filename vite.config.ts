import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Configuração recomendada pelo Tauri: porta fixa (o tauri.conf.json aponta pra ela)
// e o Vite ignora a pasta src-tauri pra não recarregar a página a cada build do Rust.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**"] },
  },
  build: {
    // WebView2 (Chromium) é o motor no Windows, então dá pra mirar um alvo moderno.
    target: "chrome110",
    sourcemap: false,
  },
});
