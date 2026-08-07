// 测试用分批假采集 sidecar：模拟真实 sidecar 逐条追加 videos.jsonl 的时序。
// 先写 2 条（让 Agent 在 RUNNING 阶段增量导入），等 1.2s（> 几个轮询周期），
// 再追加第 3 条并写 done——复现"完成时大部分视频已在 RUNNING 阶段入库"的真实场景。
const fs = require("fs");
const path = require("path");

const job = JSON.parse(fs.readFileSync(process.argv[2], "utf8"));
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function rec(i) {
  return {
    index: i,
    platform: job.platform,
    id: `fake${i}`,
    title: `fake ${i}`,
    author: "tester",
    url: `https://example.com/${i}`,
    likes: 100 * i,
    plays: 100,
    comments: 1,
    file: path.join(job.outputDir, `fake${i}.mp4`),
  };
}

async function main() {
  fs.mkdirSync(path.dirname(job.progressFile), { recursive: true });
  const write = (state, downloaded) =>
    fs.writeFileSync(
      job.progressFile,
      JSON.stringify({ state, downloaded, target: job.count, message: `batch ${downloaded}` }),
    );
  write("running", 0);

  // 第一批：2 条，Agent 会在 RUNNING 阶段导入。
  fs.appendFileSync(job.videosFile, JSON.stringify(rec(1)) + "\n");
  fs.appendFileSync(job.videosFile, JSON.stringify(rec(2)) + "\n");
  write("running", 2);
  await sleep(1200);

  // 第二批：最后 1 条 + done。
  fs.appendFileSync(job.videosFile, JSON.stringify(rec(3)) + "\n");
  write("done", 3);
}

main();
