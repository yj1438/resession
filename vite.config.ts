import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// 构建时间戳注入前端：状态栏可见，exe 内嵌资源是否新鲜一眼可辨
// （源自"改了 dist 忘了重编 exe"的部署错位教训）
export default defineConfig({
  plugins: [react()],
  define: {
    __BUILD_DATE__: JSON.stringify(new Date().toISOString().slice(0, 16).replace("T", " ")),
  },
});
