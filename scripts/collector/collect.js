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

/** 直接下载已签名 CDN 直链（无需 cookie/鉴权），保留超时与重试。 */
async function ytdlpDownloadDirect(url, outBase) {
  const args = [
    "--no-playlist",
    "--socket-timeout",
    "30",
    "--retries",
    "4",
    "-o",
    `${outBase}.%(ext)s`,
    url,
  ];
  await runCmd(job.ytdlpPath, args, 900_000);
  const files = fs.readdirSync(job.outputDir).filter((f) => f.startsWith(path.basename(outBase)));
  if (files.length === 0) throw new Error(`下载后未找到文件: ${outBase}`);
  return path.join(job.outputDir, files[0]);
}

// Instagram 直链方案（规避 Chrome app-bound 加密导致 cookie 无法导出）：
// 播放器阶段收集短码 -> 逐条打开单 reels 页拿 media_id -> 页面内 info API 取 CDN 直链与元数据 -> 直链下载。
const IG_GRAPHQL_APP_ID = "936619743392459";

/** 单 reels 页：al:ios:url meta 拿 media_id -> info API 取直链/赞数/文案/作者。 */
function buildReelInfoScript(shortcode) {
  return `
    (async () => {
      let al = "";
      const deadline = Date.now() + 8000;
      while (Date.now() < deadline) {
        [...document.querySelectorAll('meta')].forEach((mt) => {
          if (mt.getAttribute('property') === 'al:ios:url') al = mt.getAttribute('content') || "";
        });
        if (al) break;
        await new Promise((r) => setTimeout(r, 500));
      }
      const m = /id=(\\d+)/.exec(al || "");
      const id = m ? m[1] : "";
      if (!id) return { ok: false, reason: 'no-media-id' };
      const res = await fetch('https://www.instagram.com/api/v1/media/' + id + '/info/', {
        credentials: 'include',
        headers: { 'X-IG-App-ID': ${JSON.stringify(IG_GRAPHQL_APP_ID)} },
      });
      const raw = await res.text();
      let data = null;
      try { data = raw ? JSON.parse(raw) : null; } catch { return { ok: false, reason: 'bad-json' }; }
      const it = data?.items?.[0];
      if (!res.ok || !it) return { ok: false, reason: 'no-item', status: res.status };
      const vv = (it.video_versions || []).filter((v) => v.url);
      if (!vv.length) return { ok: false, reason: 'not-video' };
      vv.sort((a, b) => (b.width || 0) - (a.width || 0));
      return {
        ok: true,
        videoUrl: vv[0].url,
        likeCount: it.like_count ?? null,
        title: it.caption?.text ?? "",
        author: it.user?.username ?? "",
      };
    })()
  `;
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
  // 阶段1：刷 Reels 推荐播放器收集候选短码（地址栏 URL 随播放推进更新）。
  const candidates = new Map(); // shortcode -> { id, url, videoUrl, likeCount, title, author }
  const need = job.count * 2;
  await runOpencli(["browser", session, "open", "https://www.instagram.com/reels/"]);
  for (let i = 0; i < 120 && candidates.size < need; i += 1) {
    if (markerExists(job.cancelFile)) {
      writeProgress({ state: "cancelled", message: "用户取消" });
      process.exit(2);
    }
    const url = await runOpencliRaw(["browser", session, "get", "url"]).catch(() => "");
    const clean = String(url).replace(/[?#].*$/, "");
    const m = /\/reels\/([A-Za-z0-9_-]+)\/?$/.exec(clean);
    if (m && !candidates.has(m[1])) {
      candidates.set(m[1], { id: m[1], url: `https://www.instagram.com/reels/${m[1]}/` });
    }
    await runOpencliRaw(["browser", session, "keys", "ArrowDown"]).catch(() => {});
    await new Promise((r) => setTimeout(r, 2600));
  }

  // 阶段2：逐条打开单 reels 页，页面内 info API 拿 CDN 直链与元数据。
  for (const cand of candidates.values()) {
    if (markerExists(job.cancelFile)) {
      writeProgress({ state: "cancelled", message: "用户取消" });
      process.exit(2);
    }
    await runOpencli(["browser", session, "open", cand.url]).catch(() => {});
    await new Promise((r) => setTimeout(r, 1500));
    let info = { ok: false };
    for (let attempt = 0; attempt < 2 && !info?.ok; attempt += 1) {
      info = await runOpencli(["browser", session, "eval", buildReelInfoScript(cand.id)]).catch(
        () => ({ ok: false }),
      );
      if (!info?.ok) await new Promise((r) => setTimeout(r, 2000));
    }
    cand.videoUrl = info?.ok ? (info.videoUrl || "") : "";
    cand.likeCount = info?.likeCount ?? null;
    cand.title = info?.title || "";
    cand.author = info?.author || "";
  }
  await runOpencli(["browser", session, "close"]).catch(() => {});

  // 阶段3：赞数过滤 + 直链下载。
  let index = 0;
  for (const cand of candidates.values()) {
    if (markerExists(job.cancelFile)) {
      writeProgress({ state: "cancelled", message: "用户取消" });
      process.exit(2);
    }
    if (index >= job.count) break;
    if (!cand.videoUrl) {
      writeProgress({ message: `跳过（无直链）: ${cand.url}` });
      continue;
    }
    if (cand.likeCount != null && cand.likeCount > job.maxLikes) continue;
    index += 1;
    const outBase = path.join(job.outputDir, `ig_${cand.id}`);
    const file = await ytdlpDownloadDirect(cand.videoUrl, outBase);
    recordVideo({
      index,
      platform: "instagram",
      id: cand.id,
      title: cand.title,
      author: cand.author,
      url: cand.url,
      likes: cand.likeCount ?? 0,
      plays: 0,
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
