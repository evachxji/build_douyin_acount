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
  };
  return map[agent] ?? agent;
}

/** 任务类型 -> 中文描述。 */
export function taskTypeLabel(type: string): string {
  const map: Record<string, string> = {
    analyze: "视频分析",
    prepare_publish: "准备发布",
    collect_batch: "批量采集",
  };
  return map[type] ?? type;
}
