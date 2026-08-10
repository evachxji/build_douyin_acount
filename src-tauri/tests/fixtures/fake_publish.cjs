// 测试用假 sidecar：模拟真实编排流程，不启动浏览器。
// 状态流：browser_started -> uploading -> form_filled（模拟自动上传注入成功）。
// mode=hold：停在 waiting_for_file（自动上传失败的兜底态），供冒烟人工提示弹窗。
// mode=published：等 confirm 标记后写 published。
const fs = require("fs");
const path = require("path");

const job = JSON.parse(fs.readFileSync(process.argv[2], "utf8"));
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function main() {
  const mode = process.env.FAKE_PUBLISH_MODE || "form_filled";
  fs.mkdirSync(path.dirname(job.progressFile), { recursive: true });
  const write = (state, message) =>
    fs.writeFileSync(
      job.progressFile,
      JSON.stringify({ state, message, updatedAt: new Date().toISOString() }),
    );

  write("browser_started", "fake start");
  if (mode === "never") {
    await sleep(60_000);
    return;
  }
  await sleep(300);
  if (mode === "hold") {
    write("waiting_for_file", "fake fallback: " + job.videoPath);
    await sleep(60_000); // 停在选文件兜底阶段，供冒烟验证信息弹窗
    return;
  }
  if (mode === "failed") {
    write("failed", "fake failure");
    process.exit(1);
  }
  write("uploading", "fake uploading");
  await sleep(400);
  write("form_filled", "fake form filled");

  if (mode === "published") {
    const deadline = Date.now() + 30_000;
    while (!fs.existsSync(job.confirmFile)) {
      if (Date.now() > deadline) return;
      await sleep(200);
    }
    write("published", "fake published");
  }
}

main();
