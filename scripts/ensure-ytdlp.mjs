// 确保打包资源 src-tauri/resources/yt-dlp.exe 存在（构建期依赖，gitignored 不进仓库）：
// 缺失时从 GitHub 下载最新版。幂等——已存在直接退出。
// 失败可手动下载 https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe 放到该路径。
import { createWriteStream, existsSync, mkdirSync } from "node:fs";
import { pipeline } from "node:stream/promises";
import { Readable } from "node:stream";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.join(path.dirname(fileURLToPath(import.meta.url)), "..");
const destDir = path.join(root, "src-tauri", "resources");
const dest = path.join(destDir, "yt-dlp.exe");
const url = "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe";

if (existsSync(dest)) {
  process.exit(0);
}

console.log(`[ensure-ytdlp] 下载 ${url}`);
mkdirSync(destDir, { recursive: true });
const resp = await fetch(url, { redirect: "follow" });
if (!resp.ok || !resp.body) {
  console.error(`[ensure-ytdlp] 下载失败: HTTP ${resp.status}`);
  console.error(`[ensure-ytdlp] 可手动下载 ${url} 放到 ${dest}`);
  process.exit(1);
}
await pipeline(Readable.fromWeb(resp.body), createWriteStream(dest));
console.log(`[ensure-ytdlp] 已写入 ${dest}`);
