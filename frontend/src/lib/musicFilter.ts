/** 添加音乐弹窗的分类栏目 derive 与过滤（纯函数，vitest 守护）。 */

/** 「未分类」栏目名：无分类映射的歌曲（剪映缓存同步来的/手动放进目录的）。 */
export const UNCATEGORIZED = "未分类";

/** 从音乐列表 derive 分类栏目：按各文件 categories 首次出现顺序
 * （后端 list_music_files 已按剪映栏目 ordinal 排好序，derive 保持该顺序）。 */
export function deriveCategories(files: { categories: string[] }[]): {
  categories: string[];
  hasUncategorized: boolean;
} {
  const seen = new Set<string>();
  const categories: string[] = [];
  let hasUncategorized = false;
  for (const f of files) {
    if (f.categories.length === 0) hasUncategorized = true;
    for (const c of f.categories) {
      if (!seen.has(c)) {
        seen.add(c);
        categories.push(c);
      }
    }
  }
  return { categories, hasUncategorized };
}

/** 按选中分类过滤（"" = 全部；UNCATEGORIZED = 仅无分类歌曲）。 */
export function filterByCategory<T extends { categories: string[] }>(
  files: T[],
  active: string,
): T[] {
  if (active === "") return files;
  if (active === UNCATEGORIZED) return files.filter((f) => f.categories.length === 0);
  return files.filter((f) => f.categories.includes(active));
}
