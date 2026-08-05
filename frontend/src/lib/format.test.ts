import { describe, expect, it } from "vitest";
import { formatCount, formatDuration, statusMeta } from "./format";

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
    expect(statusMeta("READY").label).toBe("可发布");
    expect(statusMeta("LOW_PRIORITY").label).toBe("低优先级");
    expect(statusMeta("WAITING_FOR_HUMAN").label).toBe("待人工确认");
  });
});
