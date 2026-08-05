// 短视频采集 sidecar（opencli + yt-dlp 编排）：
// - TikTok：opencli tiktok explore（推荐流，返回 URL/点赞/标题等）-> 点赞过滤 -> yt-dlp 下载
// - Instagram：opencli 打开 /reels/ 播放器 -> 逐条读取当前 reel URL（方向键下一条）-> yt-dlp 取赞数过滤 -> 下载
// 进度：progressFile 写状态与计数；videosFile（JSONL）逐条追加已下载视频元数据。
//
// 用法：node collect.js <job.json>
// job 字段：platform / count / maxLikes / outputDir / progressFile / videosFile /
//           cancelFile / ytdlpPath

import { execFile } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

const job = JSON.parse(fs.readFileSync(process.argv[2], "utf8"));
fs.mkdirSync(path.dirname(job.progressFile), { recursive: true });
fs.mkdirSync(job.outputDir, { recursive: true });

// opencli CLI 入口候选（nvm 多版本安装下的探测）。
const OPENCLI_ENTRIES = [
  "C:/Users/<user>/AppData/Local/nvm/v24.14.0/node_modules/@jackwener/opencli/dist/src/main.js",
  "C:/nvm4w/nodejs/node_modules/@jackwener/opencli/dist/src/main.js",
  "C:/Users/<user>/AppData/Roaming/npm/node_modules/@jackwener/opencli/dist/src/main.js",
];

const session = `collect-${job.platform}-${job.taskId ?? "x"}`;

function writeProgress(patch) {
  const base = { state: "running", downloaded: 0, target: job.count, message: "" };
  const prev = fs.existsSync(job.progressFile)
    ? JSON.parse(fs.readFileSync(job.progressFile, "utf8"))
    : base;
  fs.writeFileSync(
    job.progressFile,
    JSON.stringify({ ...prev, ...patch }, null, 2),
  );
  console.log(`[collector] ${patch.state ?? "running"} ${patch.message ?? ""}`);
}

function markerExists(file) {
  return fs.existsSync(file);
}

function runCmd(cmd, args, timeoutMs = 300_000) {
  return new Promise((resolve, reject) => {
    execFile(
      cmd,
      args,
      { timeout: timeoutMs, maxBuffer: 16 * 1024 * 1024 },
      (err, stdout) => {
        if (err) {
          reject(new Error(`${cmd} ${args.slice(0, 4).join(" ")}: ${err.message}`));
          return;
        }
        resolve(stdout);
      },
    );
  });
}

function runOpencli(args) {
  return new Promise((resolve, reject) => {
    const entry = OPENCLI_ENTRIES.find((p) => fs.existsSync(p));
    if (!entry) {
      reject(new Error("未找到 opencli CLI 入口"));
      return;
    }
    execFile(
      "node",
      [entry, ...args],
      { timeout: 300_000, maxBuffer: 16 * 1024 * 1024 },
      (err, stdout) => {
        if (err) {
          reject(
            new Error(
              `opencli ${args.slice(0, 3).join(" ")} 失败: ${(stdout || "").trim() || err.message}`,
            ),
          );
          return;
        }
        try {
          resolve(JSON.parse(stdout));
        } catch {
          reject(new Error(`opencli 输出解析失败: ${String(stdout).slice(0, 200)}`));
        }
      },
    );
  });
}

/** 运行 opencli 并返回原始文本（`get url` / `keys` 等非 JSON 输出）。 */
function runOpencliRaw(args) {
  return new Promise((resolve, reject) => {
    const entry = OPENCLI_ENTRIES.find((p) => fs.existsSync(p));
    if (!entry) {
      reject(new Error("未找到 opencli CLI 入口"));
      return;
    }
    execFile(
      "node",
      [entry, ...args],
      { timeout: 120_000, maxBuffer: 4 * 1024 * 1024 },
      (err, stdout) => {
        if (err) {
          reject(new Error(`opencli ${args.slice(0, 3).join(" ")} 失败: ${err.message}`));
          return;
        }
        resolve(stdout.trim());
      },
    );
  });
}

async function ytdlpMetadata(url) {
  const out = await runCmd(
    job.ytdlpPath,
    ["--skip-download", "--dump-json", url],
    600_000,
  );
  return JSON.parse(out);
}

async function ytdlpDownload(url, outBase) {
  const args = [
    "--no-playlist",
    "-f",
    "mp4/best",
    "--socket-timeout",
    "30",
    "--retries",
    "4",
  ];
  if (job.cookiesFile && fs.existsSync(job.cookiesFile)) {
    args.push("--cookies", job.cookiesFile);
  }
  args.push("-o", `${outBase}.%(ext)s`, url);
  await runCmd(job.ytdlpPath, args, 900_000);
  const files = fs.readdirSync(job.outputDir).filter((f) => f.startsWith(path.basename(outBase)));
  if (files.length === 0) throw new Error(`下载后未找到文件: ${outBase}`);
  return path.join(job.outputDir, files[0]);
}

function recordVideo(v) {
  fs.appendFileSync(job.videosFile, JSON.stringify(v) + "\n");
  writeProgress({
    downloaded: v.index,
    message: `已下载 ${v.index}/${job.count}: ${v.title || v.url}`,
  });
}

async function collectTiktok() {
  // 拉取 3 倍候选量，过滤后仍能凑够 count。
  const limit = Math.min(120, job.count * 3);
  const entries = await runOpencli(["tiktok", "explore", "--limit", String(limit), "-f", "json"]);
  const seen = new Set();
  const candidates = (entries ?? []).filter((e) => {
    if (seen.has(e.id) || !e.url) return false;
    seen.add(e.id);
    return e.likes <= job.maxLikes;
  });

  let index = 0;
  for (const e of candidates) {
    if (markerExists(job.cancelFile)) {
      writeProgress({ state: "cancelled", message: "用户取消" });
      process.exit(2);
    }
    if (index >= job.count) break;
    index += 1;
    const outBase = path.join(job.outputDir, `tt_${e.id}`);
    const file = await ytdlpDownload(e.url, outBase);
    recordVideo({
      index,
      platform: "tiktok",
      id: e.id,
      title: e.title || e.desc || "",
      author: e.author || "",
      url: e.url,
      likes: e.likes ?? 0,
      plays: e.plays ?? 0,
      comments: e.comments ?? 0,
      file,
    });
  }
  writeProgress({ state: "done", downloaded: index, message: "采集完成" });
}

async function collectInstagram() {
  // 刷 Reels 推荐播放器：读取当前 URL -> 方向键下一条，收集候选短码。
  const candidates = new Map();
  const need = job.count * 2;
  await runOpencli(["browser", session, "open", "https://www.instagram.com/reels/"]);
  for (let i = 0; i < 80 && candidates.size < need; i += 1) {
    if (markerExists(job.cancelFile)) {
      writeProgress({ state: "cancelled", message: "用户取消" });
      process.exit(2);
    }
    const url = await runOpencliRaw(["browser", session, "get", "url"]).catch(() => "");
    const m = /\/reels\/([A-Za-z0-9_-]+)\/?/.exec(String(url));
    if (m && !candidates.has(m[1])) {
      candidates.set(m[1], `https://www.instagram.com/reel/${m[1]}/`);
    }
    await runOpencliRaw(["browser", session, "keys", "ArrowDown"]).catch(() => {});
    await new Promise((r) => setTimeout(r, 2200));
  }
  await runOpencli(["browser", session, "close"]).catch(() => {});

  let index = 0;
  for (const url of candidates.values()) {
    if (markerExists(job.cancelFile)) {
      writeProgress({ state: "cancelled", message: "用户取消" });
      process.exit(2);
    }
    if (index >= job.count) break;
    let meta;
    try {
      meta = await ytdlpMetadata(url);
    } catch {
      continue; // 元数据解析失败跳过（可能已删除/私密）
    }
    if ((meta.like_count ?? 0) > job.maxLikes) continue;
    index += 1;
    const outBase = path.join(job.outputDir, `ig_${meta.id ?? Date.now()}`);
    const file = await ytdlpDownload(url, outBase);
    recordVideo({
      index,
      platform: "instagram",
      id: meta.id ?? "",
      title: meta.title || meta.description || "",
      author: meta.uploader || "",
      url,
      likes: meta.like_count ?? 0,
      plays: meta.view_count ?? 0,
      comments: 0,
      file,
    });
  }
  writeProgress({ state: "done", downloaded: index, message: "采集完成" });
}

async function main() {
  writeProgress({ state: "running", message: `开始采集 ${job.platform}` });
  if (job.platform === "tiktok") {
    await collectTiktok();
  } else if (job.platform === "instagram") {
    await collectInstagram();
  } else {
    throw new Error(`unknown platform: ${job.platform}`);
  }
}

main().catch((err) => {
  if (markerExists(job.cancelFile)) {
    writeProgress({ state: "cancelled", message: "用户取消" });
    process.exit(2);
  }
  writeProgress({ state: "failed", message: err.message || String(err) });
  console.error("[collector] failed:", err);
  process.exit(1);
});
