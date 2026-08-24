import type { DouyinPost } from "../api/types";

/** 日历格：key 为本地日期 YYYY-MM-DD。 */
export interface CalendarCell {
  key: string;
  day: number;
  inMonth: boolean;
}

const pad = (n: number) => String(n).padStart(2, "0");

export function dayKey(year: number, month: number, day: number): string {
  return `${year}-${pad(month + 1)}-${pad(day)}`;
}

/** epoch 秒 -> 本地日期 YYYY-MM-DD。 */
export function localDayKey(epochSec: number): string {
  const d = new Date(epochSec * 1000);
  return dayKey(d.getFullYear(), d.getMonth(), d.getDate());
}

export function todayKey(now: Date = new Date()): string {
  return dayKey(now.getFullYear(), now.getMonth(), now.getDate());
}

/** epoch 秒是否落在本地今天。 */
export function isTodayEpoch(epochSec: number): boolean {
  return localDayKey(epochSec) === todayKey();
}

/** 某月日历的 42 格（6 行 × 7 列），周一为每周第一天，含前后月补位。month 为 0-11。 */
export function monthCells(year: number, month: number): CalendarCell[] {
  // JS getDay() 周日=0，转成周一=0 的偏移。
  const offset = (new Date(year, month, 1).getDay() + 6) % 7;
  const cells: CalendarCell[] = [];
  for (let i = 0; i < 42; i += 1) {
    const d = new Date(year, month, 1 - offset + i);
    cells.push({
      key: dayKey(d.getFullYear(), d.getMonth(), d.getDate()),
      day: d.getDate(),
      inMonth: d.getMonth() === month,
    });
  }
  return cells;
}

/** 月份平移（可跨年）。month 为 0-11，delta 可正可负。 */
export function shiftMonth(
  year: number,
  month: number,
  delta: number,
): { year: number; month: number } {
  const d = new Date(year, month + delta, 1);
  return { year: d.getFullYear(), month: d.getMonth() };
}

/** 作品显示时间：优先 public_time（定时发布为未来时间），缺失回退 create_time。 */
export function postTime(p: DouyinPost): number {
  return p.public_time > 0 ? p.public_time : p.create_time;
}

export function isScheduled(
  p: DouyinPost,
  nowSec: number = Math.floor(Date.now() / 1000),
): boolean {
  return p.status === "scheduled" || postTime(p) > nowSec;
}

/** 按本地日期分组（组内按时间升序）。 */
export function groupPostsByDay(posts: DouyinPost[]): Map<string, DouyinPost[]> {
  const map = new Map<string, DouyinPost[]>();
  for (const p of posts) {
    const t = postTime(p);
    if (!t) continue;
    const key = localDayKey(t);
    const arr = map.get(key) ?? [];
    arr.push(p);
    map.set(key, arr);
  }
  for (const arr of map.values()) {
    arr.sort((a, b) => postTime(a) - postTime(b));
  }
  return map;
}

/** epoch 秒 -> "HH:mm"（本地时区）。 */
export function formatHM(epochSec: number): string {
  const d = new Date(epochSec * 1000);
  return `${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

/** 率类指标（0-100）-> "12.3%"；缺失显示占位符。 */
export function formatRate(v: number | null | undefined): string {
  return v == null ? "—" : `${v.toFixed(1)}%`;
}

/** 作品的创作者中心链接：已发布 -> 详情页；定时/未发布 -> 编辑页。 */
export function postUrl(p: DouyinPost): string {
  return isScheduled(p)
    ? `https://creator.douyin.com/creator-micro/content/post/video?iid=${p.aweme_id}&enter_from=manage`
    : `https://creator.douyin.com/creator-micro/work-management/work-detail/${p.aweme_id}?enter_from=content`;
}

/** 作品是否被平台处罚（流量减少等违规标记）。 */
export function hasViolation(p: DouyinPost): boolean {
  return p.violation_text != null && p.violation_text !== "";
}

/** 作品当前为私密（仅自己可见）。 */
export function isPrivate(p: DouyinPost): boolean {
  return p.status === "private";
}

/** 可见性标签文案。 */
export function visibilityLabel(p: DouyinPost): string {
  return isPrivate(p) ? "私密" : "公开";
}

/** 日历日期圆点标记：私密视频不参与（全天都私密则无任何标记）。 */
export interface DayMarks {
  published: boolean;
  scheduled: boolean;
  violating: boolean;
}

export function dayMarks(
  posts: DouyinPost[] | undefined,
  nowSec: number = Math.floor(Date.now() / 1000),
): DayMarks {
  const list = posts ?? [];
  return {
    published: list.some((p) => !isScheduled(p, nowSec) && !isPrivate(p)),
    scheduled: list.some((p) => isScheduled(p, nowSec)),
    violating: list.some((p) => hasViolation(p) && !isPrivate(p)),
  };
}

/** 违规警告文案：「作品流量减少 · 原创性不足」风格；缺详情时只显示审核摘要首段。 */
export function violationLabel(p: DouyinPost): string {
  if (!hasViolation(p)) return "";
  const head = (p.violation_text ?? "").split(/[，,。]/)[0] || "违规";
  return p.violation_brief ? `${head} · ${p.violation_brief}` : head;
}
