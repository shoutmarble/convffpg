import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  root: __dirname,
  plugins: [react()],
  server: {
    port: 1420,
    strictPort: true,
    host: "0.0.0.0"
  },
  build: {
    outDir: "dist",
    emptyOutDir: true
  }
});