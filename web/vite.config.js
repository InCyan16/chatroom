import { defineConfig } from "vite";
import preact from "@preact/preset-vite";

export default defineConfig({
  base: "./",
  plugins: [preact()],
  server: {
    port: 5173,
    strictPort: true,
    proxy: {
      "/api": "http://127.0.0.1:3000",
      "/ws": { target: "ws://127.0.0.1:3000", ws: true },
    },
  },
});
