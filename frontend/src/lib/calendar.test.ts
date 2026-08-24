import { describe, expect, it } from "vitest";
import type { DouyinPost } from "../api/types";
import {
  dayMarks,
  formatHM,
  formatRate,
  groupPostsByDay,
  hasViolation,
  isPrivate,
  isScheduled,
  localDayKey,
  monthCells,
  postTime,
  postUrl,
  shiftMonth,
  todayKey,
  violationLabel,
  visibilityLabel,
} from "./calendar";

function post(partial: Partial<DouyinPost>): DouyinPost {
  return {
    id: 0,
    aweme_id: "x",
    title: null,
    cover_path: null,
    cover_url: null,
    status: "published",
    play_count: 0,
    digg_count: 0,
    comment_count: 0,
    share_count: 0,
    completion_rate: null,
    bounce_rate_2s: null,
    violation_status: null,
    violation_text: null,
    violation_brief: null,
    violation_url: null,
    create_time: 0,
    public_time: 0,
    synced_at: "",
    updated_at: "",
    ...partial,
  };
}

/** 本地时间构造 epoch 秒（避免测试依赖特定时区）。 */
function localEpoch(y: number, m: number, d: number, hh = 0, mm = 0): number {
  return Math.floor(new Date(y, m - 1, d, hh, mm).getTime() / 1000);
}

describe("monthCells", () => {
  it("恒为 42 格（6×7）", () => {
    expect(monthCells(2026, 7)).toHaveLength(42);
    expect(monthCells(2026, 1)).toHaveLength(42);
  });

  it("2026-08（1 号为周六）首格为 7/27 周一补位", () => {
    const cells = monthCells(2026, 7);
    expect(cells[0]).toEqual({ key: "2026-07-27", day: 27, inMonth: false });
    // 8/1 周六落在第 6 格（周一开头）。
    expect(cells[5]).toEqual({ key: "2026-08-01", day: 1, inMonth: true });
    expect(cells[41]).toEqual({ key: "2026-09-06", day: 6, inMonth: false });
  });

  it("闰年 2 月与平年 2 月都覆盖全月", () => {
    const leap = monthCells(2024, 1);
    expect(leap.some((c) => c.key === "2024-02-29" && c.inMonth)).toBe(true);
    const plain = monthCells(2026, 1);
    expect(plain.some((c) => c.key === "2026-02-29")).toBe(false);
    expect(plain.some((c) => c.key === "2026-02-28" && c.inMonth)).toBe(true);
  });
});

describe("shiftMonth", () => {
  it("跨年向前", () => {
    expect(shiftMonth(2026, 0, -1)).toEqual({ year: 2025, month: 11 });
  });
  it("跨年向后", () => {
    expect(shiftMonth(2025, 11, 1)).toEqual({ year: 2026, month: 0 });
  });
});

describe("localDayKey / groupPostsByDay", () => {
  it("epoch 边界：23:59 与次日 00:01 落入不同 key", () => {
    expect(localDayKey(localEpoch(2026, 8, 21, 23, 59))).toBe("2026-08-21");
    expect(localDayKey(localEpoch(2026, 8, 22, 0, 1))).toBe("2026-08-22");
  });

  it("按本地日分组且组内时间升序；public_time 优先于 create_time", () => {
    const a = post({ aweme_id: "a", create_time: localEpoch(2026, 8, 21, 9, 0) });
    const b = post({ aweme_id: "b", create_time: localEpoch(2026, 8, 21, 18, 0) });
    // public_time 在未来：即使 create_time 在今天，也应归入 public_time 那天。
    const c = post({
      aweme_id: "c",
      create_time: localEpoch(2026, 8, 21, 8, 0),
      public_time: localEpoch(2026, 8, 25, 12, 0),
    });
    const grouped = groupPostsByDay([b, a, c]);
    expect(grouped.get("2026-08-21")?.map((p) => p.aweme_id)).toEqual(["a", "b"]);
    expect(grouped.get("2026-08-25")?.map((p) => p.aweme_id)).toEqual(["c"]);
  });
});

describe("isScheduled / postTime", () => {
  it("public_time 在未来为定时", () => {
    const future = Math.floor(Date.now() / 1000) + 86400;
    expect(isScheduled(post({ public_time: future }))).toBe(true);
  });
  it("status=scheduled 直接判定", () => {
    expect(isScheduled(post({ status: "scheduled", public_time: 0 }))).toBe(true);
  });
  it("已发布为 false，postTime 回退 create_time", () => {
    const p = post({ status: "published", public_time: 0, create_time: 1785000000 });
    expect(isScheduled(p)).toBe(false);
    expect(postTime(p)).toBe(1785000000);
  });
});

describe("formatHM / formatRate / todayKey", () => {
  it("formatHM 补零", () => {
    expect(formatHM(localEpoch(2026, 8, 21, 9, 5))).toBe("09:05");
    expect(formatHM(localEpoch(2026, 8, 21, 23, 59))).toBe("23:59");
  });
  it("formatRate：缺失显示占位符", () => {
    expect(formatRate(null)).toBe("—");
    expect(formatRate(undefined)).toBe("—");
    expect(formatRate(12.34)).toBe("12.3%");
    expect(formatRate(0)).toBe("0.0%");
  });
  it("todayKey 与 localDayKey(现在) 一致", () => {
    expect(todayKey()).toBe(localDayKey(Math.floor(Date.now() / 1000)));
  });
});

describe("postUrl", () => {
  it("已发布 -> 作品详情页", () => {
    const p = post({ aweme_id: "7671214869823212843", create_time: 1785000000 });
    expect(postUrl(p)).toBe(
      "https://creator.douyin.com/creator-micro/work-management/work-detail/7671214869823212843?enter_from=content",
    );
  });
  it("定时发布 -> 编辑页", () => {
    const future = Math.floor(Date.now() / 1000) + 86400;
    const p = post({ aweme_id: "7672585192384548150", create_time: future });
    expect(postUrl(p)).toBe(
      "https://creator.douyin.com/creator-micro/content/post/video?iid=7672585192384548150&enter_from=manage",
    );
  });
});

describe("hasViolation / violationLabel", () => {
  it("violation_text 非空即违规", () => {
    expect(hasViolation(post({}))).toBe(false);
    expect(hasViolation(post({ violation_text: "" }))).toBe(false);
    expect(hasViolation(post({ violation_text: "作品流量减少，优化后可以获得更多流量" }))).toBe(
      true,
    );
  });
  it("警告文案：摘要首段 + 违规原因", () => {
    const p = post({
      violation_text: "作品流量减少，优化后可以获得更多流量",
      violation_brief: "原创性不足",
    });
    expect(violationLabel(p)).toBe("作品流量减少 · 原创性不足");
  });
  it("缺违规原因时只显示摘要首段", () => {
    expect(violationLabel(post({ violation_text: "作品流量减少，优化后可以获得更多流量" }))).toBe(
      "作品流量减少",
    );
    expect(violationLabel(post({}))).toBe("");
  });
});

describe("isPrivate / visibilityLabel", () => {
  it("status=private 为私密", () => {
    expect(isPrivate(post({ status: "private" }))).toBe(true);
    expect(isPrivate(post({ status: "published" }))).toBe(false);
    expect(visibilityLabel(post({ status: "private" }))).toBe("私密");
    expect(visibilityLabel(post({ status: "published" }))).toBe("公开");
  });
});

describe("dayMarks（私密视频不参与圆点标记）", () => {
  const now = Math.floor(Date.now() / 1000);
  it("公开已发布 -> 蓝点", () => {
    expect(dayMarks([post({ create_time: now - 3600 })], now)).toEqual({
      published: true,
      scheduled: false,
      violating: false,
    });
  });
  it("全天都是私密视频 -> 去掉所有标记", () => {
    const marks = dayMarks(
      [
        post({ status: "private", create_time: now - 3600 }),
        post({ status: "private", create_time: now - 7200, violation_text: "作品流量减少" }),
      ],
      now,
    );
    expect(marks).toEqual({ published: false, scheduled: false, violating: false });
  });
  it("公开 + 私密混合：公开视频仍产生蓝点，私密违规不产生红点", () => {
    const marks = dayMarks(
      [
        post({ create_time: now - 3600 }),
        post({ status: "private", create_time: now - 7200, violation_text: "作品流量减少" }),
      ],
      now,
    );
    expect(marks).toEqual({ published: true, scheduled: false, violating: false });
  });
  it("公开违规 -> 红点；定时 -> 琥珀点（私密与否不影响定时）", () => {
    const marks = dayMarks(
      [
        post({ create_time: now - 3600, violation_text: "作品流量减少" }),
        post({ status: "scheduled", create_time: now + 86400 }),
      ],
      now,
    );
    expect(marks).toEqual({ published: true, scheduled: true, violating: true });
  });
  it("空列表/undefined -> 无标记", () => {
    expect(dayMarks([], now)).toEqual({ published: false, scheduled: false, violating: false });
    expect(dayMarks(undefined, now)).toEqual({ published: false, scheduled: false, violating: false });
  });
});
