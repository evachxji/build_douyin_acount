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
// 4. 【发布按钮点击的唯一授权路径】填表完成后停在发布前，等待用户在应用内
//    侧栏点勾授权（confirm 标记），sidecar 据此代点一次发布按钮（仅一次，
//    幂等）；用户亦可自行在页面点击发布（跳转检测兜底）。除此之外不存在任何
//    自动/定时/批量触发点击的路径；识别不到按钮元素即停止并提示，不盲目操作。
// 5. 发布已提交检测（页面跳转内容管理 / 显式成功提示，被动零点击），
//    命中后读回注入钩子捕获的作品 id/标题/定时时间/封面，随 progress 上报 agent
//    直接登记发布日历；结束仅释放会话租约，标签页保留给用户，绝不关闭。
//
// 用法：node publish.js <job.json>
// job.json 字段：draftId / videoPath / title / body / hashtags[] /
//                declaration（自主声明开关，false 时跳过「转载/取材站外」自动设置，缺省开启）/
//                scheduleTime（"YYYY-MM-DD HH:mm" 本地时间或 null——agent 按发布日历
//                空档日 + 08:00~19:00 随机分钟算好；null 表示 14 天无空档，提示人工）/
//                progressFile / cancelFile / confirmFile / coverDir（封面落盘目录）

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
const USER_HOME = process.env.USERPROFILE ?? "";
const OPENCLI_ENTRIES = [
  path.join(USER_HOME, "AppData/Local/nvm/v24.14.0/node_modules/@jackwener/opencli/dist/src/main.js"),
  "C:/nvm4w/nodejs/node_modules/@jackwener/opencli/dist/src/main.js",
  path.join(USER_HOME, "AppData/Roaming/npm/node_modules/@jackwener/opencli/dist/src/main.js"),
];

class CancelRequested extends Error {}
class TabClosed extends Error {}

// 最近一次进度：心跳定时器原样重写，保持 progress 文件 mtime 常新。
let lastProgress = null;

function writeProgress(state, message, extra) {
  lastProgress = { state, message: message || "" };
  const payload = {
    state,
    message: message || "",
    updatedAt: new Date().toISOString(),
    ...(extra || {}),
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

/** 设置封面槽位区（横封面4:3 / 竖封面3:4）状态探测（同步瞬时脚本）：
 *  AI 推荐封面被选中应用到一个槽位后，另一个方向的槽位封面会进入自动生成。
 *  以「设置封面」标题为锚向上找到含「横封面/竖封面」标签的槽位区容器，
 *  其内可见叶子文本含「生成中」即未就绪。返回 {"state":"ready"|"generating"|"no-section"}。 */
function buildCoverSlotsStateScript(cfg) {
  return `(() => {
    const K = ${JSON.stringify(cfg)};
    let anchor = null;
    for (const el of document.querySelectorAll("span,div")) {
      if (el.children.length) continue;
      if (el.offsetParent === null) continue;
      if ((el.textContent || "").includes(K.slotTitlePattern)) { anchor = el; break; }
    }
    if (!anchor) return JSON.stringify({ state: "no-section" });
    const slotRe = new RegExp(K.slotLabelPattern);
    let scope = anchor;
    for (let i = 0; i < 8 && scope; i += 1) {
      if (slotRe.test(scope.textContent || "")) break;
      scope = scope.parentElement;
    }
    if (!scope || !slotRe.test(scope.textContent || "")) return JSON.stringify({ state: "no-section" });
    let generating = false;
    for (const el of scope.querySelectorAll("span,div")) {
      if (el.children.length) continue;
      if (el.offsetParent === null) continue;
      if ((el.textContent || "").includes(K.generatingPattern)) { generating = true; break; }
    }
    return JSON.stringify({ state: generating ? "generating" : "ready" });
  })()`;
}

/** 等设置封面槽位区（另一个方向的封面）自动生成完毕：AI 封面选中应用后触发，
 *  与 AI 封面生成同为慢操作（上限 180s），就绪后才应交还人工弹确认窗。
 *  超时/连续探测失败返回 false（warn 提示人工），不阻断表单完成。 */
async function waitCoverSlotsReady() {
  const deadline = Date.now() + 180_000;
  let noSection = 0;
  while (Date.now() < deadline) {
    if (markerExists(job.cancelFile)) throw new CancelRequested();
    if (!(await sessionAlive())) throw new TabClosed("浏览器标签页已关闭");
    // eval 返回裸文本（脚本内已 JSON.stringify），用 Raw 变体读取。
    const raw = await runOpencliRaw([
      "browser", session, "eval", buildCoverSlotsStateScript(SELECTORS.aiCover),
    ]).catch(() => null);
    try {
      const st = JSON.parse(raw || "");
      if (st.state === "ready") return true;
      noSection = st.state === "no-section" ? noSection + 1 : 0;
      if (noSection >= 5) return false;
    } catch {
      /* eval 往返异常，继续轮询 */
    }
    await sleep(3000);
  }
  return false;
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

/** 视频备选封面点击脚本（同步瞬时）：超时预备方案用。
 *  候选识别刻意避开类名依赖（曾踩 recommendCover 子串误匹配坑）：先以「智能推荐封面」
 *  标题锚定 AI 区容器，找不到候选再以「设置封面」标题锚定槽位区容器（含 横封面|竖封面
 *  标签文本的祖先）。候选项 = 容器内可见 img（naturalWidth/Height > 40，排除图标）且所属
 *  项内无「生成中」文本（半成品排除）。点击第 1 项并打临时标记 data-pub-pick 供校验找回。
 *  返回 {"picked":bool,"count":n}。 */
function buildFrameCoverPickScript(cfg) {
  return `(() => {
    const K = ${JSON.stringify(cfg)};
    const leafByText = (text) => {
      for (const el of document.querySelectorAll("span,div")) {
        if (el.children.length) continue;
        if (el.offsetParent === null) continue;
        if ((el.textContent || "").includes(text)) return el;
      }
      return null;
    };
    const scopeWithImg = (anchor) => {
      let scope = anchor;
      for (let i = 0; i < 6 && scope; i += 1) {
        if (scope.querySelector && scope.querySelector("img")) return scope;
        scope = scope.parentElement;
      }
      return null;
    };
    const slotScope = (anchor) => {
      const slotRe = new RegExp(K.slotLabelPattern);
      let scope = anchor;
      for (let i = 0; i < 8 && scope; i += 1) {
        if (slotRe.test(scope.textContent || "")) return scope;
        scope = scope.parentElement;
      }
      return null;
    };
    const candidatesIn = (scope) => {
      if (!scope) return [];
      const imgs = [...scope.querySelectorAll("img")].filter(
        (im) => im.offsetParent !== null && im.naturalWidth > 40 && im.naturalHeight > 40,
      );
      const items = [];
      for (const im of imgs) {
        // 候选项：向上找不含其他 img 的最小容器（单图项），跳过含「生成中」文本的半成品。
        let item = im;
        for (let i = 0; i < 4 && item.parentElement; i += 1) {
          const p = item.parentElement;
          if (p.querySelectorAll("img").length > 1) break;
          item = p;
        }
        if ((item.textContent || "").includes(K.generatingPattern)) continue;
        items.push(item);
      }
      return items;
    };
    let items = candidatesIn(scopeWithImg(leafByText(K.titlePattern)));
    if (!items.length) items = candidatesIn(slotScope(leafByText(K.slotTitlePattern)));
    if (!items.length) return JSON.stringify({ picked: false, count: 0 });
    const t = items[0];
    t.setAttribute("data-pub-pick", "1");
    t.dispatchEvent(new MouseEvent("mousedown", { bubbles: true }));
    t.dispatchEvent(new MouseEvent("mouseup", { bubbles: true }));
    t.click();
    return JSON.stringify({ picked: true, count: items.length });
  })()`;
}

/** 降级点击后的选中态校验：data-pub-pick 标记项是否出现选中类名或 svg 勾。 */
function buildFrameCoverVerifyScript() {
  return `(() => {
    const t = document.querySelector("[data-pub-pick='1']");
    return JSON.stringify({
      selected: !!t && (/select|check|active/i.test(String(t.className || "")) || !!t.querySelector("svg")),
    });
  })()`;
}

/** 超时预备方案：AI 推荐封面未生成（服务端超时/卡死）时，改选视频备选封面第 1 张。
 *  校验两信号：主封面 img src 变化 / 点击项出现选中态；全不命中返回 false 提示人工，
 *  不把「无法确认」当成功。 */
async function pickVideoFrameCover() {
  const verify = buildCoverVerifyScript(SELECTORS.aiCover);
  const before = JSON.parse(
    (await runOpencliRaw(["browser", session, "eval", verify]).catch(() => "{}")) || "{}",
  );
  const raw = await runOpencliRaw([
    "browser", session, "eval", buildFrameCoverPickScript(SELECTORS.aiCover),
  ]).catch(() => null);
  let st = null;
  try {
    st = JSON.parse(raw || "");
  } catch {
    /* eval 往返异常按未选中处理 */
  }
  if (!st?.picked) return false;
  await sleep(1500);
  const after = JSON.parse(
    (await runOpencliRaw(["browser", session, "eval", verify]).catch(() => "{}")) || "{}",
  );
  if (before.main && after.main && before.main !== after.main) return true;
  const sel = await runOpencliRaw(["browser", session, "eval", buildFrameCoverVerifyScript()]).catch(
    () => null,
  );
  try {
    return JSON.parse(sel || "{}").selected === true;
  } catch {
    return false;
  }
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
  // 自主声明：设置为开启时自动执行（内容为转载 -> 取材站外 -> 确定）；
  // 关闭（job.declaration === false）则整步跳过，由用户自行决定是否人工声明。
  let declState = "skipped";
  if (job.declaration !== false) {
    const decl = await runOpencli([
      "browser", session, "eval", buildDeclarationScript(SELECTORS.declaration),
    ]).catch(() => null);
    declState = typeof decl === "string" ? decl : decl?.result ?? decl?.value;
  }
  // 定时发布（快步骤先做）：agent 已按日历空档算好时间；null 表示 14 天无空档。
  let schedSet = false;
  if (job.scheduleTime) {
    schedSet = (await applySchedule(job.scheduleTime)) === "set";
  }
  // AI 智能推荐封面（慢步骤放最后：生成需数十秒，等到就绪再点选第 1 张；
  // 校验不过不盲目报成功，提示人工核对）。
  let coverPicked = await pickAiRecommendCover();
  // 超时预备方案：AI 封面一直没生成（服务端超时/卡死）时，降级改选右侧
  // 视频备选封面（抖音自动抽帧生成的那几张，生成快、通常早已就绪）。
  let coverFallback = false;
  if (!coverPicked) {
    coverPicked = await pickVideoFrameCover();
    coverFallback = coverPicked;
  }
  // AI 封面选中应用到一个槽位后，另一个方向（横/竖）的槽位封面会进入自动生成——
  // 等它也生成完再交还人工，避免确认窗弹出时槽位封面还是半成品。
  const slotsReady = coverPicked ? await waitCoverSlotsReady() : false;
  const warns = [];
  if (tagFailed.length) {
    warns.push(`以下话题未能联想选中，仍是纯文本（不生效），请人工点选：${tagFailed.join("、")}`);
  }
  if (job.declaration !== false && declState !== "declared") {
    warns.push("自主声明未能自动完成，请人工点选：自主声明 → 内容为转载信息 → 取材站外 → 确定");
  }
  if (!coverPicked) {
    warns.push("AI 推荐封面未生成，视频备选封面也未能自动选中，请人工在「设置封面」区点选封面");
  }
  if (coverPicked && !slotsReady) {
    warns.push("另一个方向的封面（横/竖）仍在自动生成中，请人工稍候核对");
  }
  if (job.scheduleTime && !schedSet) {
    warns.push("定时发布未自动设置，请人工选择「定时发布」并设置时间");
  }
  if (!job.scheduleTime) {
    warns.push("未来 14 天每天都有作品、无空档日，请人工选择定时发布时间");
  }
  const declText = declState === "declared" ? "，自主声明已选【内容为转载信息/取材站外】" : "";
  const coverText = coverPicked
    ? coverFallback
      ? "，AI 封面未生成，已改选视频备选封面"
      : "，封面已选 AI 推荐第 1 张"
    : "";
  const schedText = schedSet ? `，定时发布已设为 ${job.scheduleTime}` : "";
  const warn = warns.length ? `；${warns.join("；")}` : "";
  writeProgress("form_filled", `标题/文案/话题已填写${declText}${coverText}${schedText}，请人工核对后点击页面上的【发布】${warn}`);
}

/** 发布信息捕获钩子：wrap 页面的 fetch/XHR，凡 `/media/(aweme|item|video)` 族接口
 *  （草稿保存与发布提交同族，moderate 契约已确认该域族）响应里带 aweme_id/item_id 时，
 *  把当时的页面实际值（标题/定时时间/封面截图）连同作品 id 快照进 localStorage。
 *  localStorage 同源持久——发布提交后页面跳转到内容管理，数据仍可在新页面读回。 */
async function injectPublishHook() {
  const script = `(() => {
    if (window.__pubHookInstalled) return 'already';
    window.__pubHookInstalled = true;
    const RE = new RegExp('media/(aweme|item|video)');
    const pick = (j) => {
      if (!j || typeof j !== 'object') return '';
      const d = j.data && typeof j.data === 'object' ? j.data : j;
      return String(d.aweme_id || d.item_id || '');
    };
    const snapCover = () => {
      try {
        const img = document.querySelector('[class*="recommendCover-"][class*="selected-"] img')
          || document.querySelector('[class*="cover"] img[src^="blob:"]');
        if (!img || !img.naturalWidth) return '';
        const scale = Math.min(1, 480 / img.naturalWidth);
        const c = document.createElement('canvas');
        c.width = Math.max(1, Math.round(img.naturalWidth * scale));
        c.height = Math.max(1, Math.round(img.naturalHeight * scale));
        c.getContext('2d').drawImage(img, 0, 0, c.width, c.height);
        return c.toDataURL('image/jpeg', 0.8);
      } catch (e) { return ''; }
    };
    const capture = (awemeId) => {
      try {
        const titleEl = document.querySelector('input[placeholder*="标题"],textarea[placeholder*="标题"]');
        const timeInput = document.querySelector('input[placeholder="日期和时间"]');
        localStorage.setItem('__pub_capture', JSON.stringify({
          at: Date.now(),
          awemeId: String(awemeId || ''),
          title: (titleEl && titleEl.value) || '',
          scheduleTime: (timeInput && timeInput.value) || '',
          coverData: snapCover(),
        }));
      } catch (e) {}
    };
    const of = window.fetch.bind(window);
    window.fetch = (...a) => of(...a).then((res) => {
      try {
        const u = typeof a[0] === 'string' ? a[0] : (a[0] && a[0].url) || '';
        if (RE.test(u) && res.ok) {
          res.clone().json().then((j) => { const id = pick(j); if (id) capture(id); }).catch(() => {});
        }
      } catch (e) {}
      return res;
    });
    const oo = XMLHttpRequest.prototype.open;
    const os = XMLHttpRequest.prototype.send;
    XMLHttpRequest.prototype.open = function (m, u, ...r) { this.__pubUrl = u; return oo.call(this, m, u, ...r); };
    XMLHttpRequest.prototype.send = function (...a) {
      this.addEventListener('load', () => {
        try {
          if (RE.test(this.__pubUrl || '') && this.status < 300) {
            const id = pick(JSON.parse(this.responseText));
            if (id) capture(id);
          }
        } catch (e) {}
      });
      return os.apply(this, a);
    });
    return 'installed';
  })()`;
  await runOpencliRaw(["browser", session, "eval", script]).catch(() => {});
}

/** 读回钩子快照（localStorage，跨页面跳转存活；30 分钟有效期防陈旧），读完即清除。
 *  钩子未命中（接口改版）时以当前 DOM 值兜底（页面未跳转的场景）。 */
async function readCapturedPost() {
  const script = `(() => {
    let c = null;
    try { c = JSON.parse(localStorage.getItem('__pub_capture') || 'null'); } catch (e) {}
    try { localStorage.removeItem('__pub_capture'); } catch (e) {}
    if (c && c.at && Date.now() - c.at > 30 * 60 * 1000) c = null;
    const titleEl = document.querySelector('input[placeholder*="标题"],textarea[placeholder*="标题"]');
    const timeInput = document.querySelector('input[placeholder="日期和时间"]');
    return JSON.stringify({
      awemeId: (c && c.awemeId) || '',
      title: (c && c.title) || (titleEl && titleEl.value) || '',
      scheduleTime: (c && c.scheduleTime) || (timeInput && timeInput.value) || '',
      coverData: (c && c.coverData) || '',
    });
  })()`;
  const raw = await runOpencliRaw(["browser", session, "eval", script]).catch(() => "");
  try {
    return JSON.parse(raw);
  } catch {
    return null;
  }
}

/** 封面 dataURL 落盘到 covers 目录（日历直接展示本地文件）；失败返回空串。 */
function saveCoverData(coverData) {
  const m = /^data:image\/(jpeg|jpg|png);base64,(.+)$/.exec(coverData || "");
  if (!m || !job.coverDir) return "";
  try {
    fs.mkdirSync(job.coverDir, { recursive: true });
    const p = path.join(
      job.coverDir,
      `local_${job.draftId}.${m[1] === "png" ? "png" : "jpg"}`,
    );
    fs.writeFileSync(p, Buffer.from(m[2], "base64"));
    return p;
  } catch {
    return "";
  }
}

/** 发布完成统一出口：读回钩子捕获的页面实际值（用户可能手改过标题/时间），
 *  随 progress 上报，agent 据此直接登记发布日历（不再跑整轮同步）。
 *  仅释放会话租约——标签页保留给用户，绝不关闭。 */
async function finishPublished(message) {
  const cap = (await readCapturedPost()) || {};
  const coverPath = saveCoverData(cap.coverData);
  writeProgress("published", message, {
    post: {
      awemeId: cap.awemeId || "",
      title: cap.title || job.title || "",
      scheduleTime: cap.scheduleTime || job.scheduleTime || "",
      coverPath,
    },
  });
  await runOpencli(["browser", session, "unbind"]).catch(() => {});
  return "published";
}

/** 发布按钮点击脚本（同步瞬时）：用户在应用内点勾授权后由 sidecar 代点一次。
 *  按候选可见文本精确匹配「可见且未 disabled」的 button（候选含「定时发布」——
 *  设置定时后按钮文本变化）。返回 'clicked' / 'not-found' / 'ambiguous'。
 *  识别不到或匹配到多个一律不点（不盲目操作），由进度消息提示人工。 */
function buildPublishClickScript(labels) {
  return `(() => {
    const L = ${JSON.stringify(labels)};
    const btns = [...document.querySelectorAll("button")].filter((b) => {
      if (b.offsetParent === null) return false;
      if (b.disabled || b.getAttribute("aria-disabled") === "true") return false;
      return L.includes((b.textContent || "").replace(/\\s+/g, ""));
    });
    if (!btns.length) return "not-found";
    if (btns.length > 1) return "ambiguous";
    const b = btns[0];
    b.dispatchEvent(new MouseEvent("mousedown", { bubbles: true }));
    b.dispatchEvent(new MouseEvent("mouseup", { bubbles: true }));
    b.click();
    return "clicked";
  })()`;
}

async function monitorAfterFill() {
  // 监控阶段：先注入发布信息捕获钩子，再等待取消、点勾授权（confirm 标记，
  // 代点一次发布按钮）、窗口关闭或「发布已提交」信号。除点勾授权的一次代点外，
  // 绝不主动点击发布按钮；用户自行在页面点击发布同样被检测兜底。
  const navLog = path.join(logDir, `publisher-${job.draftId}-nav.log`);
  await injectPublishHook();
  // confirm 标记文件持续存在——confirmHandled 保证整个监控期只代点一次。
  let confirmHandled = false;
  let publishClickedAt = 0;
  let slowWarned = false;
  while (true) {
    if (markerExists(job.cancelFile)) {
      writeProgress("cancelled", "用户取消，标签页已保留（可手动处置）");
      await releaseSession();
      return "cancelled";
    }
    // 用户在应用内点勾授权发布：代点一次发布按钮。
    if (!confirmHandled && markerExists(job.confirmFile)) {
      confirmHandled = true;
      const r = await runOpencliRaw([
        "browser", session, "eval", buildPublishClickScript(SELECTORS.publishButton.labels),
      ]).catch(() => null);
      if (r === "clicked") {
        publishClickedAt = Date.now();
        writeProgress("form_filled", "已按你的确认点击【发布】，等待发布结果…");
      } else {
        writeProgress(
          "form_filled",
          `未能自动点击发布按钮（${r === "ambiguous" ? "匹配到多个候选" : "未找到可点击的按钮"}），请人工在页面点击【发布】`,
        );
      }
    }
    // 代点后 90s 仍无成功信号：提醒检查页面（校验提示/二次弹窗），人工点击仍被检测兜底。
    if (publishClickedAt && !slowWarned && Date.now() - publishClickedAt > 90_000) {
      slowWarned = true;
      writeProgress(
        "form_filled",
        "点击发布已超过 90 秒未检测到成功，请检查页面是否有校验提示或确认弹窗；可人工在页面点击【发布】",
      );
    }
    // 一次 tab list 同时判活与取当前 URL（get url 会为无活页会话创建幽灵页，不用）。
    const tabsRes = await runOpencli(["browser", session, "tab", "list"]).catch(() => null);
    const tabs = Array.isArray(tabsRes) ? tabsRes : tabsRes?.tabs ?? [];
    const httpTab = tabs.find((t) => String(t.url || "").startsWith("http"));
    if (!httpTab) {
      writeProgress("browser_closed", "浏览器标签页已关闭，未完成发布");
      await runOpencli(["browser", session, "close"]).catch(() => {}); // 释放会话租约
      return "browser_closed";
    }
    // 信号一（主）：页面跳转到内容管理页 = 发布已提交。「发布成功」toast 一闪而过，
    // 3s 轮询未必抓得到，URL 跳转是更可靠的滞后证据。
    const url = String(httpTab.url || "");
    if (url.includes("/creator-micro/content/manage")) {
      return await finishPublished(`页面已跳转到内容管理（${url}），发布已提交`);
    }
    // 信号二：显式「发布成功」提示（发布后未跳页的场景）。
    for (const sel of SELECTORS.successIndicators) {
      try {
        const res = await runOpencli(["browser", session, "find", "--css", sel]);
        if ((res?.entries ?? []).some((e) => e.visible)) {
          return await finishPublished(`页面出现「发布成功」提示（${sel}）`);
        }
      } catch {
        /* 继续轮询 */
      }
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
