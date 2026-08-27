import { describe, expect, it } from "vitest";
import { deriveCategories, filterByCategory, UNCATEGORIZED } from "./musicFilter";

const song = (name: string, categories: string[]) => ({ name, categories });

describe("deriveCategories", () => {
  it("按首次出现顺序去重，保持剪映栏目顺序", () => {
    const files = [
      song("a", ["轻快", "最新"]),
      song("b", ["纯音乐"]),
      song("c", ["轻快"]),
      song("d", []),
    ];
    const r = deriveCategories(files);
    expect(r.categories).toEqual(["轻快", "最新", "纯音乐"]);
    expect(r.hasUncategorized).toBe(true);
  });

  it("全有分类时 hasUncategorized 为 false", () => {
    const r = deriveCategories([song("a", ["轻快"])]);
    expect(r.hasUncategorized).toBe(false);
  });

  it("空列表", () => {
    const r = deriveCategories([]);
    expect(r.categories).toEqual([]);
    expect(r.hasUncategorized).toBe(false);
  });
});

describe("filterByCategory", () => {
  const files = [
    song("a", ["轻快"]),
    song("b", ["纯音乐", "轻快"]),
    song("c", []),
  ];

  it("空分类 = 全部", () => {
    expect(filterByCategory(files, "")).toHaveLength(3);
  });

  it("按分类过滤（一首歌多分类都命中）", () => {
    expect(filterByCategory(files, "轻快").map((f) => f.name)).toEqual(["a", "b"]);
    expect(filterByCategory(files, "纯音乐").map((f) => f.name)).toEqual(["b"]);
  });

  it("未分类 = 仅无分类歌曲", () => {
    expect(filterByCategory(files, UNCATEGORIZED).map((f) => f.name)).toEqual(["c"]);
  });
});
