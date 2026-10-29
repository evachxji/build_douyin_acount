// 短视频采集 sidecar（opencli + yt-dlp 编排）：
// - TikTok：opencli tiktok explore（推荐流，返回 URL/点赞/标题等）-> 点赞过滤 -> yt-dlp 下载
// - Instagram：opencli 打开 /reels/ -> 页面内 fetch 推荐流 API 分页拿候选（自带 CDN 直链）-> 赞数过滤 -> 直链下载
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
const USER_HOME = process.env.USERPROFILE ?? "";
const OPENCLI_ENTRIES = [
  path.join(USER_HOME, "AppData/Local/nvm/v24.14.0/node_modules/@jackwener/opencli/dist/src/main.js"),
  "C:/nvm4w/nodejs/node_modules/@jackwener/opencli/dist/src/main.js",
  path.join(USER_HOME, "AppData/Roaming/npm/node_modules/@jackwener/opencli/dist/src/main.js"),
];

// 后台运行：opencli browser 命令默认 foreground（开页即把 Chrome 抢上前台），
// OPENCLI_WINDOW=background 让 open/get url/eval 全部不抢焦点（子进程继承该环境变量）。
process.env.OPENCLI_WINDOW = "background";
// 会话租约保活 7 天（默认 10 分钟）：任务结束驻留 about:blank 标签页保住容器窗口，
// 否则关最后一个标签页会连窗口一起关掉，下次任务扩展被迫新建窗口闪到前台。
process.env.OPENCLI_BROWSER_IDLE_TIMEOUT = "604800";

// 稳定会话名（不带 taskId）：下次任务 open 复用同一驻留标签页导航，不累积标签页。
const session = `collect-${job.platform}`;

// 损耗留痕：stats 计数随每次 writeProgress 合入 progress.json（任务完成时由 Agent 带进结果，
// 任务面板可一眼看出损耗在哪段）；skipped.jsonl 逐条记录跳过明细（stage/reason/id/url），
// 任务运行期间可在 collect 目录查看，Done 后随临时目录清理。
const stats = { downloaded: 0 };

function bump(key, n = 1) {
  stats[key] = (stats[key] ?? 0) + n;
}

function recordSkip(entry) {
  if (!job.skippedFile) return;
  try {
    fs.appendFileSync(job.skippedFile, JSON.stringify(entry) + "\n");
  } catch {}
}

function writeProgress(patch) {
  const base = { state: "running", downloaded: 0, target: job.count, message: "" };
  const prev = fs.existsSync(job.progressFile)
    ? JSON.parse(fs.readFileSync(job.progressFile, "utf8"))
    : base;
  fs.writeFileSync(
    job.progressFile,
    JSON.stringify({ ...prev, ...patch, stats: { ...stats } }, null, 2),
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

/** 枚举当前 Chrome 顶层窗口句柄（判断 open/explore 是否新建了窗口）。 */
function chromeWindowHandles() {
  return new Promise((resolve) => {
    execFile(
      "powershell",
      [
        "-NoProfile",
        "-Command",
        "(Get-Process chrome -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowHandle -ne 0 } | ForEach-Object { $_.MainWindowHandle.ToInt64() }) -join ','",
      ],
      { timeout: 15000 },
      (err, stdout) => {
        const set = new Set();
        if (!err) {
          String(stdout).trim().split(",").filter(Boolean).forEach((h) => set.add(h));
        }
        resolve(set);
      },
    );
  });
}

/** opencli 后台模式只保证不抢焦点；容器窗口不存在时扩展仍会新建可见窗口闪到屏幕上。
 *  对比调用前后的窗口句柄，把新建窗口最小化（已存在的用户窗口绝不动）。 */
async function minimizeNewChromeWindows(before) {
  // 新窗口的 MainWindowHandle 注册略有延迟，多探几轮；没等到新窗口就放弃（正常路径）。
  for (let i = 0; i < 5; i += 1) {
    const after = await chromeWindowHandles();
    const fresh = [...after].filter((h) => !before.has(h));
    if (fresh.length > 0) {
      const calls = fresh.map((h) => `[void][U32]::ShowWindowAsync([IntPtr]${h}, 6)`).join("; ");
      await new Promise((resolve) => {
        execFile(
          "powershell",
          [
            "-NoProfile",
            "-Command",
            `Add-Type -TypeDefinition 'using System;using System.Runtime.InteropServices;public class U32{[DllImport("user32.dll")]public static extern bool ShowWindowAsync(IntPtr h,int c);}'; ${calls}`,
          ],
          { timeout: 15000 },
          () => resolve(),
        );
      });
      return;
    }
    await new Promise((r) => setTimeout(r, 300));
  }
}

/** 任务结束不关标签页：驻留 about:blank 保住容器窗口——关最后一个标签页会连窗口一起关掉，
 *  下次任务扩展被迫新建窗口闪到前台。租约靠 OPENCLI_BROWSER_IDLE_TIMEOUT 保活，
 *  下次 open 复用同一标签页导航（实测 page id 不变，不累积标签页）。 */
async function parkSession() {
  const url = await runOpencliRaw(["browser", session, "get", "url"]).catch(() => "");
  // 标签页已被用户关闭：get url 已为死会话重建 about:blank 幽灵页，它就是驻留页，无需再动。
  if (!String(url).trim() || String(url).trim() === "about:blank") return;
  await runOpencliRaw(["browser", session, "eval", "location.replace('about:blank'), 'parked'"]).catch(() => {});
}

// TikTok 反爬挑战 workaround（yt-dlp issue #17403，2026-08-11 起全量失败
// "Unexpected response from webpage request"）：yt-dlp 默认 UA 的 Chrome 版本
// 过旧被挑战页拒绝，换成近期 Chrome UA 即可通过；上游修复后可移除。
const TIKTOK_UA =
  "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36";

async function ytdlpDownload(url, outBase) {
  const args = [
    "--no-playlist",
    "-f",
    "mp4/best",
    "--socket-timeout",
    "30",
    "--retries",
    "4",
    "--user-agent",
    TIKTOK_UA,
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
// 打开 reels 页借登录态 -> 页面内 fetch 推荐流 API /api/v1/clips/discover/ 分页拿候选
// （每条自带 CDN 直链 video_versions、赞数、文案、作者，免刷流免逐条开单页解析）-> 直链下载。
// 曾用「方向键切 reel 读地址栏」刷流：CDP 派发按键到无焦点页面 97% 不生效（2026-08-21 实测
// stale_rounds 117/120），合成 click/scrollBy 同样被 IG 忽略，故改为纯 API 方案。
const IG_GRAPHQL_APP_ID = "936619743392459";

/** 推荐流分页：max_id 游标 + seen_reels（已见 pk 列表，服务端据此前推新内容）。 */
function buildClipsDiscoverScript(maxId, seenPks) {
  return `
    (async () => {
      const csrf = (document.cookie.match(/csrftoken=([^;]+)/) || [])[1] || '';
      const params = new URLSearchParams();
      const maxId = ${JSON.stringify(maxId)};
      const seen = ${JSON.stringify(seenPks)};
      if (maxId) params.set('max_id', maxId);
      if (seen.length) params.set('seen_reels', seen.join(','));
      const res = await fetch('/api/v1/clips/discover/', {
        method: 'POST',
        credentials: 'include',
        headers: {
          'X-IG-App-ID': ${JSON.stringify(IG_GRAPHQL_APP_ID)},
          'X-CSRFToken': csrf,
          'X-Requested-With': 'XMLHttpRequest',
          'Content-Type': 'application/x-www-form-urlencoded',
        },
        body: params.toString(),
      });
      const data = await res.json().catch(() => null);
      if (!res.ok || !data?.items) return { ok: false, status: res.status, reason: 'no-items' };
      const items = [];
      for (const it of data.items) {
        const m = it.media || it;
        const vv = (m.video_versions || []).filter((v) => v.url);
        if (!m.code || !vv.length) continue; // 广告/图文等非视频条目
        vv.sort((a, b) => (b.width || 0) - (a.width || 0));
        items.push({
          pk: String(m.pk),
          code: m.code,
          videoUrl: vv[0].url,
          likeCount: m.like_count ?? null,
          title: m.caption?.text ?? '',
          author: m.user?.username ?? '',
        });
      }
      return { ok: true, items, nextMaxId: data.paging_info?.max_id || '' };
    })()
  `;
}

function recordVideo(v) {
  fs.appendFileSync(job.videosFile, JSON.stringify(v) + "\n");
  stats.downloaded = v.index;
  writeProgress({
    downloaded: v.index,
    message: `已下载 ${v.index}/${job.count}: ${v.title || v.url}`,
  });
}

async function collectTiktok() {
  // 拉取 3 倍候选量，过滤后仍能凑够 count。
  const limit = Math.min(120, job.count * 3);
  // explore 走 automation 容器：容器窗口不存在时同样会新建可见窗口，前后对比句柄最小化。
  const beforeHandles = await chromeWindowHandles();
  const entries = await runOpencli(["tiktok", "explore", "--limit", String(limit), "-f", "json"]);
  await minimizeNewChromeWindows(beforeHandles);
  bump("feed_entries", (entries ?? []).length);
  const seen = new Set();
  const candidates = (entries ?? []).filter((e) => {
    if (seen.has(e.id) || !e.url) return false;
    seen.add(e.id);
    if (e.likes > job.maxLikes) {
      bump("filtered_likes");
      recordSkip({ stage: "filter", reason: "likes-over-limit", id: e.id, url: e.url, likes: e.likes });
      return false;
    }
    return true;
  });
  stats.candidates = candidates.length;

  let index = 0;
  for (const e of candidates) {
    if (markerExists(job.cancelFile)) {
      writeProgress({ state: "cancelled", message: "用户取消" });
      process.exit(2);
    }
    if (index >= job.count) break;
    const outBase = path.join(job.outputDir, `tt_${e.id}`);
    let file;
    try {
      file = await ytdlpDownload(e.url, outBase);
    } catch (err) {
      // 单条下载失败只跳过，不中断整轮采集。
      bump("download_failed");
      recordSkip({
        stage: "download",
        reason: "download-failed",
        id: e.id,
        url: e.url,
        detail: String(err?.message || err).slice(0, 300),
      });
      writeProgress({ message: `下载失败跳过: ${e.url}` });
      continue;
    }
    index += 1;
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
  // 阶段1：推荐流 API 分页收集候选（shortcode + CDN 直链 + 赞数/文案/作者一次到位）。
  const candidates = new Map(); // shortcode -> { id, url, videoUrl, likeCount, title, author }
  const seenPks = [];
  const seenPkSet = new Set();
  const need = job.count * 2;
  const beforeHandles = await chromeWindowHandles();
  await runOpencli(["browser", session, "open", "https://www.instagram.com/reels/"]);
  await minimizeNewChromeWindows(beforeHandles);
  let maxId = "";
  let dryRounds = 0;
  let rounds = 0;
  for (let round = 0; round < 40 && candidates.size < need; round += 1) {
    rounds += 1;
    if (markerExists(job.cancelFile)) {
      writeProgress({ state: "cancelled", message: "用户取消" });
      process.exit(2);
    }
    const url = await runOpencliRaw(["browser", session, "get", "url"]).catch(() => "");
    // 用户关闭了采集标签页：get url 已为死会话创建 about:blank 幽灵页（即驻留页），按取消退出。
    if (String(url).trim() === "about:blank") {
      await parkSession();
      writeProgress({ state: "cancelled", message: "浏览器标签页已关闭，采集中止" });
      process.exit(2);
    }
    const feed = await runOpencli([
      "browser",
      session,
      "eval",
      buildClipsDiscoverScript(maxId, seenPks),
    ]).catch((e) => ({ ok: false, reason: String(e?.message || e).slice(0, 200) }));
    if (!feed?.ok) {
      bump("fetch_failed");
      recordSkip({ stage: "feed", reason: feed?.reason ?? "eval-error", status: feed?.status });
      dryRounds += 1;
      if (dryRounds >= 5) break; // 连续失败（未登录/限流/API 改版）：止损
      await new Promise((r) => setTimeout(r, 2000));
      continue;
    }
    let fresh = 0;
    for (const m of feed.items) {
      if (!seenPkSet.has(m.pk)) {
        seenPkSet.add(m.pk);
        seenPks.push(m.pk);
      }
      if (candidates.has(m.code)) continue;
      candidates.set(m.code, {
        id: m.code,
        url: `https://www.instagram.com/reels/${m.code}/`,
        videoUrl: m.videoUrl,
        likeCount: m.likeCount,
        title: m.title,
        author: m.author,
      });
      fresh += 1;
    }
    dryRounds = fresh === 0 ? dryRounds + 1 : 0;
    if (dryRounds >= 5) break; // 服务端开始重复：止损
    if (feed.nextMaxId) maxId = feed.nextMaxId;
    writeProgress({ message: `推荐流收集候选 ${candidates.size}/${need}` });
    await new Promise((r) => setTimeout(r, 1200));
  }
  stats.rounds = rounds;
  stats.candidates = candidates.size;
  await parkSession();

  // 阶段2：赞数过滤 + 直链下载。
  writeProgress({ message: "候选收集完成，开始下载" });
  let index = 0;
  for (const cand of candidates.values()) {
    if (markerExists(job.cancelFile)) {
      writeProgress({ state: "cancelled", message: "用户取消" });
      process.exit(2);
    }
    if (index >= job.count) break;
    if (cand.likeCount != null && cand.likeCount > job.maxLikes) {
      bump("filtered_likes");
      recordSkip({
        stage: "filter",
        reason: "likes-over-limit",
        id: cand.id,
        url: cand.url,
        likes: cand.likeCount,
      });
      continue;
    }
    const outBase = path.join(job.outputDir, `ig_${cand.id}`);
    let file;
    try {
      file = await ytdlpDownloadDirect(cand.videoUrl, outBase);
    } catch (e) {
      // 单条 CDN 直链下载失败（过期/403/限流常见）只跳过，不中断整轮采集。
      bump("download_failed");
      recordSkip({
        stage: "download",
        reason: "download-failed",
        id: cand.id,
        url: cand.url,
        detail: String(e?.message || e).slice(0, 300),
      });
      writeProgress({ message: `下载失败跳过: ${cand.url}` });
      continue;
    }
    index += 1;
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
  // 心跳：Rust agent 靠 progress.json 的 mtime 判断 sidecar 是否存活
  // （应用重启会带走 sidecar 进程，running 状态不变 → 僵尸任务）。
  const heartbeat = setInterval(() => {
    try {
      const now = new Date();
      fs.utimesSync(job.progressFile, now, now);
    } catch {}
  }, 5000);
  heartbeat.unref();
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
