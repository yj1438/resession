import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// M1 接入 Tauri 后在此补充 clearScreen/false 与 host/port 约定
export default defineConfig({
  plugins: [react()],
});
