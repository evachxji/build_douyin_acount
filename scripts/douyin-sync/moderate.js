// 抖音违规作品处理 sidecar（opencli 桥接已登录 Chrome，页面内 fetch 写接口）：
// 1) 将违规作品权限改为「仅自己可见」（POST /web/api/media/aweme/update/，表单
//    item_id&download=1&visibility_type=1——2026-08 实测自管理页「设置权限」弹窗抓包）；
// 2) 若该作品发布日期为今天，且仍有定时发布作品，则将最晚一条定时作品改为立即发布
//    （POST /web/api/media/update/timer/，表单 item_id&timing=<now+10s>——定时弹窗的
//    「立即发布」走同一接口，时间范围「支持当前到未来14天」）。前提：该违规作品的
//    权限修改已返回成功（status_code=0），否则跳过本次提升。
// 进度：progressFile 写状态与计数（5s 心跳）；结果：resultsFile（JSONL）逐条追加。
//
// 用法：node moderate.js <job.json>
// job 字段：taskId / progressFile / resultsFile / cancelFile /
//           targets([{aweme_id, create_time}]) / promoteQueue([{aweme_id, create_time}] 按时间倒序)

import { execFile } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

const job = JSON.parse(fs.readFileSync(process.argv[2], "utf8"));
fs.mkdirSync(path.dirname(job.progressFile), { recursive: true });

// opencli CLI 入口候选（nvm 多版本安装下的探测）。
const USER_HOME = process.env.USERPROFILE ?? "";
const OPENCLI_ENTRIES = [
  path.join(USER_HOME, "AppData/Local/nvm/v24.14.0/node_modules/@jackwener/opencli/dist/src/main.js"),
  "C:/nvm4w/nodejs/node_modules/@jackwener/opencli/dist/src/main.js",
  path.join(USER_HOME, "AppData/Roaming/npm/node_modules/@jackwener/opencli/dist/src/main.js"),
];

// 后台运行：opencli browser 命令默认 foreground（开页即把 Chrome 抢上前台），
// OPENCLI_WINDOW=background 让 open/eval 全部不抢焦点（子进程继承该环境变量）。
process.env.OPENCLI_WINDOW = "background";
// 会话租约保活 7 天（默认 10 分钟）：任务结束驻留 about:blank 标签页保住容器窗口，
// 否则关最后一个标签页会连窗口一起关掉，下次任务扩展被迫新建窗口闪到前台。
process.env.OPENCLI_BROWSER_IDLE_TIMEOUT = "604800";

// 稳定会话名（不带 taskId）：下次任务 open 复用同一驻留标签页导航，不累积标签页。
const session = "dy-mod";
const MANAGE_URL = "https://creator.douyin.com/creator-micro/content/manage";

const stats = { privated: 0, published: 0, failed: 0 };

function writeProgress(patch) {
  const base = { state: "running", done: 0, total: (job.targets || []).length, message: "" };
  const prev = fs.existsSync(job.progressFile)
    ? JSON.parse(fs.readFileSync(job.progressFile, "utf8"))
    : base;
  fs.writeFileSync(
    job.progressFile,
    JSON.stringify({ ...prev, ...patch, stats: { ...stats } }, null, 2),
  );
  console.log(`[douyin-moderate] ${patch.state ?? "running"} ${patch.message ?? ""}`);
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

/** 运行 opencli 并返回原始文本（`get url` 及 eval 字符串结果等非 JSON 输出）。 */
function runOpencliRaw(args, timeoutMs = 120_000) {
  return new Promise((resolve, reject) => {
    const entry = OPENCLI_ENTRIES.find((p) => fs.existsSync(p));
    if (!entry) {
      reject(new Error("未找到 opencli CLI 入口"));
      return;
    }
    execFile(
      "node",
      [entry, ...args],
      { timeout: timeoutMs, maxBuffer: 4 * 1024 * 1024 },
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

/** 页面内 POST 表单写接口（改权限/改定时），返回 {ok, code, msg}。 */
function buildPostFormScript(url, form) {
  return `
    (async () => {
      try {
        const res = await fetch(${JSON.stringify(url)}, {
          method: 'POST',
          credentials: 'include',
          headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
          body: ${JSON.stringify(form)},
        });
        const j = await res.json().catch(() => null);
        if (!j) return { ok: false, status: res.status };
        return { ok: res.ok && (j.status_code === 0 || j.status === 'ok' || j.code === 0),
                 code: j.status_code, msg: j.status_msg || j.message || '' };
      } catch (e) {
        return { ok: false, error: String(e && e.message || e) };
      }
    })()
  `;
}

function recordResult(entry) {
  fs.appendFileSync(job.resultsFile, JSON.stringify(entry) + "\n");
}

/** 本地日期（YYYY-MM-DD），与 Chrome/用户同一时区。 */
function localDay(epochSec) {
  const d = new Date(epochSec * 1000);
  return `${d.getFullYear()}-${d.getMonth()}-${d.getDate()}`;
}

async function main() {
  writeProgress({ state: "running", message: "打开创作者中心" });
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
  await new Promise((r) => setTimeout(r, 3000));

  const targets = job.targets || [];
  const promoteQueue = [...(job.promoteQueue || [])]; // 已按发布时间倒序（最晚在前）
  const today = localDay(Math.floor(Date.now() / 1000));

  for (let i = 0; i < targets.length; i += 1) {
    const t = targets[i];
    if (markerExists(job.cancelFile)) {
      writeProgress({ state: "cancelled", message: "用户取消" });
      return "cancelled";
    }
    writeProgress({ done: i, message: `设置仅自己可见 ${i + 1}/${targets.length}` });

    // 步骤1：违规作品 -> 仅自己可见。
    const priv = await runOpencli([
      "browser",
      session,
      "eval",
      buildPostFormScript(
        "https://creator.douyin.com/web/api/media/aweme/update/?aid=1128",
        `item_id=${t.aweme_id}&download=1&visibility_type=1&xg_user_id=0&dx_upgraded=0`,
      ),
    ]).catch((e) => ({ ok: false, msg: String(e?.message || e).slice(0, 200) }));
    if (!priv?.ok) {
      stats.failed += 1;
      recordResult({
        aweme_id: t.aweme_id,
        action: "set_private",
        ok: false,
        detail: priv?.msg || priv?.error || `code ${priv?.code ?? "?"}`,
      });
      writeProgress({ message: `权限修改失败：${t.aweme_id}` });
      continue; // 权限未确认修改，绝不提升定时作品
    }
    stats.privated += 1;
    recordResult({ aweme_id: t.aweme_id, action: "set_private", ok: true });

    // 步骤2：该作品发布日期为今天 -> 提升最晚一条定时作品为立即发布。
    if (localDay(t.create_time) === today && promoteQueue.length > 0) {
      const next = promoteQueue.shift();
      const timing = Math.floor(Date.now() / 1000) + 10;
      writeProgress({ message: `将最晚定时作品改为立即发布` });
      const pub = await runOpencli([
        "browser",
        session,
        "eval",
        buildPostFormScript(
          "https://creator.douyin.com/web/api/media/update/timer/?aid=1128",
          `item_id=${next.aweme_id}&timing=${timing}`,
        ),
      ]).catch((e) => ({ ok: false, msg: String(e?.message || e).slice(0, 200) }));
      if (pub?.ok) {
        stats.published += 1;
        recordResult({ aweme_id: next.aweme_id, action: "publish_now", ok: true });
      } else {
        stats.failed += 1;
        recordResult({
          aweme_id: next.aweme_id,
          action: "publish_now",
          ok: false,
          detail: pub?.msg || pub?.error || `code ${pub?.code ?? "?"}`,
        });
      }
      await new Promise((r) => setTimeout(r, 400));
    }
    await new Promise((r) => setTimeout(r, 300)); // 礼貌限速
  }

  writeProgress({
    state: "done",
    done: targets.length,
    message: `处理完成：设私密 ${stats.privated}，立即发布 ${stats.published}，失败 ${stats.failed}`,
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
      console.error("[douyin-moderate] failed:", err);
      code = 1;
    }
  } finally {
    await parkSession();
  }
  process.exitCode = code;
})();
