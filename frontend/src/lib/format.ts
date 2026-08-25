/** 毫秒时长 -> mm:ss */
export function formatDuration(ms: number): string {
  const totalSecs = Math.max(0, Math.round(ms / 1000));
  const m = Math.floor(totalSecs / 60);
  const s = totalSecs % 60;
  return `${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
}

/** 点赞/评论数：万、k 缩写 */
export function formatCount(n: number): string {
  if (n >= 10_000) {
    return `${(n / 10_000).toFixed(1)}万`;
  }
  if (n >= 1_000) {
    return `${(n / 1_000).toFixed(1)}k`;
  }
  return String(n);
}

export interface StatusMeta {
  label: string;
  tone: "neutral" | "running" | "ready" | "selected" | "published" | "warn" | "danger";
}

const STATUS_META: Record<string, StatusMeta> = {
  NEW: { label: "待处理", tone: "neutral" },
  ANALYZING: { label: "分析中", tone: "running" },
  CANDIDATE: { label: "候选", tone: "ready" },
  SELECTED: { label: "待发布", tone: "selected" },
  PUBLISHED: { label: "已发布", tone: "published" },
  ELIMINATED: { label: "淘汰", tone: "warn" },
  DOWNLOAD_FAILED: { label: "下载失败", tone: "danger" },
  PENDING: { label: "排队中", tone: "neutral" },
  RUNNING: { label: "执行中", tone: "running" },
  WAITING_FOR_HUMAN: { label: "待人工确认", tone: "danger" },
  SUCCEEDED: { label: "已完成", tone: "ready" },
  FAILED: { label: "失败", tone: "danger" },
  CANCELLED: { label: "已取消", tone: "neutral" },
};

export function statusMeta(status: string): StatusMeta {
  return STATUS_META[status] ?? { label: status, tone: "neutral" };
}

export function platformLabel(platform: string): string {
  const map: Record<string, string> = {
    tiktok: "TikTok",
    instagram: "IG",
    local: "本地",
  };
  return map[platform.toLowerCase()] ?? platform;
}

/** Agent 名称 -> 中文描述（任务队列/审批弹窗展示用）。 */
export function agentLabel(agent: string): string {
  const map: Record<string, string> = {
    vision: "AI 分析",
    publisher: "抖音发布",
    collector: "素材采集",
    content: "文案生成",
    douyin_sync: "数据同步",
    douyin_moderate: "违规处理",
  };
  return map[agent] ?? agent;
}

/** 任务类型 -> 中文描述。 */
export function taskTypeLabel(type: string): string {
  const map: Record<string, string> = {
    analyze: "视频分析",
    prepare_publish: "准备发布",
    collect_batch: "批量采集",
    generate_copy: "生成文案",
    sync_works: "作品同步",
    moderate_violation: "违规处理",
  };
  return map[type] ?? type;
}

/** SQLite 或 RFC3339 时间 -> 本地时区 "MM-DD HH:mm"；空值显示占位符。 */
export function formatDateTime(v: string | null | undefined): string {
  if (!v) return "—";
  // SQLite datetime('now') 输出 UTC 且无时区标记，补 Z 后统一按 UTC 解析。
  const iso = v.includes("T") ? v : `${v.replace(" ", "T")}Z`;
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return v;
  const p = (n: number) => String(n).padStart(2, "0");
  return `${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`;
}

/** 同步时间的相对显示：1 分钟内"刚刚"、1 小时内"xx分钟前"、24 小时内"xx小时前"，超出回退 formatDateTime。 */
export function formatSyncAgo(v: string | null | undefined): string {
  if (!v) return "—";
  const iso = v.includes("T") ? v : `${v.replace(" ", "T")}Z`;
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return v;
  const minutes = Math.floor((Date.now() - d.getTime()) / 60000);
  if (minutes < 1) return "刚刚"; // 含未来时间（时钟偏差）
  if (minutes < 60) return `${minutes}分钟前`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}小时前`;
  return formatDateTime(v);
}

/** 筛选条件列表清洗：去首尾空白、丢弃空行。 */
export function cleanConditions(items: string[]): string[] {
  return items.map((s) => s.trim()).filter((s) => s.length > 0);
}

/** 字节数 -> 人类可读（GB 两位小数 / MB 一位小数）。 */
export function formatBytes(n: number): string {
  if (n >= 1_000_000_000) return `${(n / 1_000_000_000).toFixed(2)} GB`;
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)} MB`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(0)} KB`;
  return `${n} B`;
}

/** 字节/秒 -> 人类可读速率。 */
export function formatSpeed(bps: number): string {
  return `${formatBytes(bps)}/s`;
}
