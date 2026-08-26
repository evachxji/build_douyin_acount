import { describe, expect, it } from "vitest";
import {
  buildAssetFilter,
  loadPersistedFilterStatus,
  persistFilterStatus,
  selectionReducer,
} from "./helpers";

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
      status: undefined,
      platform: "instagram",
      search: "cat",
    });
    expect(filter).toEqual({
      platform: "instagram",
      search: "cat",
      limit: 300,
      offset: 0,
    });
    expect("status" in filter).toBe(false);
  });

  it("maps 待处理(NEW) to NEW+ANALYZING and other statuses to a single entry", () => {
    expect(buildAssetFilter({ status: "NEW" }).statuses).toEqual(["NEW", "ANALYZING"]);
    expect(buildAssetFilter({ status: "CANDIDATE" }).statuses).toEqual(["CANDIDATE"]);
  });
});

/** Map 实现的假 Storage，避免测试环境依赖 localStorage。 */
function fakeStorage() {
  const map = new Map<string, string>();
  return {
    getItem: (k: string) => map.get(k) ?? null,
    setItem: (k: string, v: string) => void map.set(k, v),
    removeItem: (k: string) => void map.delete(k),
  };
}

describe("filter status persistence", () => {
  it("remembers the last status and clears it when back to 全部(undefined)", () => {
    const storage = fakeStorage();
    expect(loadPersistedFilterStatus(storage)).toBeUndefined();

    persistFilterStatus("CANDIDATE", storage);
    expect(loadPersistedFilterStatus(storage)).toBe("CANDIDATE");

    persistFilterStatus(undefined, storage);
    expect(loadPersistedFilterStatus(storage)).toBeUndefined();
  });
});
