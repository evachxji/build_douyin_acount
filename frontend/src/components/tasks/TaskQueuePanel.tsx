import { useEffect, useRef, useState } from "react";
import { api } from "../../api/client";
import { useTaskStore } from "../../stores/taskStore";
import { useSelectionStore } from "../../stores/selectionStore";
import { useAssetsStore } from "../../stores/assetsStore";
import { StatusBadge } from "../StatusBadge";
import {
  agentLabel,
  formatCount,
  formatDateTime,
  platformLabel,
  taskTypeLabel,
} from "../../lib/format";
import type { AgentTask, VideoAsset } from "../../api/types";

const TERMINAL_STATUSES = ["SUCCEEDED", "FAILED", "CANCELLED"];
const ACTIVE_STATUSES = ["PENDING", "RUNNING", "WAITING_FOR_HUMAN"];

function TaskResult({ task, asset }: { task: AgentTask; asset?: VideoAsset }) {
  if (task.status !== "SUCCEEDED") {
    // 采集任务进行中：显示 sidecar 透传的实时进度（已下载 x/y + 当前阶段）。
    if (task.agent_name === "collector" && task.status === "RUNNING") {
      const p = (task.payload.progress ?? {}) as {
        downloaded?: number;
        target?: number;
        message?: string;
      };
      const text = [
        p.target ? `已下载 ${p.downloaded ?? 0}/${p.target}` : null,
        p.message || null,
      ]
        .filter(Boolean)
        .join(" · ");
      return (
        <span className="min-w-0 flex-1 truncate text-paper/85" title={text}>
          {text || "采集中…"}
        </span>
      );
    }
    return task.error ? (
      <span className="min-w-0 flex-1 truncate text-bad" title={task.error}>
        {task.error}
      </span>
    ) : (
      <span className="min-w-0 flex-1" />
    );
  }
  const payload = task.payload;
  if (task.agent_name === "vision" || task.agent_name === "filter") {
    const parts = [
      asset?.category,
      ...(asset?.tags ?? []),
      asset?.quality_score != null ? `${asset.quality_score}分` : null,
    ]
      .filter(Boolean)
      .slice(0, 4);
    const text = parts.join(" · ");
    return (
      <span className="min-w-0 flex-1 truncate text-paper/85" title={text}>
        {text || "分析完成"}
      </span>
    );
  }
  if (task.agent_name === "collector") {
    const first = payload.first_asset as
      | { title?: string; author?: string; likes?: number }
      | null
      | undefined;
    const imported = Number(payload.imported ?? 0);
    const text = [
      `已采集 ${imported} 条`,
      payload.platform ? `（${platformLabel(String(payload.platform))}）` : null,
      first?.title ? ` · ${first.title}` : null,
      first?.author ? ` · ${first.author}` : null,
      first?.likes != null ? ` · ♥${formatCount(first.likes)}` : null,
    ]
      .filter(Boolean)
      .join("");
    return (
      <span className="min-w-0 flex-1 truncate text-paper/85" title={text}>
        {text}
      </span>
    );
  }
  if (task.agent_name === "publisher") {
    const title =
      asset?.title ?? (payload.title ? String(payload.title) : `视频 #${payload.asset_id ?? "?"}`);
    return (
      <span className="min-w-0 flex-1 truncate text-paper/85" title={`已发布 · ${title}`}>
        已发布 · {title}
      </span>
    );
  }
  if (task.agent_name === "content") {
    const title = asset?.title ?? `视频 #${payload.asset_id ?? "?"}`;
    return (
      <span className="min-w-0 flex-1 truncate text-paper/85" title={`文案已生成 · ${title}`}>
        文案已生成 · {title}
      </span>
    );
  }
  return (
    <span className="min-w-0 flex-1 truncate text-paper/85">
      {asset?.title ?? `视频 #${payload.asset_id ?? "?"}`}
    </span>
  );
}

export function TaskQueuePanel({ height }: { height: number }) {
  const tasks = useTaskStore((s) => s.tasks);
  const error = useTaskStore((s) => s.error);
  const setOpen = useTaskStore((s) => s.setOpen);
  const cancel = useTaskStore((s) => s.cancel);
  const focus = useSelectionStore((s) => s.focus);
  const setFilter = useAssetsStore((s) => s.setFilter);
  // 独立于当前筛选列表的素材缓存：结果列展示与左侧筛选条件互不影响。
  const assetCacheRef = useRef(new Map<number, VideoAsset>());
  const fetchingRef = useRef(new Set<number>());
  const [, setTick] = useState(0);

  // 任务列表变化时，异步补齐缺失的素材信息（每任务仅查一次，失败不重试）。
  useEffect(() => {
    const missing = tasks
      .map((t) => Number(t.payload.asset_id))
      .filter((id) => Number.isInteger(id) && !assetCacheRef.current.has(id) && !fetchingRef.current.has(id));
    if (missing.length === 0) return;
    missing.forEach((id) => {
      fetchingRef.current.add(id);
      void api
        .getAsset(id)
        .then((a) => {
          if (a) {
            assetCacheRef.current.set(id, a);
            setTick((n) => n + 1);
          }
        })
        .catch(() => {})
        .finally(() => fetchingRef.current.delete(id));
    });
  }, [tasks]);

  const handleLocateAsset = async (t: (typeof tasks)[number]) => {
    const assetId = Number(t.payload.asset_id);
    if (!Number.isInteger(assetId)) return;
    let asset: VideoAsset | null = assetCacheRef.current.get(assetId) ?? null;
    if (!asset) {
      try {
        asset = await api.getAsset(assetId);
        if (asset) assetCacheRef.current.set(assetId, asset);
      } catch {
        // 素材可能已被删除，走不到定位逻辑。
      }
    }
    if (!asset) return;
    // 切换到该素材所在状态，并清掉搜索，确保目标可见。
    // 分析中归入"待处理"（左侧栏无独立的分析中筛选项）。
    const status = asset.status === "ANALYZING" ? "NEW" : asset.status;
    await setFilter({ status, search: undefined });
    focus(assetId);
    window.dispatchEvent(new CustomEvent("asset-focus", { detail: assetId }));
  };

  return (
    <section
      style={{ height }}
      className="flex shrink-0 flex-col border-t border-line bg-panel"
    >
      <header className="flex h-9 items-center gap-3 border-b border-line px-4">
        <span className="text-xs font-semibold uppercase tracking-wider text-mute">
          Agent 任务队列
        </span>
        <span className="text-[11px] text-mute">
          {tasks.filter((t) => t.status === "PENDING" || t.status === "RUNNING").length} 进行中
        </span>
        {error && <span className="text-[11px] text-bad">刷新失败：{error}</span>}
        <button
          onClick={() => setOpen(false)}
          className="ml-auto text-sm text-mute hover:text-paper"
          title="收起"
        >
          ▾
        </button>
      </header>
      <div className="flex items-center gap-3 border-b border-line px-2 py-1.5 text-[11px] text-mute">
        <span className="w-16 shrink-0">#ID</span>
        <span className="w-14 shrink-0">Agent</span>
        <span className="w-24 shrink-0">任务类型</span>
        <span className="w-16 shrink-0">素材</span>
        <span className="w-10 shrink-0 tabular-nums">尝试</span>
        <span className="w-20 shrink-0">创建时间</span>
        <span className="w-20 shrink-0">结束时间</span>
        <span className="w-20 shrink-0">素材状态</span>
        <span className="w-20 shrink-0">状态</span>
        <span className="w-14 shrink-0">操作</span>
        <span className="min-w-0 flex-1">结果</span>
      </div>
      <ul className="min-h-0 flex-1 overflow-y-auto p-2">
        {tasks.length === 0 && (
          <li className="px-2 py-6 text-center text-xs text-mute">暂无任务</li>
        )}
        {tasks.slice(0, 30).map((t) => {
          const cached = assetCacheRef.current.get(Number(t.payload.asset_id));
          return (
            <li
              key={t.id}
              onDoubleClick={() => handleLocateAsset(t)}
              title="双击定位对应视频"
              className="flex items-center gap-3 rounded px-2 py-1.5 text-xs hover:bg-white/5"
            >
              <span className="w-16 shrink-0 text-mute">#{t.id}</span>
              <span className="w-14 shrink-0 text-paper/85">{agentLabel(t.agent_name)}</span>
              <span className="w-24 shrink-0 text-paper/85">{taskTypeLabel(t.task_type)}</span>
              <span
                className="w-32 shrink-0 truncate text-mute"
                title={cached?.title ?? `素材 #${String(t.payload.asset_id ?? "")}`}
              >
                {cached?.title ?? `素材 #${String(t.payload.asset_id ?? "—")}`}
              </span>
              <span className="w-10 shrink-0 tabular-nums text-mute">
                {t.attempts}/{t.max_attempts}
              </span>
              <span className="w-20 shrink-0 tabular-nums text-mute">
                {formatDateTime(t.created_at)}
              </span>
              <span className="w-20 shrink-0 tabular-nums text-mute">
                {TERMINAL_STATUSES.includes(t.status)
                  ? formatDateTime(t.updated_at)
                  : "—"}
              </span>
              <span className="w-20 shrink-0">
                {cached ? (
                  <StatusBadge status={cached.status} />
                ) : (
                  <span className="text-mute">—</span>
                )}
              </span>
              <span className="w-20 shrink-0">
                <StatusBadge status={t.status} />
              </span>
              <span className="w-14 shrink-0">
                {ACTIVE_STATUSES.includes(t.status) ? (
                  <button
                    onClick={() => void cancel(t.id)}
                    className="rounded border border-bad/40 px-1.5 py-0.5 text-[11px] text-bad transition-colors hover:bg-bad/10"
                  >
                    取消
                  </button>
                ) : null}
              </span>
              <TaskResult task={t} asset={cached} />
            </li>
          );
        })}
      </ul>
    </section>
  );
}
