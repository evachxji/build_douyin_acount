// 确保打包资源 src-tauri/resources/ffmpeg.exe 与 ffprobe.exe 存在（构建期依赖，gitignored 不进仓库）：
// 缺失时从 gyan.dev（ffmpeg.org 官方推荐 Windows 构建）下载 release essentials zip，
// 用 PowerShell Expand-Archive 解压后取 bin/ 下两个 exe。幂等——两个 exe 都在直接退出。
// 下载走 curl.exe（Windows 自带）：Node 内置 fetch(undici) 在这种 ~106MB 慢速大文件上会 terminated。
// 失败可手动下载 https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip
// 解压后把 bin/ffmpeg.exe、bin/ffprobe.exe 放到 src-tauri/resources/。
import { existsSync, mkdirSync, copyFileSync, rmSync, readdirSync } from "node:fs";
import { execFileSync } from "node:child_process";
import path from "node:path";
import os from "node:os";
import { fileURLToPath } from "node:url";

const root = path.join(path.dirname(fileURLToPath(import.meta.url)), "..");
const destDir = path.join(root, "src-tauri", "resources");
const exes = ["ffmpeg.exe", "ffprobe.exe"];
const url = "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip";

if (exes.every((n) => existsSync(path.join(destDir, n)))) {
  process.exit(0);
}

const tmpZip = path.join(os.tmpdir(), "ffmpeg-release-essentials.zip");
const tmpDir = path.join(os.tmpdir(), `ffmpeg-ensure-${process.pid}`);
try {
  console.log(`[ensure-ffmpeg] 下载 ${url}（约 106MB，耐心等）`);
  execFileSync("curl.exe", ["-sL", "--fail", "--max-time", "900", "-o", tmpZip, url], {
    stdio: "inherit",
  });

  console.log(`[ensure-ffmpeg] 解压中…`);
  mkdirSync(tmpDir, { recursive: true });
  execFileSync(
    "powershell",
    ["-NoProfile", "-Command", `Expand-Archive -LiteralPath '${tmpZip}' -DestinationPath '${tmpDir}' -Force`],
    { stdio: "inherit" },
  );

  // zip 结构：ffmpeg-<ver>-essentials_build/bin/{ffmpeg,ffprobe,ffplay}.exe
  const binDir = readdirSync(tmpDir)
    .map((d) => path.join(tmpDir, d, "bin"))
    .find((p) => exes.every((n) => existsSync(path.join(p, n))));
  if (!binDir) {
    throw new Error("解压后未找到 bin/ffmpeg.exe 与 bin/ffprobe.exe");
  }
  mkdirSync(destDir, { recursive: true });
  for (const n of exes) {
    copyFileSync(path.join(binDir, n), path.join(destDir, n));
    console.log(`[ensure-ffmpeg] 已写入 ${path.join(destDir, n)}`);
  }
} catch (e) {
  console.error(`[ensure-ffmpeg] 失败: ${e.message ?? e}`);
  console.error(`[ensure-ffmpeg] 可手动下载 ${url} 解压，把 bin/ffmpeg.exe、bin/ffprobe.exe 放到 ${destDir}`);
  process.exit(1);
} finally {
  rmSync(tmpZip, { force: true });
  rmSync(tmpDir, { recursive: true, force: true });
}
