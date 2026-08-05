// 测试用假 sidecar：模拟真实编排流程，不启动浏览器。
// 状态流：browser_started -> waiting_for_file -> uploading -> form_filled（自动，无需人工放行）
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
  write("waiting_for_file", "fake waiting: " + job.videoPath);
  if (mode === "hold") {
    await sleep(60_000); // 停在选文件阶段，供冒烟验证信息弹窗
    return;
  }
  if (mode === "failed") {
    write("failed", "fake failure");
    process.exit(1);
  }
  await sleep(400);
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
