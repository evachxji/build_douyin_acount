import { describe, expect, it } from "vitest";
import { cleanConditions, formatCount, formatDateTime, formatDuration, formatSyncAgo, statusMeta } from "./format";

describe("formatDuration", () => {
  it("formats seconds as mm:ss", () => {
    expect(formatDuration(15_000)).toBe("00:15");
    expect(formatDuration(66_000)).toBe("01:06");
    expect(formatDuration(0)).toBe("00:00");
  });
});

describe("formatCount", () => {
  it("uses k for thousands and 万 for tens of thousands", () => {
    expect(formatCount(999)).toBe("999");
    expect(formatCount(1_234)).toBe("1.2k");
    expect(formatCount(12_500)).toBe("1.3万");
    expect(formatCount(1_000_001)).toBe("100.0万");
  });
});

describe("statusMeta", () => {
  it("maps every asset status to a display label", () => {
    expect(statusMeta("NEW").label).toBe("待处理");
    expect(statusMeta("CANDIDATE").label).toBe("候选");
    expect(statusMeta("ELIMINATED").label).toBe("淘汰");
    expect(statusMeta("SELECTED").label).toBe("待发布");
    expect(statusMeta("WAITING_FOR_HUMAN").label).toBe("待人工确认");
  });
});

describe("cleanConditions", () => {
  it("trims whitespace and drops empty rows", () => {
    expect(cleanConditions([" 宠物 ", "", "  ", "风景"])).toEqual(["宠物", "风景"]);
  });
});

/** 造 SQLite UTC 时间串（与库写入格式一致：无 T 无 Z）。 */
const sqliteUtcAgo = (msAgo: number) =>
  new Date(Date.now() - msAgo).toISOString().replace("T", " ").slice(0, 19);

describe("formatSyncAgo", () => {
  it("shows 刚刚 within one minute", () => {
    expect(formatSyncAgo(sqliteUtcAgo(30_000))).toBe("刚刚");
    expect(formatSyncAgo(sqliteUtcAgo(0))).toBe("刚刚");
  });

  it("shows minutes within one hour", () => {
    expect(formatSyncAgo(sqliteUtcAgo(5 * 60_000))).toBe("5分钟前");
    expect(formatSyncAgo(sqliteUtcAgo(59 * 60_000))).toBe("59分钟前");
  });

  it("shows hours within one day, without minutes", () => {
    expect(formatSyncAgo(sqliteUtcAgo(3 * 3_600_000 + 40 * 60_000))).toBe("3小时前");
    expect(formatSyncAgo(sqliteUtcAgo(23 * 3_600_000))).toBe("23小时前");
  });

  it("falls back to absolute date-time beyond one day", () => {
    const v = sqliteUtcAgo(25 * 3_600_000);
    expect(formatSyncAgo(v)).toBe(formatDateTime(v));
  });

  it("treats future timestamps as 刚刚 (clock skew)", () => {
    expect(formatSyncAgo(sqliteUtcAgo(-10 * 60_000))).toBe("刚刚");
    const farFuture = new Date(Date.now() + 3_600_000).toISOString();
    expect(formatSyncAgo(farFuture)).toBe("刚刚");
  });

  it("shows placeholder for empty input", () => {
    expect(formatSyncAgo(null)).toBe("—");
    expect(formatSyncAgo(undefined)).toBe("—");
  });
});
