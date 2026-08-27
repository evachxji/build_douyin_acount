import { useEffect, useRef, useState } from "react";
import { api } from "../../api/client";
import type { MusicCollectProgress } from "../../api/types";
import { formatBytes } from "../../lib/format";

const TERMINAL = new Set(["done", "stopped", "error", "interrupted"]);
type Phase = "idle" | "starting" | "running" | "stopping";

/** 音乐库批量采集卡片（设置页）：从剪映线上音乐库按分类各采 N 首到本地音乐库
 * （不依赖剪映客户端）。专用线程 + progress 文件轮询（1.5s，非 idle 期间）；
 * 可停止、幂等续采；完成后自动触发封面补下载并刷新「曲库 N 首」计数。
 * 状态机：idle → starting（启动中 loading，等后端初始进度）→ running（显示进度）；
 * 轮询拿到终态回 idle。挂载时若采集正在进行（重开设置页）自动接续进度显示。 */
export function MusicCollectCard() {
  const [perCategory, setPerCategory] = useState(50);
  const [progress, setProgress] = useState<MusicCollectProgress | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [phase, setPhase] = useState<Phase>("idle");
  const [musicCount, setMusicCount] = useState<number | null>(null);
  // 完成后的封面补下载 + 计数刷新只触发一次（以 updated_at 去重）。
  const doneHandledFor = useRef<number>(0);

  const refreshCount = () => {
    void api
      .getMusicStatus()
      .then((s) => setMusicCount(s.count))
      .catch(() => setMusicCount(null));
  };

  // 挂载时读一次：采集正在进行（重开设置页/另一入口发起）→ 接续轮询显示进度。
  useEffect(() => {
    refreshCount();
    void api
      .musicCollectStatus()
      .then((p) => {
        setProgress(p);
        if (p && (p.stage === "fetching" || p.stage === "downloading")) {
          setPhase("running");
        }
      })
      .catch(() => {});
  }, []);

  // 非 idle 期间 1.5s 轮询；运行态确认/终态回收驱动状态机。
  useEffect(() => {
    if (phase === "idle") return;
    let cancelled = false;
    const tick = () => {
      void api
        .musicCollectStatus()
        .then((p) => {
          if (cancelled || !p) return;
          setProgress(p);
          if (p.stage === "fetching" || p.stage === "downloading") {
            setPhase((cur) => (cur === "starting" ? "running" : cur));
          } else if (TERMINAL.has(p.stage)) {
            // starting 阶段读到旧终态残留（后端 start 前已覆盖写 fetching，理论不会）：
            // 保持 starting 继续等，不误判结束。
            setPhase((cur) => (cur === "starting" ? cur : "idle"));
          }
        })
        .catch(() => {});
    };
    tick();
    const timer = setInterval(tick, 1500);
    return () => {
      cancelled = true;
      clearInterval(timer);
    };
  }, [phase]);

  // 采集结束（done/stopped）且有实际下载 → 补封面 + 刷新「曲库 N 首」。
  useEffect(() => {
    if (!progress || (progress.stage !== "done" && progress.stage !== "stopped")) return;
    if (progress.downloaded === 0) return;
    if (doneHandledFor.current === progress.updated_at) return;
    doneHandledFor.current = progress.updated_at;
    void api.ensureMusicCovers().catch(() => {});
    refreshCount();
  }, [progress]);

  const start = async () => {
    setError(null);
    setPhase("starting");
    try {
      await api.startMusicCollect(perCategory);
      // 后端 start 返回前已同步写入初始 fetching 进度，轮询 effect 接手状态机。
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      if (msg.includes("进行中")) {
        // 采集实际已在跑（如另一设置页实例发起）：不报错，直接接续进度显示。
        setPhase("running");
      } else {
        setPhase("idle");
        setError(msg);
      }
    }
  };

  const stop = async () => {
    setError(null);
    try {
      await api.stopMusicCollect();
      setPhase("stopping");
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const openDir = async () => {
    setError(null);
    try {
      await api.openMusicDir();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const percent =
    progress && progress.planned_total > 0
      ? Math.min(
          100,
          Math.round(
            ((progress.downloaded + progress.failed) / progress.planned_total) * 100,
          ),
        )
      : 0;
  const live = progress?.stage === "fetching" || progress?.stage === "downloading";

  return (
    <div className="col-span-2 rounded border border-sky-400/30 bg-sky-400/10 p-3">
      <div className="flex items-center justify-between">
        <p className="flex items-center text-xs text-mute">
          <span className="font-semibold uppercase tracking-wider">音乐库采集</span>
          <span className="ml-3">曲库：{musicCount === null ? "…" : `${musicCount} 首`}</span>
          <span className="ml-3" title="从剪映线上音乐库按分类批量下载（不依赖剪映客户端）">
            按剪映分类各采
          </span>
          <input
            type="number"
            min={1}
            max={200}
            value={perCategory}
            disabled={phase !== "idle"}
            onChange={(e) =>
              setPerCategory(Math.max(1, Math.min(200, Number(e.target.value) || 50)))
            }
            className="mx-1.5 w-14 rounded border border-line bg-ink/40 px-1.5 py-0.5 text-center text-xs text-paper outline-none focus:border-accent/60 disabled:opacity-50"
          />
          <span>首</span>
        </p>
        <div className="flex gap-2">
          <button
            type="button"
            onClick={() => void openDir()}
            className="h-7 rounded border border-line px-3 text-xs text-mute hover:text-paper"
          >
            打开目录
          </button>
          {phase === "idle" && (
            <button
              type="button"
              onClick={() => void start()}
              className="h-7 rounded border border-accent bg-accent px-3 text-xs font-semibold text-ink hover:brightness-110"
            >
              开始采集
            </button>
          )}
          {phase === "starting" && (
            <button
              type="button"
              disabled
              className="inline-flex h-7 items-center gap-1.5 rounded border border-accent bg-accent px-3 text-xs font-semibold text-ink opacity-70"
            >
              <span className="h-3 w-3 animate-spin rounded-full border-2 border-ink/40 border-t-ink" />
              启动中…
            </button>
          )}
          {phase === "running" && (
            <button
              type="button"
              onClick={() => void stop()}
              className="h-7 rounded border border-bad/50 px-3 text-xs font-semibold text-bad hover:bg-bad/10"
            >
              停止
            </button>
          )}
          {phase === "stopping" && (
            <button
              type="button"
              disabled
              className="inline-flex h-7 items-center gap-1.5 rounded border border-bad/50 px-3 text-xs font-semibold text-bad opacity-70"
            >
              <span className="h-3 w-3 animate-spin rounded-full border-2 border-bad/40 border-t-bad" />
              停止中…
            </button>
          )}
        </div>
      </div>

      {progress && (
        <div className="mt-2 space-y-1.5 text-xs text-mute">
          {live && (
            <div className="flex items-center gap-2">
              <div className="h-2 flex-1 overflow-hidden rounded bg-white/10">
                <div
                  className={`h-full rounded bg-accent transition-all ${
                    progress.stage === "fetching" ? "w-1/3 animate-pulse" : ""
                  }`}
                  style={
                    progress.stage === "downloading" ? { width: `${percent}%` } : undefined
                  }
                />
              </div>
              {progress.stage === "downloading" && (
                <span className="shrink-0 tabular-nums text-paper/85">{percent}%</span>
              )}
            </div>
          )}
          <p>
            {progress.stage === "fetching" &&
              `获取列表中…（分类 ${progress.category_index}/${progress.category_count} ${progress.category}）`}
            {progress.stage === "downloading" &&
              `下载中 ${progress.downloaded + progress.failed}/${progress.planned_total} 首 · 分类 ${progress.category_index}/${progress.category_count} · 已下载 ${progress.downloaded} · 跳过 ${progress.skipped}${
                progress.failed > 0 ? ` · 失败 ${progress.failed}` : ""
              }`}
            {(progress.stage === "done" ||
              progress.stage === "stopped" ||
              progress.stage === "error" ||
              progress.stage === "interrupted") && (
              <span
                className={
                  progress.stage === "error" || progress.stage === "interrupted"
                    ? "text-bad"
                    : "text-emerald-400"
                }
              >
                {progress.message}
              </span>
            )}
          </p>
          {progress.current_song && progress.stage === "downloading" && (
            <p className="truncate text-mute/80" title={progress.current_song}>
              当前：{progress.current_song}（已 {formatBytes(progress.bytes_downloaded)}）
            </p>
          )}
        </div>
      )}
      {error && <p className="mt-2 text-xs text-bad">{error}</p>}
    </div>
  );
}
