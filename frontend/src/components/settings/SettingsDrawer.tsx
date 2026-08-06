import { useEffect, useState } from "react";
import { api } from "../../api/client";
import type { AppSettings } from "../../api/types";
import { cleanConditions } from "../../lib/format";

const EMPTY: AppSettings = {
  ollama_url: "",
  vision_model: "",
  llm_model: "",
  whisper_python: "",
  whisper_script: "",
  whisper_model: "",
  quality_min_keep: 60,
  hit_conditions: [],
  exclude_conditions: [],
  node_bin: "node",
  publisher_script: "",
  publisher_browser: "chrome",
  collector_script: "",
  ytdlp_path: "yt-dlp",
  account_style: "治愈",
  batch_size: 20,
  tiktok_ratio: 0.3,
  instagram_ratio: 0.7,
  max_likes: 1_000_000,
};

const conditionField =
  "h-9 w-full rounded border border-line bg-ink px-2 text-sm text-paper focus:border-accent/60 focus:outline-none";

function ConditionList({
  title,
  placeholder,
  items,
  onChange,
}: {
  title: string;
  placeholder: string;
  items: string[];
  onChange: (items: string[]) => void;
}) {
  const update = (i: number, v: string) => {
    const next = items.slice();
    next[i] = v;
    onChange(next);
  };
  const remove = (i: number) => onChange(items.filter((_, idx) => idx !== i));
  return (
    <div className="border-t border-line pt-3">
      <p className="text-xs font-semibold uppercase tracking-wider text-mute">{title}</p>
      {items.length === 0 && <p className="mt-2 text-xs text-mute">不限制（空列表）</p>}
      <div className="mt-2 space-y-2">
        {items.map((item, i) => (
          <div key={i} className="flex gap-2">
            <input
              className={conditionField}
              value={item}
              placeholder={placeholder}
              onChange={(e) => update(i, e.target.value)}
            />
            <button
              type="button"
              onClick={() => remove(i)}
              className="h-9 shrink-0 rounded border border-line px-3 text-xs text-mute hover:text-bad"
            >
              删除
            </button>
          </div>
        ))}
      </div>
      <button
        type="button"
        onClick={() => onChange([...items, ""])}
        className="mt-2 text-xs text-accent hover:brightness-110"
      >
        + 添加条件
      </button>
    </div>
  );
}

export function SettingsDrawer({
  open,
  onClose,
}: {
  open: boolean;
  onClose: () => void;
}) {
  const [settings, setSettings] = useState<AppSettings>(EMPTY);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (open) {
      setError(null);
      void api.getSettings().then(setSettings).catch((e) => setError(String(e)));
    }
  }, [open]);

  if (!open) return null;

  const set = (patch: Partial<AppSettings>) =>
    setSettings((s) => ({ ...s, ...patch }));

  const save = async () => {
    try {
      const cleaned: AppSettings = {
        ...settings,
        hit_conditions: cleanConditions(settings.hit_conditions),
        exclude_conditions: cleanConditions(settings.exclude_conditions),
      };
      await api.updateSettings(cleaned);
      setSettings(cleaned);
      onClose();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const field =
    "h-9 w-full rounded border border-line bg-ink px-2 text-sm text-paper focus:border-accent/60 focus:outline-none";

  return (
    <div
      className="fixed inset-0 z-40 flex justify-end bg-black/50 backdrop-blur-sm"
      onClick={onClose}
    >
      <div
        className="flex h-full w-[420px] flex-col border-l border-line bg-panel p-5 shadow-2xl"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between">
          <h2 className="text-base font-bold text-paper">设置</h2>
          <button onClick={onClose} className="text-sm text-mute hover:text-paper">
            ✕
          </button>
        </div>

        <div className="mt-4 space-y-3 overflow-y-auto text-sm">
          <label className="block">
            <span className="text-xs text-mute">Ollama 地址</span>
            <input className={field} value={settings.ollama_url} onChange={(e) => set({ ollama_url: e.target.value })} />
          </label>
          <label className="block">
            <span className="text-xs text-mute">视觉模型（Vision）</span>
            <input className={field} value={settings.vision_model} onChange={(e) => set({ vision_model: e.target.value })} />
          </label>
          <label className="block">
            <span className="text-xs text-mute">文本模型（LLM）</span>
            <input className={field} value={settings.llm_model} onChange={(e) => set({ llm_model: e.target.value })} />
          </label>
          <label className="block">
            <span className="text-xs text-mute">Whisper 模型</span>
            <input className={field} value={settings.whisper_model} onChange={(e) => set({ whisper_model: e.target.value })} />
          </label>
          <label className="block">
            <span className="text-xs text-mute">质量分保留线（0-100）</span>
            <input
              type="number"
              className={field}
              value={settings.quality_min_keep}
              onChange={(e) => set({ quality_min_keep: Number(e.target.value) })}
            />
          </label>
          <ConditionList
            title="命中条件"
            placeholder="如：宠物、生活、风景、手工"
            items={settings.hit_conditions}
            onChange={(hit_conditions) => set({ hit_conditions })}
          />
          <ConditionList
            title="排除条件"
            placeholder="如：长中文对话、色情、性暗示"
            items={settings.exclude_conditions}
            onChange={(exclude_conditions) => set({ exclude_conditions })}
          />
          <div className="border-t border-line pt-3">
            <p className="text-xs font-semibold uppercase tracking-wider text-mute">
              采集设置
            </p>
          </div>
          <label className="block">
            <span className="text-xs text-mute">每轮采集条数</span>
            <input
              type="number"
              className={field}
              value={settings.batch_size}
              onChange={(e) => set({ batch_size: Number(e.target.value) })}
            />
          </label>
          <div className="grid grid-cols-2 gap-3">
            <label className="block">
              <span className="text-xs text-mute">TikTok 比例（0-1）</span>
              <input
                type="number"
                step="0.05"
                className={field}
                value={settings.tiktok_ratio}
                onChange={(e) => set({ tiktok_ratio: Number(e.target.value) })}
              />
            </label>
            <label className="block">
              <span className="text-xs text-mute">Instagram 比例（0-1）</span>
              <input
                type="number"
                step="0.05"
                className={field}
                value={settings.instagram_ratio}
                onChange={(e) => set({ instagram_ratio: Number(e.target.value) })}
              />
            </label>
          </div>
          <label className="block">
            <span className="text-xs text-mute">点赞上限（超过不采集）</span>
            <input
              type="number"
              step="10000"
              className={field}
              value={settings.max_likes}
              onChange={(e) => set({ max_likes: Number(e.target.value) })}
            />
          </label>
          <label className="block">
            <span className="text-xs text-mute">yt-dlp 路径</span>
            <input className={field} value={settings.ytdlp_path} onChange={(e) => set({ ytdlp_path: e.target.value })} />
          </label>
          <label className="block">
            <span className="text-xs text-mute">默认文案风格</span>
            <select
              className={field}
              value={settings.account_style}
              onChange={(e) => set({ account_style: e.target.value })}
            >
              {["搞笑", "治愈", "高级感", "情绪价值"].map((s) => (
                <option key={s} value={s}>
                  {s}
                </option>
              ))}
            </select>
          </label>
          <p className="text-[11px] leading-relaxed text-mute">
            采集配额与 yt-dlp 路径保存后，下一次采集立即生效。
          </p>
        </div>

        {error && <p className="mt-3 text-xs text-bad">{error}</p>}
        <div className="mt-auto flex gap-2">
          <button
            onClick={onClose}
            className="h-9 flex-1 rounded border border-line text-sm text-mute hover:text-paper"
          >
            取消
          </button>
          <button
            onClick={() => void save()}
            className="h-9 flex-1 rounded border border-accent bg-accent text-sm font-semibold text-ink hover:brightness-110"
          >
            保存
          </button>
        </div>
      </div>
    </div>
  );
}
