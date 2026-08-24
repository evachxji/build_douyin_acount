// 测试用假违规处理 sidecar：写 running -> results.jsonl -> done（带 stats）。
// FAKE_MODERATE_MODE=failed 时改写 failed。
const fs = require("fs");
const path = require("path");

const job = JSON.parse(fs.readFileSync(process.argv[2], "utf8"));
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function main() {
  fs.mkdirSync(path.dirname(job.progressFile), { recursive: true });
  const total = (job.targets || []).length;
  const write = (state, msg, stats) =>
    fs.writeFileSync(
      job.progressFile,
      JSON.stringify({ state, done: 0, total, message: msg, ...(stats ? { stats } : {}) }),
    );
  write("running", "fake moderate start");

  const mode = process.env.FAKE_MODERATE_MODE || "done";
  if (mode === "failed") {
    write("failed", "fake moderate failure");
    process.exit(1);
  }
  await sleep(300);

  const results = [];
  for (const t of job.targets || []) {
    results.push({ aweme_id: t.aweme_id, action: "set_private", ok: true });
  }
  if ((job.promoteQueue || []).length > 0) {
    results.push({ aweme_id: job.promoteQueue[0].aweme_id, action: "publish_now", ok: true });
  }
  fs.writeFileSync(job.resultsFile, results.map((r) => JSON.stringify(r)).join("\n") + "\n");
  const stats = {
    privated: (job.targets || []).length,
    published: (job.promoteQueue || []).length > 0 ? 1 : 0,
    failed: 0,
  };
  write("done", "fake moderate done", stats);
}

main();
