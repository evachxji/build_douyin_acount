import { useEffect, useRef, useState } from "react";
import { api } from "../../api/client";
import type {
  WhisperDownloadRoute,
  WhisperModelInfo,
} from "../../api/types";
import { formatBytes, formatSpeed } from "../../lib/format";

const STAGE_LABEL: Record<string, string> = {
  connecting: "连接中",
  downloading: "下载中",
  verifying: "校验文件",
  done: "已完成",
  error: "下载失败",
  cancelled: "已取消",
  interrupted: "已中断",
};

const ACTIVE_STAGES = ["connecting", "downloading", "verifying"];

function routeCn(route: string): string {
  return route === "mirror" ? "国内镜像" : "官方源";
}

/**
 * Whisper 模型下载弹窗：未安装模型的引导 + 一键下载 + 实时进度
 * （路由/速率/阶段/文件路径）。进度数据由父组件轮询后经 info 传入。
 */
export function WhisperModelDialog({
  modelName,
  info,
  onClose,
  onInstalled,
}: {
  modelName: string;
  info: WhisperModelInfo | null;
  onClose: () => void;
  /** 下载完成时回调一次（父组件把该模型写进设置）。 */
  onInstalled: (name: string) => void;
}) {
  const [route, setRoute] = useState<WhisperDownloadRoute>("auto");
  const [starting, setStarting] = useState(false);
  const [startError, setStartError] = useState<string | null>(null);
  const notifiedRef = useRef(false);

  const progress = info?.download ?? null;
  const stage = progress?.stage ?? null;
  const active = stage !== null && ACTIVE_STAGES.includes(stage);
  const pct =
    progress && progress.total_bytes > 0
      ? Math.min(100, Math.floor((progress.total_downloaded / progress.total_bytes) * 100))
      : 0;

  useEffect(() => {
    if (stage === "done" && !notifiedRef.current) {
      notifiedRef.current = true;
      onInstalled(modelName);
    }
  }, [stage, modelName, onInstalled]);

  const start = async () => {
    setStarting(true);
    setStartError(null);
    try {
      await api.startWhisperDownload(modelName, route);
    } catch (e) {
      setStartError(e instanceof Error ? e.message : String(e));
    } finally {
      setStarting(false);
    }
  };

  const cancel = async () => {
    try {
      await api.cancelWhisperDownload(modelName);
    } catch {
      /* 已自行结束（完成/失败竞态），进度区会随下一次轮询刷新 */
    }
  };

  const row = "flex items-baseline justify-between gap-3 text-xs";
  const label = "shrink-0 text-mute";
  const value = "min-w-0 break-all text-right text-paper";

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 backdrop-blur-sm">
      <div className="w-[520px] rounded-xl border border-accent/40 bg-panel p-6 shadow-2xl">
        <div className="flex items-center justify-between">
          <h3 className="text-sm font-bold text-paper">
            Whisper 模型：{modelName}
            {stage && (
              <span
                className={`ml-2 text-xs font-normal ${
                  stage === "done"
                    ? "text-emerald-400"
                    : stage === "error"
                      ? "text-bad"
                      : active
                        ? "text-amber-400"
                        : "text-mute"
                }`}
              >
                {STAGE_LABEL[stage] ?? stage}
              </span>
            )}
          </h3>
          <button onClick={onClose} className="text-sm text-mute hover:text-paper">
            ✕
          </button>
        </div>

        <div className="mt-4 space-y-1.5">
          {info && (
            <>
              <div className={row}>
                <span className={label}>模型规格</span>
                <span className={value}>
                  约 {info.size_mb}MB · {info.vram_hint} · {info.file_count} 个文件
                </span>
              </div>
              <div className={row}>
                <span className={label}>保存位置</span>
                <span className={value}>{info.dir}</span>
              </div>
            </>
          )}
          {progress && (
            <div className={row}>
              <span className={label}>下载路由</span>
              <span className={value}>{routeCn(progress.route)}</span>
            </div>
          )}
        </div>

        {active && progress && (
          <div className="mt-4">
            <div className="h-2 w-full overflow-hidden rounded bg-ink">
              <div
                className="h-full rounded bg-accent transition-[width] duration-500"
                style={{ width: `${pct}%` }}
              />
            </div>
            <div className="mt-2 space-y-1.5">
              <div className={row}>
                <span className={label}>总进度</span>
                <span className={value}>
                  {formatBytes(progress.total_downloaded)} / {formatBytes(progress.total_bytes)}（{pct}%）
                  <span className="ml-2 text-accent">{formatSpeed(progress.speed_bps)}</span>
                </span>
              </div>
              <div className={row}>
                <span className={label}>当前文件</span>
                <span className={value}>
                  {progress.current_file || "—"}
                  {progress.file_index > 0 && `（${progress.file_index}/${progress.file_count}）`}
                  {progress.file_total > 0 &&
                    ` ${formatBytes(progress.file_downloaded)}/${formatBytes(progress.file_total)}`}
                </span>
              </div>
              {progress.url && (
                <div className={row}>
                  <span className={label}>下载地址</span>
                  <span className={value}>{progress.url}</span>
                </div>
              )}
            </div>
          </div>
        )}

        {progress?.message && (
          <p
            className={`mt-3 text-xs ${
              stage === "error" ? "text-bad" : stage === "done" ? "text-emerald-400" : "text-mute"
            }`}
          >
            {progress.message}
          </p>
        )}
        {startError && <p className="mt-3 text-xs text-bad">{startError}</p>}

        {!active && stage !== "done" && (
          <div className="mt-4">
            <span className="text-xs text-mute">下载源</span>
            <div className="mt-1.5 space-y-1 text-xs text-paper">
              {(
                [
                  ["auto", "自动（官方源优先，失败或速度过慢时自动切换国内镜像）"],
                  ["official", "官方源（huggingface.co，国外网络不佳时可能很慢）"],
                  ["mirror", "国内镜像（hf-mirror.com）"],
                ] as [WhisperDownloadRoute, string][]
              ).map(([v, text]) => (
                <label key={v} className="flex items-center gap-2">
                  <input
                    type="radio"
                    name="whisper-route"
                    checked={route === v}
                    onChange={() => setRoute(v)}
                  />
                  {text}
                </label>
              ))}
            </div>
          </div>
        )}

        <div className="mt-5 flex gap-2">
          <button
            onClick={onClose}
            className="h-9 flex-1 rounded border border-line text-sm text-mute hover:text-paper"
          >
            {active ? "后台继续" : "关闭"}
          </button>
          {active ? (
            <button
              onClick={() => void cancel()}
              className="h-9 flex-1 rounded border border-bad/60 text-sm text-bad hover:bg-bad/10"
            >
              取消下载（保留断点）
            </button>
          ) : stage === "done" ? (
            <button
              onClick={onClose}
              className="h-9 flex-1 rounded border border-accent bg-accent text-sm font-semibold text-ink hover:brightness-110"
            >
              完成
            </button>
          ) : (
            <button
              disabled={starting || info?.installed}
              onClick={() => void start()}
              className="h-9 flex-1 rounded border border-accent bg-accent text-sm font-semibold text-ink hover:brightness-110 disabled:opacity-50"
            >
              {starting
                ? "启动中…"
                : stage === "cancelled" || stage === "interrupted"
                  ? "继续下载（断点续传）"
                  : stage === "error"
                    ? "重试下载"
                    : "立即下载"}
            </button>
          )}
        </div>
        {!active && stage !== "done" && (
          <p className="mt-2 text-[11px] leading-relaxed text-mute">
            下载在后台进行，关闭此窗口不中断；全局同时只能下载一个模型。
          </p>
        )}
      </div>
    </div>
  );
}
