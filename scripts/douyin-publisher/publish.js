// 抖音发布辅助 sidecar（opencli 编排版）：
// 1. 在用户已登录的 Chrome 中打开 creator.douyin.com 上传页（opencli 会话）。
// 2. 文件选择弹窗由用户本人操作：程序展示视频完整路径（可一键复制），
//    用户选好文件后点击应用内【已选择】按钮（写入 continue 标记）。
// 3. sidecar 检测到 continue 后，等待上传/转码完成，自动填写标题/文案/话题。
// 4. 【绝不点击最终发布按钮】——填表完成后保持窗口打开，等待用户本人核对并点击发布。
// 5. 仅当页面出现显式「发布成功」提示才上报 published；窗口关闭/取消如实上报。
//
// 用法：node publish.js <job.json>
// job.json 字段：draftId / videoPath / title / body / hashtags[] /
//                progressFile / cancelFile / continueFile

import { execFile } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

const SELECTORS = JSON.parse(
  fs.readFileSync(new URL("./selectors.json", import.meta.url), "utf8"),
);

const job = JSON.parse(fs.readFileSync(process.argv[2], "utf8"));
const logDir = path.dirname(job.progressFile);
fs.mkdirSync(logDir, { recursive: true });
const session = `dy-pub-${job.draftId}`;

// opencli CLI 入口候选（nvm 多版本安装下的探测）。
const OPENCLI_ENTRIES = [
  "C:/Users/<user>/AppData/Local/nvm/v24.14.0/node_modules/@jackwener/opencli/dist/src/main.js",
  "C:/nvm4w/nodejs/node_modules/@jackwener/opencli/dist/src/main.js",
  "C:/Users/<user>/AppData/Roaming/npm/node_modules/@jackwener/opencli/dist/src/main.js",
];

function writeProgress(state, message) {
  const payload = {
    state,
    message: message || "",
    updatedAt: new Date().toISOString(),
  };
  fs.writeFileSync(job.progressFile, JSON.stringify(payload, null, 2));
  console.log(`[publisher] ${state} ${message || ""}`);
}

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

/** 关闭会话标签页。 */
async function closeSession() {
  // 清除页面 beforeunload，避免 Chrome 弹出「离开此网站」二次确认阻塞关闭。
  await runOpencli([
    "browser",
    session,
    "eval",
    "window.onbeforeunload=null;window.onunload=null;true",
  ]).catch(() => {});
  await runOpencli(["browser", session, "close"]).catch(() => {});
}

/** 探测标签页是否仍存活（用户关闭标签页后会话失效）。 */
async function sessionAlive() {
  try {
    const url = await runOpencliRaw(["browser", session, "get", "url"]);
    return url.startsWith("http");
  } catch {
    return false;
  }
}

/** 等待某个 CSS 选择器可见（opencli find 探测）。 */
async function waitSelector(selector, timeoutMs, intervalMs = 3000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
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

async function waitCancelOrContinue(timeoutMs) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (markerExists(job.cancelFile)) return "cancel";
    if (markerExists(job.continueFile)) return "continue";
    if (!(await sessionAlive())) return "closed";
    await new Promise((r) => setTimeout(r, 2000));
  }
  return "timeout";
}

async function fillComposer() {
  // 上传/转码完成后作曲器出现标题输入框（placeholder 以「填写作品标题」开头）。
  const titleRef = await waitSelector(SELECTORS.titleInputs[0], 10 * 60 * 1000);
  if (titleRef === null) {
    throw new Error("等待视频上传/转码完成超时（10 分钟）。请确认已在弹窗中选择视频文件。");
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
  // 抖音编辑器/ opencli fill 在 contenteditable 上会稳定丢掉输入串的最后一个字符，
  // 因此在末尾补一个尾随空格（丢了也只是空格；同时让最后一个话题卡片完整提交）。
  const captionParts = [job.body || "", ...(job.hashtags || []).map((t) => `#${t}`)];
  const captionResult = await runOpencliFill([
    "browser",
    session,
    "fill",
    String(editorRef),
    captionParts.filter(Boolean).join(" ") + " ",
  ]);
  if (!captionResult?.filled) {
    throw new Error("文案填写失败（页面可能已改版）");
  }
  writeProgress("form_filled", "标题/文案/话题已填写，请人工核对后点击页面上的【发布】");
}

async function monitorAfterFill() {
  // 监控阶段：绝不点击发布按钮。仅等待取消、窗口关闭或显式「发布成功」提示。
  const navLog = path.join(logDir, `publisher-${job.draftId}-nav.log`);
  while (true) {
    if (markerExists(job.cancelFile)) {
      writeProgress("cancelled", "用户取消，关闭浏览器标签页");
      await closeSession();
      return "cancelled";
    }
    if (!(await sessionAlive())) {
      writeProgress("browser_closed", "浏览器标签页已关闭，未完成发布");
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
  await openUploadPage();
  // 文件选择弹窗由用户本人操作；此处仅给出路径并等待【已选择】。
  writeProgress(
    "waiting_for_file",
    `请在打开的抖音页面点击「上传视频」，在文件选择弹窗中选择：${job.videoPath}`,
  );

  const wait = await waitCancelOrContinue(60 * 60 * 1000);
  if (wait === "cancel") {
    writeProgress("cancelled", "用户取消");
    await closeSession();
    process.exit(2);
  }
  if (wait === "closed") {
    writeProgress("browser_closed", "浏览器标签页已关闭，未完成上传");
    process.exit(2);
  }
  if (wait === "timeout") {
    writeProgress("failed", "等待【已选择】超时（1 小时）");
    await closeSession();
    process.exit(1);
  }

  await fillComposer();
  const result = await monitorAfterFill();
  process.exit(result === "published" ? 0 : 2);
}

main().catch(async (err) => {
  writeProgress("failed", err.message || String(err));
  console.error("[publisher] failed:", err);
  await closeSession().catch(() => {});
  process.exit(1);
});
