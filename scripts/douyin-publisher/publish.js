// 抖音发布辅助 sidecar（opencli 编排版）：
// 1. 在用户已登录的 Chrome 中打开 creator.douyin.com 上传页（opencli 会话）。
// 2. 自动上传：sidecar 在 127.0.0.1 起一次性只读 HTTP 服务托管视频文件，
//    页面内 eval 脚本 fetch 字节 -> 构造 File -> DataTransfer 赋给视频 file input
//    -> dispatch change，上传组件照常接管。不弹原生文件选择框，无需人工粘贴路径。
//    （opencli upload 的 CDP DOM.setFileInputFiles 被 Chrome 扩展调试会话拒绝——
//    "Not allowed"；CDP 点击又无 user activation 开不了原生弹窗，故走内存注入。）
//    注入失败/超时（页面改版、元素缺失、大文件内存不足）时回退人工流程：
//    应用弹窗展示视频路径（自动复制），用户手动选择文件后程序继续。
// 3. 等待上传/转码完成，自动填写标题/文案/话题。
// 4. 【绝不点击最终发布按钮】——填表完成后保持窗口打开，等待用户本人核对并点击发布。
// 5. 仅当页面出现显式「发布成功」提示才上报 published；窗口关闭/取消如实上报。
//
// 用法：node publish.js <job.json>
// job.json 字段：draftId / videoPath / title / body / hashtags[] /
//                scheduleTime（"YYYY-MM-DD HH:mm" 本地时间或 null——agent 按发布日历
//                空档日 + 08:00~19:00 随机分钟算好；null 表示 14 天无空档，提示人工）/
//                progressFile / cancelFile / confirmFile

import { execFile } from "node:child_process";
import fs from "node:fs";
import http from "node:http";
import path from "node:path";

const SELECTORS = JSON.parse(
  fs.readFileSync(new URL("./selectors.json", import.meta.url), "utf8"),
);

const job = JSON.parse(fs.readFileSync(process.argv[2], "utf8"));
const logDir = path.dirname(job.progressFile);
fs.mkdirSync(logDir, { recursive: true });
const session = `dy-pub-${job.draftId}`;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// opencli CLI 入口候选（nvm 多版本安装下的探测）。
const OPENCLI_ENTRIES = [
  "C:/Users/<user>/AppData/Local/nvm/v24.14.0/node_modules/@jackwener/opencli/dist/src/main.js",
  "C:/nvm4w/nodejs/node_modules/@jackwener/opencli/dist/src/main.js",
  "C:/Users/<user>/AppData/Roaming/npm/node_modules/@jackwener/opencli/dist/src/main.js",
];

class CancelRequested extends Error {}
class TabClosed extends Error {}

// 最近一次进度：心跳定时器原样重写，保持 progress 文件 mtime 常新。
let lastProgress = null;

function writeProgress(state, message) {
  lastProgress = { state, message: message || "" };
  const payload = {
    state,
    message: message || "",
    updatedAt: new Date().toISOString(),
  };
  fs.writeFileSync(job.progressFile, JSON.stringify(payload, null, 2));
  console.log(`[publisher] ${state} ${message || ""}`);
}

// 心跳：每 5s 重写一次当前进度。agent 端按 progress 文件 mtime 判活——进程死亡
// （如随应用重启被杀）则 mtime 冻结，agent 据此判定僵尸并重新拉起 sidecar。
const heartbeat = setInterval(() => {
  if (!lastProgress) return;
  try {
    writeProgress(lastProgress.state, lastProgress.message);
  } catch {
    // 进度目录被清理等异常不影响主流程。
  }
}, 5000);
heartbeat.unref();

function markerExists(file) {
  return fs.existsSync(file);
}

/** 运行 opencli CLI 并解析 JSON 输出（无 shell，避免参数注入）。 */
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
      { timeout: 120_000, maxBuffer: 4 * 1024 * 1024 },
      (err, stdout) => {
        if (err) {
          reject(
            new Error(`opencli ${args.slice(0, 3).join(" ")} 失败: ${(stdout || "").trim() || err.message}`),
          );
          return;
        }
        try {
          resolve(JSON.parse(stdout));
        } catch {
          reject(new Error(`opencli 输出解析失败: ${stdout.slice(0, 300)}`));
        }
      },
    );
  });
}

/** 运行 opencli CLI 并返回原始文本输出（`get url` / `get title` 等返回纯文本而非 JSON）。 */
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

/** 运行 opencli fill：抖音编辑器会把 #话题 转成话题卡片，导致 verified=false 但
 *  内容实际已写入（fill 命令会非零退出）。此处不以退出码判断，只认 filled 字段。 */
function runOpencliFill(args) {
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
      (_err, stdout) => {
        try {
          resolve(JSON.parse(stdout));
        } catch {
          reject(new Error(`opencli fill 输出解析失败: ${(stdout || "").slice(0, 300)}`));
        }
      },
    );
  });
}

/** 打开上传页（在用户已登录的 Chrome 中新开标签页）。 */
async function openUploadPage() {
  const res = await runOpencli(["browser", session, "open", SELECTORS.uploadUrl]);
  if (!res?.url) {
    throw new Error(`打开上传页失败: ${JSON.stringify(res).slice(0, 200)}`);
  }
}

/** 关闭会话标签页。抖音页通过 addEventListener 注册 beforeunload（JS 清不掉），
 *  常规 close 会触发 Chrome「离开此网站？」确认框；且 close 释放租约后会话命令
 *  就打不到弹窗残留页了（keys Enter 会落到 daemon 新建的幽灵页）——已实测踩坑。
 *  故顺序必须：先 tab close 强关（chrome.tabs.remove 不触发 beforeunload 确认，
 *  已实测验证），再 close 释放租约。tab list 只返回本会话的页，不会误关用户标签页。 */
async function closeSession() {
  const tabs = await runOpencli(["browser", session, "tab", "list"]).catch(() => null);
  const list = Array.isArray(tabs) ? tabs : tabs?.tabs ?? [];
  for (const t of list) {
    if (t.page) {
      await runOpencli(["browser", session, "tab", "close", String(t.page)]).catch(() => {});
    }
  }
  await runOpencli(["browser", session, "close"]).catch(() => {});
}

/** 取消路径：释放会话租约，标签页保留给用户自行处置（不关闭、不弹离开确认）。 */
async function releaseSession() {
  await runOpencli(["browser", session, "unbind"]).catch(() => {});
}

/** 探测标签页是否仍存活（用户关闭标签页后会话失效）。
 *  用 tab list 而非 get url：daemon 会为无活页的会话自动创建 about:blank 幽灵页。 */
async function sessionAlive() {
  try {
    const res = await runOpencli(["browser", session, "tab", "list"]);
    const tabs = Array.isArray(res) ? res : res?.tabs ?? [];
    return tabs.some((t) => String(t.url || "").startsWith("http"));
  } catch {
    return false;
  }
}

/** 等待某个 CSS 选择器可见（opencli find 探测）；等待期间响应取消/标签关闭。 */
async function waitSelector(selector, timeoutMs, intervalMs = 3000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (markerExists(job.cancelFile)) throw new CancelRequested();
    if (!(await sessionAlive())) throw new TabClosed("浏览器标签页已关闭");
    try {
      const res = await runOpencli(["browser", session, "find", "--css", selector]);
      const entries = res?.entries ?? [];
      if (entries.some((e) => e.visible)) {
        return entries.find((e) => e.visible)?.ref ?? null;
      }
    } catch {
      /* 页面未就绪，继续轮询 */
    }
    await new Promise((r) => setTimeout(r, intervalMs));
  }
  return null;
}

/** 键入话题脚本：光标归尾后用 execCommand 插入文本（不点击编辑器——
 *  点击可能选中已有话题卡片，键入会把它替换掉）。 */
function buildTypeTagScript(tag) {
  return `(() => {
    const ed = document.querySelector('[contenteditable=true]');
    if (!ed) return false;
    ed.focus();
    const sel = window.getSelection();
    sel.selectAllChildren(ed);
    sel.collapseToEnd();
    return document.execCommand('insertText', false, ' #' + ${JSON.stringify(tag)});
  })()`;
}

/** 话题联想一站式脚本（异步，页面内完成全部等待，消除 opencli 往返开销）：
 *  等联想项出现(≤2.5s) -> 点选 -> 等卡片成型(≤2s)。
 *  返回 'carded' / 'no-suggestion' / 'clicked-no-card' / 'no-editor'。
 *  联想区是编辑器内联推荐面板（tag-hash-* 哈希类名），不依赖容器类名匹配；
 *  卡片成型标记是编辑器内的 [data-mention] 元素（文本含空格/nbsp，比较前一律剥掉）。 */
function buildTopicApplyScript(tag) {
  return `(async () => {
    const tag = ${JSON.stringify(tag)};
    const norm = (s) => (s || "").replace(/[\\s]/g, "").replace(/^[#＃]/, "");
    const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
    const ed = document.querySelector('[contenteditable=true]');
    if (!ed) return 'no-editor';
    const carded = () =>
      [...ed.querySelectorAll("[data-mention]")].some((m) => norm(m.textContent) === tag);
    const findItem = () => {
      const items = [];
      for (const el of document.querySelectorAll("body *")) {
        if (el.offsetParent === null) continue;
        if (ed.contains(el)) continue;
        if (norm(el.textContent) !== tag) continue;
        items.push(el);
      }
      if (!items.length) return null;
      // 优先联想区（类名含 tag-hash）内文本最短的（最内层）元素。
      items.sort((a, b) => {
        const ah = String(a.className || "").includes("tag-hash") ? 0 : 1;
        const bh = String(b.className || "").includes("tag-hash") ? 0 : 1;
        return ah - bh || a.textContent.length - b.textContent.length;
      });
      return items[0];
    };
    if (carded()) return 'carded';
    let item = null;
    for (let i = 0; i < 5 && !item; i += 1) {
      item = findItem();
      if (!item) await sleep(500);
    }
    if (!item) return carded() ? 'carded' : 'no-suggestion';
    item.dispatchEvent(new MouseEvent("mousedown", { bubbles: true }));
    item.dispatchEvent(new MouseEvent("mouseup", { bubbles: true }));
    item.click();
    for (let i = 0; i < 8; i += 1) {
      if (carded()) return 'carded';
      await sleep(250);
    }
    return 'clicked-no-card';
  })()`;
}

/** 自主声明一站式脚本（异步，页面内完成全部等待与点击）：
 *  点【自主声明】→【内容为转载】→【取材站外】→【确定】。
 *  返回 'declared' 或失败位置（'no-entry'/'no-repost'/'no-source'/'no-confirm'）。
 *  全部按可见文本精确匹配；识别不到元素即中止（不盲目操作），由进度消息提示人工处理。 */
function buildDeclarationScript(labels) {
  return `(async () => {
    const L = ${JSON.stringify(labels)};
    const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
    // 可交互元素优先级：radio/button/带 role 的最高，addon 次之，普通 div/span 最低。
    const interactiveScore = (el) => {
      const tag = (el.tagName || "").toUpperCase();
      const role = el.getAttribute && el.getAttribute("role");
      if (["BUTTON", "LABEL", "A", "INPUT", "SELECT"].includes(tag) || role) return 0;
      if ((el.className || "").includes("addon")) return 1;
      return 2;
    };
    const clickText = async (text, scope, tries = 6) => {
      for (let i = 0; i < tries; i += 1) {
        const els = [];
        for (const el of (scope || document).querySelectorAll("*")) {
          if (el.offsetParent === null) continue;
          if ((el.textContent || "").trim() !== text) continue;
          els.push(el);
        }
        if (els.length) {
          els.sort(
            (a, b) =>
              interactiveScore(a) - interactiveScore(b) ||
              a.textContent.length - b.textContent.length,
          );
          const t = els[0];
          t.dispatchEvent(new MouseEvent("mousedown", { bubbles: true }));
          t.dispatchEvent(new MouseEvent("mouseup", { bubbles: true }));
          t.click();
          return true;
        }
        await sleep(500);
      }
      return false;
    };
    // 1. 展开【自主声明】
    if (!(await clickText(L.entry))) return 'no-entry';
    await sleep(800);
    // 2. 选择【内容为转载】
    if (!(await clickText(L.repost))) return 'no-repost';
    await sleep(800);
    // 3. 选择【取材站外】
    if (!(await clickText(L.source))) return 'no-source';
    await sleep(500);
    // 4. 点【确定】（优先弹窗容器内的按钮，避免误点页面上其他"确定"）
    const dialogs = [...document.querySelectorAll('[class*="modal"], [class*="dialog"], [role="dialog"]')]
      .filter((d) => d.offsetParent !== null);
    let confirmed = false;
    for (const d of dialogs) {
      if (clickText(L.confirm, d)) { confirmed = true; break; }
    }
    if (!confirmed && !(await clickText(L.confirm))) return 'no-confirm';
    await sleep(500);
    // 校验：弹窗关闭，且声明栏文本已变为所选声明（防误点到别的选项）。
    const modalGone = !document.querySelector(".semi-modal");
    const box = document.querySelector('[class*="selectText"]');
    const boxText = box ? (box.textContent || "").trim() : "";
    if (!modalGone || !boxText.includes(L.repost.slice(0, 4))) return 'verification-failed';
    return 'declared';
  })()`;
}

/** 插入 #话题 并一站式点选联想项；以卡片成型（[data-mention]）为成功标准，失败保留纯文本。 */
async function applyTopicTag(tag) {
  const typed = await runOpencli(["browser", session, "eval", buildTypeTagScript(tag)]).catch(() => null);
  if (typed !== true && typed?.result !== true && typed?.value !== true) return false;
  const res = await runOpencli(["browser", session, "eval", buildTopicApplyScript(tag)]).catch(() => null);
  const s = typeof res === "string" ? res : res?.result ?? res?.value;
  if (s === "carded") return true;
  // 未成型：关掉可能残留的联想，保留的纯文本话题由进度消息提示人工处理。
  await runOpencli(["browser", session, "keys", "Escape"]).catch(() => {});
  return false;
}

/** 一次性只读视频文件服务：绑定 127.0.0.1 随机端口，只服务 /video 路径。
 *  CORS + Private-Network-Access 头齐全（https 抖音页面 fetch 本地服务需要，
 *  已在真实页面实测通过）。页面 fetch 完成后即可关闭。 */
function startVideoServer(videoPath) {
  const server = http.createServer((req, res) => {
    res.setHeader("Access-Control-Allow-Origin", "*");
    res.setHeader("Access-Control-Allow-Private-Network", "true");
    res.setHeader("Access-Control-Allow-Methods", "GET, OPTIONS");
    res.setHeader("Access-Control-Allow-Headers", "*");
    if (req.method === "OPTIONS") {
      res.writeHead(204);
      res.end();
      return;
    }
    if (!req.url || !req.url.startsWith("/video")) {
      res.writeHead(404);
      res.end();
      return;
    }
    const stat = fs.statSync(videoPath);
    res.writeHead(200, {
      "Content-Length": stat.size,
      "Content-Type": "application/octet-stream",
    });
    fs.createReadStream(videoPath).pipe(res);
  });
  return new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => resolve(server));
  });
}

/** 注入脚本（发后即忘）：枚举页面 file input，按 accept 特征选视频上传框（多个且
 *  无视频特征时拒绝注入——注错 input 上传不会开始），随后异步 fetch 本地服务字节、
 *  构造 File 赋入并 dispatch change。脚本同步返回 'started'/'no-input'/
 *  'no-video-input'——刻意不 await fetch：注入成功会触发抖音整页跳转作曲器，
 *  await 会让 eval Promise 随旧文档销毁而 hang 到 execFile 超时（实测确认）。
 *  注入是否生效由 pageReachedComposer 轮询判定，不依赖本脚本返回值。 */
function buildInjectScript(url, fileName, mime) {
  return `(() => {
    const inputs = [...document.querySelectorAll("input[type='file']")];
    if (!inputs.length) return "no-input";
    const idx = inputs.findIndex((el) =>
      /video|\\.mp4|\\.mov|\\.webm|\\.flv/i.test(el.getAttribute("accept") || ""));
    if (idx < 0 && inputs.length > 1) return "no-video-input";
    const input = inputs[idx >= 0 ? idx : 0];
    fetch(${JSON.stringify(url)})
      .then((resp) => {
        if (!resp.ok) throw new Error("fetch " + resp.status);
        return resp.arrayBuffer();
      })
      .then((buf) => {
        if (!buf.byteLength) return;
        const file = new File([buf], ${JSON.stringify(fileName)}, { type: ${JSON.stringify(mime)} });
        const dt = new DataTransfer();
        dt.items.add(file);
        input.files = dt.files;
        input.dispatchEvent(new Event("input", { bubbles: true }));
        input.dispatchEvent(new Event("change", { bubbles: true }));
      })
      .catch(() => {});
    return "started";
  })()`;
}

/** 页面是否已进入作曲器（上传/转码完成或进行中）：URL 离开 upload 页或标题框已渲染。 */
async function pageReachedComposer() {
  const url = await runOpencliRaw(["browser", session, "get", "url"]).catch(() => "");
  if (url.includes("/creator-micro/content/post")) return true;
  const res = await runOpencli([
    "browser", session, "find", "--css", SELECTORS.titleInputs[0],
  ]).catch(() => null);
  return (res?.entries ?? []).some((e) => e.visible);
}

/** 自动上传：本地服务托管视频 + 页面内 eval 内存注入（见文件头注释）。
 *  返回是否注入成功；90s 内未进入作曲器则判定失败，由调用方回退人工。 */
async function autoUploadVideo() {
  if (!fs.existsSync(job.videoPath)) {
    throw new Error(`视频文件不存在：${job.videoPath}`);
  }
  const fileName = path.basename(job.videoPath);
  const mime = {
    ".mov": "video/quicktime",
    ".webm": "video/webm",
    ".flv": "video/x-flv",
    ".mkv": "video/x-matroska",
    ".avi": "video/x-msvideo",
  }[path.extname(fileName).toLowerCase()] || "video/mp4";
  const server = await startVideoServer(job.videoPath);
  try {
    const url = `http://127.0.0.1:${server.address().port}/video${path.extname(fileName)}`;
    const deadline = Date.now() + 90_000;
    let started = false;
    while (Date.now() < deadline) {
      if (markerExists(job.cancelFile)) throw new CancelRequested();
      if (!(await sessionAlive())) throw new TabClosed("浏览器标签页已关闭");
      // 注入生效（或用户手动选择了文件）：页面进入作曲器。
      if (await pageReachedComposer()) return true;
      if (!started) {
        const res = await runOpencli([
          "browser", session, "eval", buildInjectScript(url, fileName, mime),
        ]).catch(() => null);
        const v = typeof res === "string" ? res : res?.result ?? res?.value;
        // started 之后不再重复发注入（避免命中作曲器页的重传 input 造成二次上传）。
        if (v === "started") started = true;
      }
      await new Promise((r) => setTimeout(r, 1500));
    }
    // deadline 前的循环可能被 eval 长往返挤压，最后再确认一次页面状态。
    return await pageReachedComposer();
  } finally {
    server.close();
  }
}

/** AI 封面状态探测（同步瞬时脚本）：返回 JSON 字符串
 *  {"state":"no-section"|"generating"|"ready","covers":n}。
 *  就绪判据不只看标题文案（新上传页标题可能始终显示「AI智能推荐封面」），
 *  而是封面容器内不存在任何「生成中」可见文本且已有带图封面项。 */
function buildCoverStateScript(cfg) {
  return `(() => {
    const K = ${JSON.stringify(cfg)};
    let title = null;
    for (const el of document.querySelectorAll("span,div")) {
      if (el.children.length) continue;
      if (el.offsetParent === null) continue;
      if ((el.textContent || "").includes(K.titlePattern)) { title = el; break; }
    }
    if (!title) return JSON.stringify({ state: "no-section", covers: 0 });
    let scope = title;
    for (let i = 0; i < 6 && scope; i += 1) {
      if (scope.querySelector && scope.querySelector('[class*="' + K.coverClassKeyword + '"]')) break;
      scope = scope.parentElement;
    }
    const covers = scope
      ? [...scope.querySelectorAll('[class*="' + K.coverClassKeyword + '"]')].filter(
          (el) => el.offsetParent !== null && el.querySelector("img"),
        )
      : [];
    let generating = false;
    for (const el of (scope || document).querySelectorAll("span,div")) {
      if (el.children.length) continue;
      if (el.offsetParent === null) continue;
      if ((el.textContent || "").includes(K.generatingPattern)) { generating = true; break; }
    }
    const state = generating || covers.length === 0 ? "generating" : "ready";
    return JSON.stringify({ state, covers: covers.length });
  })()`;
}

/** 封面确认弹窗脚本（异步，≤7s）：点缩略图后会弹「是否确认应用此封面？」，
 *  在可见弹窗容器内按文本精确点【确定】。返回 'confirmed' / 'no-modal'（未弹窗，
 *  可能该页面形态直接选中）/ 'no-confirm'。原生 button 派发 click 即可生效。 */
function buildCoverConfirmScript() {
  return `(async () => {
    const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
    let modal = null;
    for (let i = 0; i < 10; i += 1) {
      modal = [...document.querySelectorAll('[class*="modal"], [class*="dialog"], [role="dialog"]')]
        .filter((d) => d.offsetParent !== null)
        .find((d) => (d.textContent || "").includes("应用此封面"));
      if (modal) break;
      await sleep(500);
    }
    if (!modal) return "no-modal";
    const btn = [...modal.querySelectorAll("button")]
      .filter((b) => b.offsetParent !== null)
      .find((b) => (b.textContent || "").trim() === "确定");
    if (!btn) return "no-confirm";
    btn.click();
    await sleep(1200);
    return "confirmed";
  })()`;
}

/** AI 封面校验探测（同步瞬时脚本）：返回 JSON 字符串
 *  {"main":主封面img src, "firstCls":第一个封面项class, "firstSvg":第一项是否含svg选中标记}。 */
function buildCoverVerifyScript(cfg) {
  return `(() => {
    const item = document.querySelector('[class*="${cfg.coverClassKeyword}"]');
    const mainImg = document.querySelector('[class*="coverControl"] img');
    return JSON.stringify({
      main: mainImg ? mainImg.src : "",
      firstCls: item ? String(item.className) : "",
      firstSvg: item ? !!item.querySelector("svg") : false,
    });
  })()`;
}

/** 选 AI 推荐封面第 1 张：Node 侧轮询（每次 eval 瞬时返回，绕开 CDP 115s 单次上限），
 *  等「生成中」结束（AI 封面生成需数十秒，大视频更久，上限 150s）后用 opencli click
 *  （CDP 真实鼠标输入，先 scrollIntoView）点击第一项，再以主封面变化/选中标记校验。
 *  校验不过一律返回 false 提示人工——不再把「无法确认」当成功（曾静默失败）。 */
async function pickAiRecommendCover() {
  const deadline = Date.now() + 150_000;
  let noSection = 0;
  while (Date.now() < deadline) {
    if (markerExists(job.cancelFile)) throw new CancelRequested();
    if (!(await sessionAlive())) throw new TabClosed("浏览器标签页已关闭");
    // eval 返回裸文本（脚本内已 JSON.stringify），用 Raw 变体读取。
    const raw = await runOpencliRaw([
      "browser", session, "eval", buildCoverStateScript(SELECTORS.aiCover),
    ]).catch(() => null);
    try {
      const st = JSON.parse(raw || "");
      if (st.state === "ready" && st.covers > 0) {
        const verify = buildCoverVerifyScript(SELECTORS.aiCover);
        const before = JSON.parse(
          (await runOpencliRaw(["browser", session, "eval", verify]).catch(() => "{}")) || "{}",
        );
        const css = `[class*="${SELECTORS.aiCover.coverClassKeyword}"]`;
        const clicked = await runOpencli(["browser", session, "click", css, "--nth", "0"]).catch(
          () => null,
        );
        if (!clicked?.clicked) return false;
        // 点击后弹「是否确认应用此封面？」，点【确定】（未弹窗则该页面形态直接选中）。
        const confirm = await runOpencliRaw([
          "browser", session, "eval", buildCoverConfirmScript(),
        ]).catch(() => null);
        if (confirm !== "confirmed" && confirm !== "no-modal") return false;
        const after = JSON.parse(
          (await runOpencliRaw(["browser", session, "eval", verify]).catch(() => "{}")) || "{}",
        );
        if (before.main && after.main && before.main !== after.main) return true;
        if (after.firstSvg && !before.firstSvg) return true;
        const clsBefore = before.firstCls || "";
        const clsAfter = after.firstCls || "";
        if (clsAfter !== clsBefore && /check|select|active/i.test(clsAfter)) return true;
        return false; // 点击无可见效果：提示人工核对，不盲目报成功
      }
      // 封面区与标题框同属基础信息区、理应已渲染；连续探测不到才放弃（防短暂重组）。
      noSection = st.state === "no-section" ? noSection + 1 : 0;
      if (noSection >= 5) return false;
    } catch {
      /* eval 往返异常，继续轮询 */
    }
    await sleep(3000);
  }
  return false;
}

/** 「定时发布」radio 脚本（异步）：按可见文本精确匹配 label；已选中则不动。
 *  返回 'ready'（点击后已选中）/ 'already'（本已选中）/ 'no-radio' / 'radio-not-checked'。 */
function buildSchedulePrepScript(cfg) {
  return `(async () => {
    const K = ${JSON.stringify(cfg)};
    const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
    const labels = [...document.querySelectorAll("label")].filter(
      (l) => l.offsetParent !== null && (l.textContent || "").replace(/\\s+/g, "") === K.radioLabel,
    );
    if (!labels.length) return "no-radio";
    const label = labels[0];
    if (label.getAttribute("data-checked") === "true") return "already";
    label.dispatchEvent(new MouseEvent("mousedown", { bubbles: true }));
    label.dispatchEvent(new MouseEvent("mouseup", { bubbles: true }));
    label.click();
    await sleep(800);
    return label.getAttribute("data-checked") === "true" ? "ready" : "radio-not-checked";
  })()`;
}

/** 设置定时发布：点 radio -> fill 时间输入框 -> Enter 让 semi DatePicker 解析 ->
 *  读回校验。返回 'set' 或失败位置（供进度消息提示人工）。 */
async function applySchedule(timeStr) {
  // eval 返回裸字符串（非 JSON），用 Raw 变体读取。
  const prep = await runOpencliRaw([
    "browser", session, "eval", buildSchedulePrepScript(SELECTORS.schedule),
  ]).catch(() => null);
  if (prep !== "ready" && prep !== "already") return prep || "prep-failed";
  const inputRef = await waitSelector(
    `input[placeholder='${SELECTORS.schedule.timeInputPlaceholder}']`,
    15_000,
  );
  if (inputRef === null) return "no-input";
  const fill = await runOpencliFill(["browser", session, "fill", String(inputRef), timeStr]);
  if (!fill?.filled) return "fill-failed";
  await runOpencli(["browser", session, "keys", "Enter"]).catch(() => {});
  await sleep(1000);
  const check = await runOpencli(["browser", session, "get", "value", String(inputRef)]).catch(
    () => null,
  );
  // 关闭可能残留的日期弹层（不影响已生效值）。
  await runOpencli(["browser", session, "keys", "Escape"]).catch(() => {});
  return String(check?.value ?? "") === timeStr ? "set" : "value-mismatch";
}

async function fillComposer() {
  // 上传/转码完成后作曲器出现标题输入框（placeholder 以「填写作品标题」开头）。
  const titleRef = await waitSelector(SELECTORS.titleInputs[0], 10 * 60 * 1000);
  if (titleRef === null) {
    throw new Error("等待视频上传/转码完成超时（10 分钟）。请确认视频文件已成功上传到页面。");
  }
  writeProgress("uploading", "视频已上传完成，正在填写表单");

  const titleResult = await runOpencliFill([
    "browser", session, "fill", String(titleRef), job.title || "",
  ]);
  if (!titleResult?.filled) {
    throw new Error("标题填写失败（页面可能已改版）");
  }
  // 宽松校验：标题字段非空即可（抖音可能做归一化处理）。
  const titleCheck = await runOpencli(["browser", session, "get", "value", String(titleRef)]);
  if (!String(titleCheck?.value ?? "").trim()) {
    throw new Error("标题字段为空，填写失败");
  }

  const editorRef = await waitSelector(SELECTORS.descriptionEditors[0], 30_000);
  if (editorRef === null) {
    throw new Error("未找到文案输入区，页面结构可能已改版");
  }
  // 抖音编辑器 / opencli fill 在 contenteditable 上会稳定丢掉输入串的最后一个字符，
  // 因此在末尾补一个尾随空格（丢了也只是空格）。
  const captionResult = await runOpencliFill([
    "browser",
    session,
    "fill",
    String(editorRef),
    (job.body || "") + " ",
  ]);
  if (!captionResult?.filled) {
    throw new Error("文案填写失败（页面可能已改版）");
  }
  // 话题逐个键入并点选联想下拉项：纯文本 #话题 在抖音编辑器里不生效。
  const tagFailed = [];
  for (const raw of job.hashtags || []) {
    const tag = String(raw).trim().replace(/^[#＃]/, "");
    if (!tag) continue;
    if (!(await applyTopicTag(tag))) tagFailed.push(`#${tag}`);
  }
  // 自主声明：内容为转载 -> 取材站外 -> 确定（识别不到元素即提示人工，不盲目操作）。
  const decl = await runOpencli([
    "browser", session, "eval", buildDeclarationScript(SELECTORS.declaration),
  ]).catch(() => null);
  const declState = typeof decl === "string" ? decl : decl?.result ?? decl?.value;
  // 定时发布（快步骤先做）：agent 已按日历空档算好时间；null 表示 14 天无空档。
  let schedSet = false;
  if (job.scheduleTime) {
    schedSet = (await applySchedule(job.scheduleTime)) === "set";
  }
  // AI 智能推荐封面（慢步骤放最后：生成需数十秒，等到就绪再点选第 1 张；
  // 校验不过不盲目报成功，提示人工核对）。
  const coverPicked = await pickAiRecommendCover();
  const warns = [];
  if (tagFailed.length) {
    warns.push(`以下话题未能联想选中，仍是纯文本（不生效），请人工点选：${tagFailed.join("、")}`);
  }
  if (declState !== "declared") {
    warns.push("自主声明未能自动完成，请人工点选：自主声明 → 内容为转载信息 → 取材站外 → 确定");
  }
  if (!coverPicked) {
    warns.push("AI 推荐封面未自动选中，请人工在「设置封面」区点选第 1 张推荐封面");
  }
  if (job.scheduleTime && !schedSet) {
    warns.push("定时发布未自动设置，请人工选择「定时发布」并设置时间");
  }
  if (!job.scheduleTime) {
    warns.push("未来 14 天每天都有作品、无空档日，请人工选择定时发布时间");
  }
  const declText = declState === "declared" ? "，自主声明已选【内容为转载信息/取材站外】" : "";
  const coverText = coverPicked ? "，封面已选 AI 推荐第 1 张" : "";
  const schedText = schedSet ? `，定时发布已设为 ${job.scheduleTime}` : "";
  const warn = warns.length ? `；${warns.join("；")}` : "";
  writeProgress("form_filled", `标题/文案/话题已填写${declText}${coverText}${schedText}，请人工核对后点击页面上的【发布】${warn}`);
}

async function monitorAfterFill() {
  // 监控阶段：绝不点击发布按钮。仅等待取消、窗口关闭或显式「发布成功」提示。
  const navLog = path.join(logDir, `publisher-${job.draftId}-nav.log`);
  while (true) {
    if (markerExists(job.cancelFile)) {
      writeProgress("cancelled", "用户取消，标签页已保留（可手动处置）");
      await releaseSession();
      return "cancelled";
    }
    if (!(await sessionAlive())) {
      writeProgress("browser_closed", "浏览器标签页已关闭，未完成发布");
      await runOpencli(["browser", session, "close"]).catch(() => {}); // 释放会话租约
      return "browser_closed";
    }
    try {
      const res = await runOpencli([
        "browser",
        session,
        "find",
        "--css",
        SELECTORS.successIndicators[0],
      ]);
      if ((res?.entries ?? []).some((e) => e.visible)) {
        writeProgress("published", "页面出现「发布成功」提示");
        // 页面留给用户，解除会话租约：之后用户关窗不会触发 daemon 复活空白页。
        await runOpencli(["browser", session, "unbind"]).catch(() => {});
        return "published";
      }
    } catch {
      /* 继续轮询 */
    }
    await new Promise((r) => setTimeout(r, 3000));
  }
}

async function main() {
  writeProgress("browser_started", "正在打开抖音上传页（独立标签页）");
  // 同名会话可能残留已关闭标签页的引用（上一轮取消/异常退出），open 会撞
  // attach_failed——先幂等清理一次再打开（closeSession 含 beforeunload 弹窗处理）。
  await closeSession();
  await openUploadPage();
  // 自动上传：注入视频文件到页面 file input；失败回退人工（应用弹窗展示路径并自动复制）。
  writeProgress("uploading", "正在自动上传视频文件（注入页面文件组件，无需人工选择）");
  if (!(await autoUploadVideo())) {
    writeProgress(
      "waiting_for_file",
      "自动上传未成功（页面可能已改版），请在打开的抖音页面点击「上传视频」手动选择视频文件，选择后程序将自动填写标题/文案/话题",
    );
  }
  // 自动检测：等待作曲器（标题输入框）出现即视为已选择并上传完成。
  await fillComposer();
  const result = await monitorAfterFill();
  process.exit(result === "published" ? 0 : 2);
}

main().catch(async (err) => {
  if (err instanceof CancelRequested || markerExists(job.cancelFile)) {
    writeProgress("cancelled", "用户取消，标签页已保留（可手动处置）");
    await releaseSession();
    process.exit(2);
  }
  if (err instanceof TabClosed) {
    writeProgress("browser_closed", "浏览器标签页已关闭，未完成上传");
    process.exit(2);
  }
  writeProgress("failed", err.message || String(err));
  console.error("[publisher] failed:", err);
  await closeSession().catch(() => {});
  process.exit(1);
});
