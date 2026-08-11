// 测试用假采集 sidecar：写 running，写 videos.jsonl（引用 outputDir 下预置的 fakeN.mp4），再写 done。
// done 时附带 stats（模拟真实 sidecar 的损耗留痕），供 Agent 带进任务结果。
const fs = require("fs");
const path = require("path");

const job = JSON.parse(fs.readFileSync(process.argv[2], "utf8"));
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function main() {
  fs.mkdirSync(path.dirname(job.progressFile), { recursive: true });
  const write = (state, msg, stats) =>
    fs.writeFileSync(
      job.progressFile,
      JSON.stringify({
        state,
        downloaded: 0,
        target: job.count,
        message: msg,
        ...(stats ? { stats } : {}),
      }),
    );
  write("running", "fake collect start");

  const mode = process.env.FAKE_COLLECT_MODE || "done";
  if (mode === "failed") {
    write("failed", "fake collect failure");
    process.exit(1);
  }
  await sleep(400);

  const records = [
    {
      index: 1,
      platform: job.platform,
      id: "fake1",
      title: "fake one",
      author: "tester",
      url: "https://example.com/1",
      likes: 100,
      plays: 200,
      comments: 3,
      file: path.join(job.outputDir, "fake1.mp4"),
    },
    {
      index: 2,
      platform: job.platform,
      id: "fake2",
      title: "fake two",
      author: "tester",
      url: "https://example.com/2",
      likes: 50,
      plays: 100,
      comments: 1,
      file: path.join(job.outputDir, "fake2.mp4"),
    },
  ].slice(0, job.count);
  fs.writeFileSync(job.videosFile, records.map((r) => JSON.stringify(r)).join("\n"));
  write("done", "fake collect done", {
    feed_entries: records.length + 1,
    candidates: records.length,
    filtered_likes: 1,
    downloaded: records.length,
  });
}

main();
