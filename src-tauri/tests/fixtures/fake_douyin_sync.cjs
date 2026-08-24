// 测试用假抖音同步 sidecar：写 running，写 posts.jsonl（一条已发布 + 一条定时），再写 done。
// FAKE_SYNC_MODE=failed 时改写 failed。
const fs = require("fs");
const path = require("path");

const job = JSON.parse(fs.readFileSync(process.argv[2], "utf8"));
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function main() {
  fs.mkdirSync(path.dirname(job.progressFile), { recursive: true });
  const write = (state, msg) =>
    fs.writeFileSync(
      job.progressFile,
      JSON.stringify({ state, synced: 0, scheduled: 0, message: msg }),
    );
  write("running", "fake sync start");

  const mode = process.env.FAKE_SYNC_MODE || "done";
  if (mode === "failed") {
    write("failed", "fake sync failure");
    process.exit(1);
  }
  await sleep(300);

  const posts = [
    {
      aweme_id: "fake_dy_1",
      title: "fake published",
      cover_path: null,
      cover_url: "https://example.com/c1.webp",
      status: "published",
      play_count: 1234,
      digg_count: 56,
      comment_count: 7,
      share_count: 2,
      completion_rate: 23.99,
      bounce_rate_2s: 44.87,
      create_time: 1785000000,
      public_time: 0,
    },
    {
      aweme_id: "fake_dy_2",
      title: "fake scheduled",
      cover_path: null,
      cover_url: "https://example.com/c2.webp",
      status: "scheduled",
      play_count: 0,
      digg_count: 0,
      comment_count: 0,
      share_count: 0,
      completion_rate: null,
      bounce_rate_2s: null,
      create_time: 4102444800, // 定时作品：create_time 即定时发布时间（2100 年，恒定未来）
      public_time: 0,
    },
    {
      aweme_id: "fake_dy_3",
      title: "fake violating",
      cover_path: null,
      cover_url: "https://example.com/c3.webp",
      status: "published",
      play_count: 11,
      digg_count: 0,
      comment_count: 0,
      share_count: 0,
      completion_rate: null,
      bounce_rate_2s: 60.0,
      violation_status: 5,
      violation_text: "作品流量减少，优化后可以获得更多流量",
      violation_brief: "原创性不足",
      violation_url: "https://aweme.snssdk.com/falcon/fe_douyin_security_react/media_appeal/?object_id=fake_dy_3",
      create_time: 1785000200,
      public_time: 0,
    },
  ];
  fs.writeFileSync(
    job.postsFile,
    posts.map((p) => JSON.stringify(p)).join("\n") + "\n",
  );
  fs.writeFileSync(
    job.progressFile,
    JSON.stringify({ state: "done", synced: 3, scheduled: 1, violations: 1, message: "fake sync done" }),
  );
}

main();
