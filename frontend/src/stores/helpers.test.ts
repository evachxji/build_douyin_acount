import { describe, expect, it } from "vitest";
import { buildAssetFilter, selectionReducer } from "./helpers";

describe("selectionReducer", () => {
  it("toggles ids and clears the set", () => {
    let set = selectionReducer(new Set<number>(), { type: "toggle", id: 3 });
    expect(set.has(3)).toBe(true);
    set = selectionReducer(set, { type: "toggle", id: 3 });
    expect(set.has(3)).toBe(false);

    set = selectionReducer(set, { type: "replace", ids: [1, 2, 3] });
    expect(set.size).toBe(3);
    set = selectionReducer(set, { type: "clear" });
    expect(set.size).toBe(0);
  });
});

describe("buildAssetFilter", () => {
  it("drops undefined fields and passes through the rest", () => {
    const filter = buildAssetFilter({
      category: "宠物",
      status: undefined,
      platform: "instagram",
      favorite: true,
      search: "cat",
    });
    expect(filter).toEqual({
      category: "宠物",
      platform: "instagram",
      favorite: true,
      search: "cat",
      limit: 300,
      offset: 0,
    });
    expect("status" in filter).toBe(false);
  });
});
