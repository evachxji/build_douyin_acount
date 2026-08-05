import { useEffect, useState } from "react";
import { api } from "../../api/client";
import type { RecommendationResult } from "../../api/types";
import { useAssetsStore } from "../../stores/assetsStore";
import { useTaskStore } from "../../stores/taskStore";

export function RecommendationDialog({
  open,
  onClose,
}: {
  open: boolean;
  onClose: () => void;
}) {
  const [data, setData] = useState<RecommendationResult | null>(null);
  const [loading, setLoading] = useState(false);
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [error, setError] = useState<string | null>(null);
  const setTaskOpen = useTaskStore((s) => s.setOpen);

  useEffect(() => {
    if (!open) return;
    setLoading(true);
    setError(null);
    void api
      .getRecommendations()
      .then((r) => {
        setData(r);
        setSelected(new Set());
      })
      .catch((e) => setError(e instanceof Error ? e.message : String(e)))
      .finally(() => setLoading(false));
  }, [open]);

  if (!open) return null;

  const pick = async (assetId: number) => {
    await useAssetsStore.getState().updateStatus(assetId, "SELECTED");
    setSelected((s) => new Set(s).add(assetId));
    setTaskOpen(true);
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 backdrop-blur-sm">
      <div className="flex max-h-[80vh] w-[720px] flex-col rounded-xl border border-accent/40 bg-panel shadow-2xl">
        <header className="flex items-center justify-between border-b border-line p-4">
          <div>
            <h2 className="text-base font-bold text-paper">推荐清单</h2>
            <p className="mt-0.5 text-xs text-mute">
              按分类给出质量与热度综合排名靠前的素材；入选素材已自动生成文案任务。
            </p>
          </div>
          <button onClick={onClose} className="text-sm text-mute hover:text-paper">
            ✕
          </button>
        </header>
        <div className="min-h-0 flex-1 overflow-y-auto p-4">
          {loading && <p className="text-sm text-mute">计算中…</p>}
          {error && <p className="text-sm text-bad">{error}</p>}
          {!loading && !error && data?.groups.length === 0 && (
            <p className="text-sm text-mute">暂无候选素材，先采集并完成 AI 分析。</p>
          )}
          <div className="space-y-5">
            {data?.groups.map((g) => (
              <section key={g.category}>
                <h3 className="text-sm font-semibold text-paper">
                  {g.category}
                  <span className="ml-2 text-xs font-normal text-mute">
                    推荐 {g.items.length} 条
                  </span>
                </h3>
                <div className="mt-2 space-y-2">
                  {g.items.map((item) => {
                    const done = selected.has(item.asset_id);
                    return (
                      <div
                        key={item.asset_id}
                        className="flex items-center gap-3 rounded-lg border border-line bg-ink/60 p-3"
                      >
                        <div className="min-w-0 flex-1">
                          <p className="truncate text-sm text-paper/90">
                            {item.title || `视频 #${item.asset_id}`}
                          </p>
                          <p className="mt-0.5 text-xs text-mute">
                            {item.author ?? "未知作者"} · {item.reason ?? ""} · 得分{" "}
                            {item.score.toFixed(2)}
                          </p>
                        </div>
                        <button
                          onClick={() => void pick(item.asset_id)}
                          disabled={done}
                          className={`shrink-0 rounded border px-3 py-1 text-xs ${
                            done
                              ? "border-ok/40 bg-ok/10 text-ok"
                              : "border-accent/50 text-accent hover:bg-accent/10"
                          }`}
                        >
                          {done ? "已勾选" : "勾选"}
                        </button>
                      </div>
                    );
                  })}
                </div>
              </section>
            ))}
          </div>
        </div>
      </div>
    </div>
  );
}
