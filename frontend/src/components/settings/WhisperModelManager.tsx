import { useState } from "react";
import { ask } from "@tauri-apps/plugin-dialog";
import { api } from "../../api/client";
import type { WhisperModelInfo } from "../../api/types";

const ACTIVE_STAGES = ["connecting", "downloading", "verifying"];

/**
 * Whisper 模型管理弹窗（设置页下拉框旁齿轮按钮打开）：
 * 全部模型的安装状态总览 + 下载/删除；下载细节在 WhisperModelDialog 中展示。
 */
export function WhisperModelManager({
  models,
  onClose,
  onOpenDownload,
  onRefresh,
}: {
  models: WhisperModelInfo[];
  onClose: () => void;
  /** 打开某模型的下载弹窗（新下载/继续/查看进度）。 */
  onOpenDownload: (name: string) => void;
  /** 删除完成后让父组件立即刷新列表（轮询之外的手动刷新）。 */
  onRefresh: () => void;
}) {
  const [error, setError] = useState<string | null>(null);

  const deleteModel = async (name: string) => {
    const ok = await ask(`确定删除模型 ${name} 吗？删除后需重新下载才能用于语音识别。`, {
      title: "删除 Whisper 模型",
      kind: "warning",
    });
    if (!ok) return;
    try {
      await api.deleteWhisperModel(name);
      onRefresh();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 backdrop-blur-sm">
      <div className="w-[560px] rounded-xl border border-accent/40 bg-panel p-6 shadow-2xl">
        <div className="flex items-center justify-between">
          <h3 className="text-sm font-bold text-paper">Whisper 模型管理（内置模型库）</h3>
          <button onClick={onClose} className="text-sm text-mute hover:text-paper">
            ✕
          </button>
        </div>

        <div className="mt-4 space-y-2">
          {models.map((m) => {
            const d = m.download;
            const active = d !== null && ACTIVE_STAGES.includes(d.stage);
            const pct =
              active && d && d.total_bytes > 0
                ? Math.min(100, Math.floor((d.total_downloaded / d.total_bytes) * 100))
                : 0;
            return (
              <div
                key={m.name}
                className="flex items-center justify-between gap-2 rounded border border-line px-3 py-2 text-xs"
              >
                <div className="min-w-0">
                  <span className="text-paper">{m.name}</span>
                  <span className="ml-2 text-mute">
                    约 {m.size_mb}MB · {m.vram_hint}
                  </span>
                </div>
                <div className="flex shrink-0 items-center gap-2">
                  {m.installed ? (
                    <span className="text-emerald-400">已安装</span>
                  ) : active ? (
                    <span className="text-amber-400">下载中 {pct}%</span>
                  ) : d?.stage === "error" ? (
                    <span className="text-bad">下载失败</span>
                  ) : d?.stage === "cancelled" || d?.stage === "interrupted" ? (
                    <span className="text-mute">已中断</span>
                  ) : (
                    <span className="text-mute">未安装</span>
                  )}
                  {active ? (
                    <button
                      type="button"
                      onClick={() => onOpenDownload(m.name)}
                      className="h-7 rounded border border-line px-3 text-mute hover:text-paper"
                    >
                      查看进度
                    </button>
                  ) : !m.installed ? (
                    <button
                      type="button"
                      onClick={() => onOpenDownload(m.name)}
                      className="h-7 rounded border border-accent px-3 text-accent hover:bg-accent/10"
                    >
                      {d && (d.stage === "cancelled" || d.stage === "interrupted")
                        ? "继续下载"
                        : "下载"}
                    </button>
                  ) : (
                    <button
                      type="button"
                      onClick={() => void deleteModel(m.name)}
                      className="h-7 rounded border border-line px-3 text-mute hover:text-bad"
                    >
                      删除
                    </button>
                  )}
                </div>
              </div>
            );
          })}
        </div>

        {error && <p className="mt-3 text-xs text-bad">{error}</p>}
        <p className="mt-4 text-[11px] leading-relaxed text-mute">
          模型保存在应用数据目录 storage/models/whisper/，由程序自动下载管理，无需手动安装；
          全局同时只能下载一个模型。
        </p>
      </div>
    </div>
  );
}
