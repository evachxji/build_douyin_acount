// 抖音作品数据同步 sidecar（opencli 桥接已登录 Chrome）：
// 打开创作者中心内容管理页借登录态 -> 页面内 fetch 作品列表 API（janus work_list，
// max_cursor 游标翻页，cookie 与 a_bogus 签名由浏览器上下文自动处理）-> 逐条下载封面
// 到本地 -> posts.jsonl 逐条追加。进度：progressFile 写状态与计数（5s 心跳 touch mtime）。
//
// 用法：node sync.js <job.json>
// job 字段：taskId / progressFile / postsFile / cancelFile / coverDir / maxPages

import { execFile } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

const job = JSON.parse(fs.readFileSync(process.argv[2], "utf8"));
fs.mkdirSync(path.dirname(job.progressFile), { recursive: true });
fs.mkdirSync(job.coverDir, { recursive: true });

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
const session = "dy-sync";
const MANAGE_URL = "https://creator.douyin.com/creator-micro/content/manage";
const MAX_PAGES = job.maxPages ?? 50;

// 与浏览器一致的 UA/Referer，封面 CDN 校验 Referer（2026-08 实测直连可下载）。
const CHROME_UA =
  "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36";

const stats = { synced: 0, scheduled: 0, cover_failed: 0 };

function writeProgress(patch) {
  const base = { state: "running", page: 0, synced: 0, scheduled: 0, message: "" };
  const prev = fs.existsSync(job.progressFile)
    ? JSON.parse(fs.readFileSync(job.progressFile, "utf8"))
    : base;
  fs.writeFileSync(
    job.progressFile,
    JSON.stringify({ ...prev, ...patch, stats: { ...stats } }, null, 2),
  );
  console.log(`[douyin-sync] ${patch.state ?? "running"} ${patch.message ?? ""}`);
}

function markerExists(file) {
  return fs.existsSync(file);
}

function runOpencli(args, timeoutMs = 120_000) {
  return new Promise((resolve, reject) => {
    const entry = OPENCLI_ENTRIES.find((p) => fs.existsSync(p));
    if (!entry) {
      reject(new Error("未找到 opencli CLI 入口"));
      return;
    }
    execFile(
      "node",
      [entry, ...args],
      { timeout: timeoutMs, maxBuffer: 16 * 1024 * 1024 },
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

/** 运行 opencli 并返回原始文本（`get url` 等非 JSON 输出）。 */
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

/** 页面内 fetch 一页作品列表（2026-08 实测：只认 max_cursor 游标；page_size/page_num 被忽略。
 *  必须带 scene/device_platform/aid 全参数——裸参数响应不含定时发布作品。
 *  响应 aweme_list = 定时+已发布合并（定时作品的 create_time 即定时发布时间，public_time=0）；
 *  items = 本页已发布子集（按 create_time 一对一 join），其 metrics 含
 *  completion_rate / bounce_rate_2s（0-1 小数字符串）——率类指标的唯一来源。 */
function buildFetchPageScript(maxCursor) {
  return `
    (async () => {
      const base = 'https://creator.douyin.com/janus/douyin/creator/pc/work_list?status=0&count=12&scene=star_atlas&device_platform=android&aid=1128';
      const mc = ${JSON.stringify(maxCursor || 0)};
      const url = mc ? base + '&max_cursor=' + mc : base + '&max_cursor=0';
      try {
        const res = await fetch(url, { credentials: 'include' });
        const j = await res.json().catch(() => null);
        if (!res.ok || !j) return { ok: false, status: res.status };
        const list = j.aweme_list || j.work_list || [];
        return {
          ok: true,
          status: res.status,
          statusCode: j.status_code,
          hasMore: !!j.has_more,
          maxCursor: j.max_cursor || 0,
          items: list.map((v) => ({
            aweme_id: v.aweme_id,
            desc: v.desc || v.caption || '',
            cover_url: (v.Cover && v.Cover.url_list && v.Cover.url_list[0]) || '',
            create_time: v.create_time || 0,
            status: v.status || null,
            statistics: v.statistics || null,
          })),
          metrics: (j.items || []).map((it) => ({
            create_time: it.create_time || 0,
            completion_rate: it.metrics ? it.metrics.completion_rate : null,
            bounce_rate_2s: it.metrics ? it.metrics.bounce_rate_2s : null,
            // 违规标记（2026-08 实测：健康 review.status=2 且 details 空；流量减少 status=5）。
            review_status:
              it.review && typeof it.review.status === "number" ? it.review.status : null,
            violation_text:
              it.review && it.review.details && it.review.details[0]
                ? it.review.details[0].text || null
                : null,
            violation_url:
              it.review && it.review.details && it.review.details[0]
                ? it.review.details[0].url || null
                : null,
          })),
        };
      } catch (e) {
        return { ok: false, error: String(e && e.message || e) };
      }
    })()
  `;
}

/** 率类指标："0.239939"（0-1 字符串）-> 23.99；无效值 -> null。 */
function parseRate(v) {
  const n = Number(v);
  if (!Number.isFinite(n) || n <= 0) return null;
  return Math.round(n * 10000) / 100;
}

/** 原始列表项 -> posts.jsonl 行（与 Rust NewDouyinPost 同形）。
 *  metricsByCt：items 数组按 create_time 建的索引（已发布作品才有率类指标）。 */
function mapWork(it, metricsByCt) {
  const now = Math.floor(Date.now() / 1000);
  const st = it.status || {};
  const s = it.statistics || {};
  const ct = it.create_time || 0;
  // 定时作品：create_time 在未来（status_value=140 与私密作品共享，不能作判据）。
  const status = st.is_delete
    ? "deleted"
    : st.is_prohibited
      ? "prohibited"
      : st.in_reviewing
        ? "reviewing"
        : st.is_private
          ? "private"
          : ct > now
            ? "scheduled"
            : "published";
  const metrics = metricsByCt?.get(ct);
  return {
    aweme_id: String(it.aweme_id || ""),
    title: it.desc || "",
    cover_path: null, // 下载成功后回填
    cover_url: it.cover_url || null,
    status,
    play_count: s.play_count || 0,
    digg_count: s.digg_count || 0,
    comment_count: s.comment_count || 0,
    share_count: s.share_count || 0,
    completion_rate: parseRate(metrics?.completion_rate),
    bounce_rate_2s: parseRate(metrics?.bounce_rate_2s),
    // 违规标记：violation_text 非空即违规（流量减少等）；brief 由详情接口补抓。
    violation_status: metrics?.violation_text ? metrics.review_status : null,
    violation_text: metrics?.violation_text || null,
    violation_brief: null,
    violation_url: metrics?.violation_url || null,
    create_time: ct,
    public_time: 0,
  };
}

/** 违规详情接口：取「违规原因」brief（如 原创性不足）。fields 是 {label,type,value} 数组，
 *  value 多为 JSON 字符串；reason_detail / multi_reason_detail 的 [0].brief 即原因摘要。 */
function buildFetchViolationScript(awemeId) {
  return `
    (async () => {
      try {
        const url = 'https://creator.douyin.com/aweme/v1/review/result/?page_source=1&object_id=${awemeId}&item_appeal_id=0&music_appeal_id=0&scene=10019&from_message=0&lead_warn=0';
        const res = await fetch(url, { credentials: 'include' });
        const j = await res.json().catch(() => null);
        if (!res.ok || !j || j.status_code !== 0) return { ok: false, status: res.status };
        const fields = (j.review_detail && j.review_detail.fields) || [];
        for (const f of fields) {
          if (f.type !== 'reason_detail' && f.type !== 'multi_reason_detail') continue;
          try {
            const arr = JSON.parse(f.value || '[]');
            if (arr[0] && arr[0].brief) return { ok: true, brief: String(arr[0].brief) };
          } catch {}
        }
        return { ok: true, brief: null };
      } catch (e) {
        return { ok: false, error: String(e && e.message || e) };
      }
    })()
  `;
}

function coverExt(url) {
  const m = String(url).match(/\.(webp|jpeg|jpg|png)(\?|~|$)/i);
  return m ? m[1].toLowerCase().replace("jpeg", "jpg") : "jpg";
}

/** 下载封面到 coverDir/douyin_<aweme_id>.<ext>；已存在（任意扩展名）则跳过。 */
async function downloadCover(post) {
  if (!post.cover_url) return null;
  const existing = fs
    .readdirSync(job.coverDir)
    .find((f) => f.startsWith(`douyin_${post.aweme_id}.`));
  if (existing) return path.join(job.coverDir, existing);
  const dest = path.join(job.coverDir, `douyin_${post.aweme_id}.${coverExt(post.cover_url)}`);
  try {
    const res = await fetch(post.cover_url, {
      headers: { "User-Agent": CHROME_UA, Referer: "https://creator.douyin.com/" },
      signal: AbortSignal.timeout(15_000),
    });
    if (!res.ok) throw new Error(`http ${res.status}`);
    fs.writeFileSync(dest, Buffer.from(await res.arrayBuffer()));
    return dest;
  } catch (e) {
    stats.cover_failed += 1;
    console.warn(`[douyin-sync] 封面下载失败 ${post.aweme_id}: ${e?.message || e}`);
    return null;
  }
}

async function main() {
  writeProgress({ state: "running", message: "打开创作者中心" });
  // 心跳：Rust agent 靠 progress.json 的 mtime 判断 sidecar 是否存活
  // （应用重启会带走 sidecar 进程，running 状态不变 → 僵尸任务）。
  const heartbeat = setInterval(() => {
    try {
      const now = new Date();
      fs.utimesSync(job.progressFile, now, now);
    } catch {}
  }, 5000);
  heartbeat.unref();

  const beforeHandles = await chromeWindowHandles();
  await runOpencli(["browser", session, "open", MANAGE_URL]).catch((e) => {
    throw new Error(`无法连接 Chrome（请先打开 Chrome 并登录抖音创作者中心）: ${e.message}`);
  });
  await minimizeNewChromeWindows(beforeHandles);
  // SPA 加载与登录态跳转留出时间。
  await new Promise((r) => setTimeout(r, 3000));

  const seen = new Set();
  // 已有违规原因（brief）的作品跳过详情补抓（agent 从库里读出经 job 传入）；
  // 违规状态本身仍以本轮列表为准（库里已消解的旧违规会被 upsert 清掉）。
  const knownSet = new Set(job.knownViolations || []);
  let maxCursor = 0;
  let failRounds = 0;
  for (let page = 1; page <= MAX_PAGES; page += 1) {
    if (markerExists(job.cancelFile)) {
      writeProgress({ state: "cancelled", message: "用户取消" });
      return "cancelled";
    }
    // 用户关闭了同步标签页：get url 已为死会话创建 about:blank 幽灵页，按取消退出。
    const pageUrl = await runOpencliRaw(["browser", session, "get", "url"]).catch(() => "");
    if (String(pageUrl).trim() === "about:blank") {
      writeProgress({ state: "cancelled", message: "浏览器标签页已关闭，同步中止" });
      return "cancelled";
    }
    const feed = await runOpencli([
      "browser",
      session,
      "eval",
      buildFetchPageScript(maxCursor),
    ]).catch((e) => ({ ok: false, reason: String(e?.message || e).slice(0, 200) }));
    if (!feed?.ok) {
      failRounds += 1;
      if (failRounds >= 3) {
        throw new Error(
          `作品列表拉取失败（未登录抖音创作者中心或 Cookie 已过期）: ${feed?.reason ?? `http ${feed?.status ?? "?"}`}`,
        );
      }
      await new Promise((r) => setTimeout(r, 2000));
      continue;
    }
    failRounds = 0;
    if (feed.statusCode != null && feed.statusCode !== 0) {
      throw new Error("未登录抖音创作者中心或 Cookie 已过期，请先在 Chrome 中登录");
    }
    const metricsByCt = new Map((feed.metrics || []).map((m) => [m.create_time, m]));
    for (const it of feed.items) {
      const post = mapWork(it, metricsByCt);
      if (!post.aweme_id || seen.has(post.aweme_id)) continue;
      seen.add(post.aweme_id);
      // 违规作品补抓「违规原因」brief（每个违规一次详情请求；已有 brief 的跳过）。
      if (post.violation_text && !knownSet.has(post.aweme_id)) {
        const v = await runOpencli([
          "browser",
          session,
          "eval",
          buildFetchViolationScript(post.aweme_id),
        ]).catch(() => null);
        if (v?.ok && v.brief) post.violation_brief = v.brief;
        stats.violations = (stats.violations ?? 0) + 1;
        await new Promise((r) => setTimeout(r, 300)); // 礼貌限速
      } else if (post.violation_text) {
        stats.violations = (stats.violations ?? 0) + 1;
      }
      post.cover_path = await downloadCover(post);
      fs.appendFileSync(job.postsFile, JSON.stringify(post) + "\n");
      stats.synced += 1;
      if (post.status === "scheduled") stats.scheduled += 1;
    }
    writeProgress({
      page,
      synced: stats.synced,
      scheduled: stats.scheduled,
      message: `已同步 ${stats.synced} 条（第 ${page} 页）`,
    });
    // 终止：空页 / 无更多 / 游标不再前进（异常兜底）。
    if (feed.items.length === 0 || !feed.hasMore || !feed.maxCursor || feed.maxCursor === maxCursor) {
      break;
    }
    maxCursor = feed.maxCursor;
    await new Promise((r) => setTimeout(r, 400)); // 礼貌限速
  }

  writeProgress({
    state: "done",
    synced: stats.synced,
    scheduled: stats.scheduled,
    message: "同步完成",
  });
  return "done";
}

/** 枚举当前 Chrome 顶层窗口句柄（判断 open 是否新建了窗口）。 */
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
 *  对比 open 前后的窗口句柄，把新建窗口最小化（已存在的用户窗口绝不动）。 */
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

(async () => {
  let code = 0;
  try {
    if ((await main()) === "cancelled") code = 2;
  } catch (err) {
    if (markerExists(job.cancelFile)) {
      writeProgress({ state: "cancelled", message: "用户取消" });
      code = 2;
    } else {
      writeProgress({ state: "failed", message: err.message || String(err) });
      console.error("[douyin-sync] failed:", err);
      code = 1;
    }
  } finally {
    // 同步是后台操作：驻留 about:blank 保住容器窗口（不影响用户自己打开的标签页）。
    await parkSession();
  }
  process.exitCode = code;
})();
