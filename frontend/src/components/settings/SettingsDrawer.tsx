import { useCallback, useEffect, useState } from "react";
import { api } from "../../api/client";
import type { AiProvider, AppSettings, FfmpegStatus, FfmpegUpdateInfo, FfmpegUpdateProgress, MusicStatus, WhisperModelInfo, YtDlpStatus, YtDlpUpdateInfo, YtDlpUpdateProgress } from "../../api/types";
import { cleanConditions, formatBytes, formatSpeed } from "../../lib/format";
import { WhisperModelDialog } from "./WhisperModelDialog";
import { WhisperModelManager } from "./WhisperModelManager";

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
  condition_basis: {},
  node_bin: "node",
  publisher_script: "",
  publisher_browser: "chrome",
  collector_script: "",
  douyin_sync_script: "scripts/douyin-sync/sync.js",
  account_style: "治愈",
  batch_size: 20,
  tiktok_ratio: 0.3,
  instagram_ratio: 0.7,
  max_likes: 1_000_000,
  vision_provider: "ollama",
  llm_provider: "ollama",
  openai_vision_base_url: "https://api.kimi.com/coding/v1",
  openai_vision_api_key: "",
  openai_vision_model: "k3-256k",
  openai_llm_base_url: "https://api.kimi.com/coding/v1",
  openai_llm_api_key: "",
  openai_llm_model: "k3-256k",
  publish_declaration: true,
};

/** 内容筛选：命中/排除条件合并展示。绿 tag=命中条件，红 tag=排除条件；
 *  点击底部按钮在 tag 区就地输入，回车/失焦确认、Esc 取消。 */
function ConditionTags({
  hit,
  exclude,
  onChangeHit,
  onChangeExclude,
}: {
  hit: string[];
  exclude: string[];
  onChangeHit: (items: string[]) => void;
  onChangeExclude: (items: string[]) => void;
}) {
  const [adding, setAdding] = useState<"hit" | "exclude" | null>(null);
  const [draft, setDraft] = useState("");

  const commit = () => {
    const v = draft.trim();
    if (adding === "hit" && v) onChangeHit([...hit, v]);
    if (adding === "exclude" && v) onChangeExclude([...exclude, v]);
    setAdding(null);
    setDraft("");
  };

  return (
    <div className="col-span-2 border-t border-line pt-3">
      <div className="flex items-center gap-4">
        <p className="text-xs font-semibold uppercase tracking-wider text-mute">内容筛选</p>
        <button
          type="button"
          onClick={() => {
            setAdding("hit");
            setDraft("");
          }}
          className="text-xs text-emerald-400 hover:brightness-110"
        >
          + 添加命中条件
        </button>
        <button
          type="button"
          onClick={() => {
            setAdding("exclude");
            setDraft("");
          }}
          className="text-xs text-bad hover:brightness-110"
        >
          + 添加排除条件
        </button>
      </div>
      {hit.length === 0 && exclude.length === 0 && !adding && (
        <p className="mt-2 text-xs text-mute">不限制（空列表）</p>
      )}
      <div className="mt-2 flex flex-wrap items-center gap-1.5">
        {hit.map((item, i) => (
          <span
            key={`hit-${i}`}
            className="inline-flex items-center gap-1 rounded border border-emerald-500/40 bg-emerald-500/10 px-2 py-0.5 text-xs text-emerald-400"
          >
            {item}
            <button
              type="button"
              title="删除"
              onClick={() => onChangeHit(hit.filter((_, idx) => idx !== i))}
              className="opacity-60 hover:opacity-100"
            >
              ✕
            </button>
          </span>
        ))}
        {exclude.map((item, i) => (
          <span
            key={`exclude-${i}`}
            className="inline-flex items-center gap-1 rounded border border-bad/40 bg-bad/10 px-2 py-0.5 text-xs text-bad"
          >
            {item}
            <button
              type="button"
              title="删除"
              onClick={() => onChangeExclude(exclude.filter((_, idx) => idx !== i))}
              className="opacity-60 hover:opacity-100"
            >
              ✕
            </button>
          </span>
        ))}
        {adding && (
          <input
            autoFocus
            value={draft}
            placeholder={adding === "hit" ? "如：宠物、生活、风景、手工" : "如：长中文对话、色情、性暗示"}
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                commit();
              }
              if (e.key === "Escape") {
                setAdding(null);
                setDraft("");
              }
            }}
            onBlur={commit}
            className="h-6 w-44 rounded border border-line bg-ink px-2 text-xs text-paper focus:border-accent/60 focus:outline-none"
          />
        )}
      </div>
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
  const [aiTab, setAiTab] = useState<"vision" | "llm" | "asr">("vision");
  const [whisperModels, setWhisperModels] = useState<WhisperModelInfo[]>([]);
  const [whisperDialogFor, setWhisperDialogFor] = useState<string | null>(null);
  const [whisperManagerOpen, setWhisperManagerOpen] = useState(false);
  const [ytdlpStatus, setYtdlpStatus] = useState<YtDlpStatus | null>(null);
  const [ytdlpLatest, setYtdlpLatest] = useState<YtDlpUpdateInfo | null>(null);
  const [ytdlpChecking, setYtdlpChecking] = useState(false);
  const [ytdlpCheckFailed, setYtdlpCheckFailed] = useState(false);
  const [ytdlpProgress, setYtdlpProgress] = useState<YtDlpUpdateProgress | null>(null);
  const [ytdlpMsg, setYtdlpMsg] = useState<{ ok: boolean; text: string } | null>(null);
  const [ffmpegStatus, setFfmpegStatus] = useState<FfmpegStatus | null>(null);
  const [ffmpegLatest, setFfmpegLatest] = useState<FfmpegUpdateInfo | null>(null);
  const [ffmpegChecking, setFfmpegChecking] = useState(false);
  const [ffmpegCheckFailed, setFfmpegCheckFailed] = useState(false);
  const [ffmpegProgress, setFfmpegProgress] = useState<FfmpegUpdateProgress | null>(null);
  const [ffmpegMsg, setFfmpegMsg] = useState<{ ok: boolean; text: string } | null>(null);
  const [musicStatus, setMusicStatus] = useState<MusicStatus | null>(null);
  const [musicSyncing, setMusicSyncing] = useState(false);
  const [musicMsg, setMusicMsg] = useState<{ ok: boolean; text: string } | null>(null);
  // 更新下载/校验进行中（后台线程执行，前端轮询进度——UI 全程不阻塞）。
  const ytdlpActive =
    ytdlpProgress !== null &&
    (ytdlpProgress.stage === "downloading" || ytdlpProgress.stage === "verifying");
  const ffmpegActive =
    ffmpegProgress !== null &&
    (ffmpegProgress.stage === "downloading" || ffmpegProgress.stage === "verifying");

  // 手动「检查更新」按钮：强制联网查一次（后端会刷新缓存与 db）。
  const checkYtdlpUpdate = async () => {
    setYtdlpChecking(true);
    try {
      setYtdlpLatest(await api.checkYtdlpUpdate());
      setYtdlpCheckFailed(false);
    } catch {
      setYtdlpLatest(null);
      setYtdlpCheckFailed(true);
    } finally {
      setYtdlpChecking(false);
    }
  };

  // 打开设置页只读每日检查缓存（不联网——缓存由每天启动时的后台静默检查填充）。
  // 缓存还在 checking（启动检查在途）时每 2s 重读，最多 5 次（覆盖后端 10s 超时）。
  const loadCachedYtdlpUpdate = async (attempt = 0) => {
    try {
      const r = await api.getCachedYtdlpUpdate();
      if (r.state === "checking") {
        if (attempt < 5) {
          setYtdlpChecking(true);
          setTimeout(() => void loadCachedYtdlpUpdate(attempt + 1), 2000);
        } else {
          setYtdlpChecking(false);
          setYtdlpCheckFailed(true);
        }
      } else if (r.state === "ok" && r.info) {
        setYtdlpLatest(r.info);
        setYtdlpCheckFailed(false);
        setYtdlpChecking(false);
      } else {
        setYtdlpLatest(null);
        setYtdlpCheckFailed(true);
        setYtdlpChecking(false);
      }
    } catch {
      setYtdlpChecking(false);
      setYtdlpCheckFailed(true);
    }
  };

  // 手动「检查更新」按钮：强制联网查一次（后端会刷新缓存与 db）。
  const checkFfmpegUpdate = async () => {
    setFfmpegChecking(true);
    try {
      setFfmpegLatest(await api.checkFfmpegUpdate());
      setFfmpegCheckFailed(false);
    } catch {
      setFfmpegLatest(null);
      setFfmpegCheckFailed(true);
    } finally {
      setFfmpegChecking(false);
    }
  };

  // FFmpeg 每日检查缓存（语义与 loadCachedYtdlpUpdate 相同，不联网）。
  const loadCachedFfmpegUpdate = async (attempt = 0) => {
    try {
      const r = await api.getCachedFfmpegUpdate();
      if (r.state === "checking") {
        if (attempt < 5) {
          setFfmpegChecking(true);
          setTimeout(() => void loadCachedFfmpegUpdate(attempt + 1), 2000);
        } else {
          setFfmpegChecking(false);
          setFfmpegCheckFailed(true);
        }
      } else if (r.state === "ok" && r.info) {
        setFfmpegLatest(r.info);
        setFfmpegCheckFailed(false);
        setFfmpegChecking(false);
      } else {
        setFfmpegLatest(null);
        setFfmpegCheckFailed(true);
        setFfmpegChecking(false);
      }
    } catch {
      setFfmpegChecking(false);
      setFfmpegCheckFailed(true);
    }
  };

  const refreshWhisperModels = useCallback(async () => {
    try {
      const next = await api.listWhisperModels();
      // 内容无变化时返回原引用，避免每 1.5s 触发整抽屉重渲染。
      setWhisperModels((prev) =>
        JSON.stringify(prev) === JSON.stringify(next) ? prev : next,
      );
    } catch {
      /* 读取失败保持旧状态，下一轮轮询重试 */
    }
  }, []);

  // 抽屉打开期间 1.5s 轮询模型安装状态与下载进度（全库轮询架构，无 event 推送）。
  useEffect(() => {
    if (!open) return;
    void refreshWhisperModels();
    const t = setInterval(() => void refreshWhisperModels(), 1500);
    return () => clearInterval(t);
  }, [open, refreshWhisperModels]);

  useEffect(() => {
    if (open) {
      setError(null);
      setYtdlpLatest(null);
      setYtdlpCheckFailed(false);
      setYtdlpMsg(null);
      setFfmpegLatest(null);
      setFfmpegCheckFailed(false);
      setFfmpegMsg(null);
      setMusicMsg(null);
      void api.getSettings().then(setSettings).catch((e) => setError(String(e)));
      void api.getYtdlpStatus().then(setYtdlpStatus).catch(() => setYtdlpStatus(null));
      void api.getFfmpegStatus().then(setFfmpegStatus).catch(() => setFfmpegStatus(null));
      void api.getMusicStatus().then(setMusicStatus).catch(() => setMusicStatus(null));
      // 只读每日检查缓存（每天启动时已后台静默检查，这里不联网）。
      void loadCachedYtdlpUpdate();
      void loadCachedFfmpegUpdate();
      // 后台可能正在下载（上次点了更新就关了抽屉）——恢复进度轮询。
      void api
        .getYtdlpUpdateProgress()
        .then((p) => {
          if (p && (p.stage === "downloading" || p.stage === "verifying")) setYtdlpProgress(p);
        })
        .catch(() => {});
      void api
        .getFfmpegUpdateProgress()
        .then((p) => {
          if (p && (p.stage === "downloading" || p.stage === "verifying")) setFfmpegProgress(p);
        })
        .catch(() => {});
    }
  }, [open]);

  // 更新进行中 1s 轮询进度；done/error 转移到消息行并停止。
  useEffect(() => {
    if (!open || !ytdlpActive) return;
    const t = setInterval(() => {
      void (async () => {
        try {
          const p = await api.getYtdlpUpdateProgress();
          if (p === null) return; // 后台线程尚未写入进度，下轮再读
          setYtdlpProgress(p);
          if (p.stage === "done") {
            setYtdlpMsg({ ok: true, text: p.message });
            setYtdlpProgress(null);
            void api.getYtdlpStatus().then(setYtdlpStatus).catch(() => {});
            // latest 缓存仍有效，重算 update_available 即可，不联网。
            void loadCachedYtdlpUpdate();
          } else if (p.stage === "error") {
            setYtdlpMsg({ ok: false, text: p.message });
            setYtdlpProgress(null);
          }
        } catch {
          /* 下一轮重试 */
        }
      })();
    }, 1000);
    return () => clearInterval(t);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, ytdlpActive]);

  // FFmpeg 更新轮询（逻辑同 yt-dlp）。
  useEffect(() => {
    if (!open || !ffmpegActive) return;
    const t = setInterval(() => {
      void (async () => {
        try {
          const p = await api.getFfmpegUpdateProgress();
          if (p === null) return; // 后台线程尚未写入进度，下轮再读
          setFfmpegProgress(p);
          if (p.stage === "done") {
            setFfmpegMsg({ ok: true, text: p.message });
            setFfmpegProgress(null);
            void api.getFfmpegStatus().then(setFfmpegStatus).catch(() => {});
            void loadCachedFfmpegUpdate();
          } else if (p.stage === "error") {
            setFfmpegMsg({ ok: false, text: p.message });
            setFfmpegProgress(null);
          }
        } catch {
          /* 下一轮重试 */
        }
      })();
    }, 1000);
    return () => clearInterval(t);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, ffmpegActive]);

  if (!open) return null;

  const set = (patch: Partial<AppSettings>) =>
    setSettings((s) => ({ ...s, ...patch }));

  const updateYtdlp = async () => {
    setYtdlpMsg(null);
    try {
      // 后端后台线程执行（已是最新会短路不下载），立即返回；进度走轮询。
      await api.updateYtdlp();
      setYtdlpProgress({ stage: "downloading", downloaded: 0, total: 0, speed_bps: 0, message: "" });
    } catch (e) {
      setYtdlpMsg({ ok: false, text: e instanceof Error ? e.message : String(e) });
    }
  };

  const updateFfmpeg = async () => {
    setFfmpegMsg(null);
    try {
      // 后端后台线程执行（已是最新会短路不下载 ~106MB 的 zip），立即返回；进度走轮询。
      await api.updateFfmpeg();
      setFfmpegProgress({ stage: "downloading", downloaded: 0, total: 0, speed_bps: 0, message: "" });
    } catch (e) {
      setFfmpegMsg({ ok: false, text: e instanceof Error ? e.message : String(e) });
    }
  };

  // 一键同步剪映音乐：后端 spawn_blocking 全量比对（秒级），按钮 loading 等结果。
  // 随后增量匹配专辑封面（只查新增的几首，不重复下载已有封面）；封面失败不影响同步结果。
  const syncMusic = async () => {
    setMusicMsg(null);
    setMusicSyncing(true);
    try {
      const r = await api.syncMusic();
      const syncText = `同步完成：新复制 ${r.copied} 首，已存在跳过 ${r.skipped} 首${
        r.unmatched > 0 ? `（${r.unmatched} 首未匹配歌名，已跳过）` : ""
      }`;
      setMusicMsg({ ok: true, text: `${syncText}；正在匹配专辑封面…` });
      try {
        const c = await api.ensureMusicCovers();
        setMusicMsg({
          ok: true,
          text: `${syncText}；封面新匹配 ${c.downloaded} 张${
            c.not_found > 0 ? `，${c.not_found} 首未找到` : ""
          }`,
        });
      } catch {
        setMusicMsg({ ok: true, text: `${syncText}（封面匹配失败，下次同步自动重试）` });
      }
      void api.getMusicStatus().then(setMusicStatus).catch(() => {});
    } catch (e) {
      setMusicMsg({ ok: false, text: e instanceof Error ? e.message : String(e) });
    } finally {
      setMusicSyncing(false);
    }
  };

  const openMusicDir = async () => {
    setMusicMsg(null);
    try {
      await api.openMusicDir();
    } catch (e) {
      setMusicMsg({ ok: false, text: e instanceof Error ? e.message : String(e) });
    }
  };

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
    <div className="fixed inset-0 z-40 flex justify-end bg-black/50 backdrop-blur-sm">
      <div className="flex h-full w-2/3 flex-col border-l border-line bg-panel p-5 shadow-2xl">
        <div className="flex items-center justify-between">
          <h2 className="text-base font-bold text-paper">设置</h2>
          <button onClick={onClose} className="text-sm text-mute hover:text-paper">
            ✕
          </button>
        </div>

        <div className="mt-4 grid flex-1 grid-cols-2 content-start gap-x-4 gap-y-3 overflow-y-auto text-sm">
          <div className="col-span-2 border-t border-line pt-3">
            <p className="text-xs font-semibold uppercase tracking-wider text-mute">AI 模型</p>
          </div>
          <div className="col-span-2 flex gap-1 rounded border border-line p-1">
            {(
              [
                ["vision", "视觉"],
                ["llm", "文本"],
                ["asr", "听觉"],
              ] as const
            ).map(([key, label]) => (
              <button
                key={key}
                type="button"
                onClick={() => setAiTab(key)}
                className={`h-8 flex-1 rounded text-xs transition-colors ${
                  aiTab === key ? "bg-accent font-semibold text-ink" : "text-mute hover:text-paper"
                }`}
              >
                {label}
              </button>
            ))}
          </div>
          {aiTab === "vision" && (
            <>
              <label className="block">
                <span className="text-xs text-mute">视觉模型提供方</span>
                <select
                  className={field}
                  value={settings.vision_provider}
                  onChange={(e) => set({ vision_provider: e.target.value as AiProvider })}
                >
                  <option value="ollama">本地 Ollama</option>
                  <option value="openai">云端 OpenAI 兼容（Kimi 等）</option>
                </select>
              </label>
              {settings.vision_provider === "ollama" ? (
                <>
                  <label className="block">
                    <span className="text-xs text-mute">Ollama 地址</span>
                    <input className={field} value={settings.ollama_url} onChange={(e) => set({ ollama_url: e.target.value })} />
                  </label>
                  <label className="block">
                    <span className="text-xs text-mute">本地视觉模型</span>
                    <input className={field} value={settings.vision_model} onChange={(e) => set({ vision_model: e.target.value })} />
                  </label>
                </>
              ) : (
                <>
                  <label className="block">
                    <span className="text-xs text-mute">云端 Base URL</span>
                    <input
                      className={field}
                      value={settings.openai_vision_base_url}
                      placeholder="https://api.kimi.com/coding/v1"
                      onChange={(e) => set({ openai_vision_base_url: e.target.value })}
                    />
                  </label>
                  <label className="block">
                    <span className="text-xs text-mute">云端 API Key</span>
                    <input
                      type="password"
                      className={field}
                      value={settings.openai_vision_api_key}
                      placeholder="sk-..."
                      onChange={(e) => set({ openai_vision_api_key: e.target.value })}
                    />
                  </label>
                  <label className="block">
                    <span className="text-xs text-mute">云端视觉模型</span>
                    <input className={field} value={settings.openai_vision_model} onChange={(e) => set({ openai_vision_model: e.target.value })} />
                  </label>
                </>
              )}
            </>
          )}
          {aiTab === "llm" && (
            <>
              <label className="block">
                <span className="text-xs text-mute">文本模型提供方</span>
                <select
                  className={field}
                  value={settings.llm_provider}
                  onChange={(e) => set({ llm_provider: e.target.value as AiProvider })}
                >
                  <option value="ollama">本地 Ollama</option>
                  <option value="openai">云端 OpenAI 兼容（Kimi 等）</option>
                </select>
              </label>
              {settings.llm_provider === "ollama" ? (
                <>
                  <label className="block">
                    <span className="text-xs text-mute">Ollama 地址</span>
                    <input className={field} value={settings.ollama_url} onChange={(e) => set({ ollama_url: e.target.value })} />
                  </label>
                  <label className="block">
                    <span className="text-xs text-mute">本地文本模型</span>
                    <input className={field} value={settings.llm_model} onChange={(e) => set({ llm_model: e.target.value })} />
                  </label>
                </>
              ) : (
                <>
                  <label className="block">
                    <span className="text-xs text-mute">云端 Base URL</span>
                    <input
                      className={field}
                      value={settings.openai_llm_base_url}
                      placeholder="https://api.kimi.com/coding/v1"
                      onChange={(e) => set({ openai_llm_base_url: e.target.value })}
                    />
                  </label>
                  <label className="block">
                    <span className="text-xs text-mute">云端 API Key</span>
                    <input
                      type="password"
                      className={field}
                      value={settings.openai_llm_api_key}
                      placeholder="sk-..."
                      onChange={(e) => set({ openai_llm_api_key: e.target.value })}
                    />
                  </label>
                  <label className="block">
                    <span className="text-xs text-mute">云端文本模型</span>
                    <input className={field} value={settings.openai_llm_model} onChange={(e) => set({ openai_llm_model: e.target.value })} />
                  </label>
                </>
              )}
            </>
          )}
          {aiTab === "asr" && (
            <div className="col-span-2 flex items-end gap-2">
              <label className="block flex-1">
                <span className="text-xs text-mute">Whisper 模型（语音识别）</span>
                <select
                  className={field}
                  value={settings.whisper_model}
                  onChange={(e) => {
                    const name = e.target.value;
                    set({ whisper_model: name });
                    // 选中未安装模型时弹窗引导一键下载（内置模型库，不再依赖外部预装）。
                    const info = whisperModels.find((m) => m.name === name);
                    if (info && !info.installed) setWhisperDialogFor(name);
                  }}
                >
                  {whisperModels.map((m) => (
                    <option key={m.name} value={m.name}>
                      {m.name}（约 {m.size_mb}MB · {m.vram_hint}）{m.installed ? " ✓已安装" : ""}
                    </option>
                  ))}
                  {!whisperModels.some((m) => m.name === settings.whisper_model) &&
                    settings.whisper_model !== "" && (
                      <option value={settings.whisper_model}>自定义：{settings.whisper_model}</option>
                    )}
                </select>
              </label>
              {/* 模型管理入口：下载中显示琥珀色指示点 */}
              <button
                type="button"
                onClick={() => setWhisperManagerOpen(true)}
                title="管理 Whisper 模型（下载 / 删除）"
                className="relative flex h-9 w-9 shrink-0 items-center justify-center rounded border border-line text-mute hover:border-accent/60 hover:text-paper"
              >
                ⚙
                {whisperModels.some(
                  (m) =>
                    m.download &&
                    ["connecting", "downloading", "verifying"].includes(m.download.stage),
                ) && (
                  <span className="absolute right-1 top-1 h-1.5 w-1.5 rounded-full bg-amber-400" />
                )}
              </button>
            </div>
          )}
          <label className="block">
            <span className="text-xs text-mute">质量分保留线（0-100）</span>
            <input
              type="number"
              className={field}
              value={settings.quality_min_keep}
              onChange={(e) => set({ quality_min_keep: Number(e.target.value) })}
            />
          </label>
          {/* yt-dlp：程序内置无需配置；版本信息并入标题行（常态单行高度），
              有更新时版本号右侧出现新版本 tag 与「一键更新」，进度/消息才展开第二行 */}
          <div className="col-span-2 rounded border border-sky-400/30 bg-sky-400/10 p-3">
            <div className="flex items-center justify-between">
              <p className="flex items-center text-xs text-mute">
                <span className="font-semibold uppercase tracking-wider">yt-dlp</span>
                <span className="ml-3">
                  当前版本：
                  {ytdlpStatus === null
                    ? "读取中…"
                    : ytdlpStatus.installed
                      ? (ytdlpStatus.current_version ?? "未知")
                      : "未安装"}
                </span>
                {ytdlpLatest?.update_available && ytdlpStatus !== null && (
                  <span className="ml-2 rounded-full bg-accent/15 px-2 py-0.5 text-accent">
                    → {ytdlpLatest.latest_version}
                  </span>
                )}
                {ytdlpLatest && !ytdlpLatest.update_available && ytdlpStatus?.installed && (
                  <span className="ml-2 text-mute/70">已是最新</span>
                )}
                {ytdlpCheckFailed && !ytdlpChecking && !ytdlpLatest && (
                  <span className="ml-2 text-mute/70">检查失败，可点「检查更新」重试</span>
                )}
              </p>
              <div className="flex gap-2">
                <button
                  type="button"
                  disabled={ytdlpChecking || ytdlpActive}
                  onClick={() => void checkYtdlpUpdate()}
                  className="h-7 rounded border border-line px-3 text-xs text-mute hover:text-paper disabled:opacity-50"
                >
                  {ytdlpChecking ? "检查中…" : "检查更新"}
                </button>
                {/* 已是最新不显示更新按钮（不再白下载一轮）；更新中由下方进度行表达状态 */}
                {(ytdlpLatest?.update_available || ytdlpStatus?.installed === false) && !ytdlpActive && (
                  <button
                    type="button"
                    onClick={() => void updateYtdlp()}
                    className="h-7 rounded border border-accent bg-accent px-3 text-xs font-semibold text-ink hover:brightness-110"
                  >
                    {ytdlpStatus?.installed === false ? "下载安装" : "一键更新"}
                  </button>
                )}
              </div>
            </div>
            {(ytdlpProgress || ytdlpMsg) && (
              <div className="mt-2 space-y-1 text-xs text-mute">
                {ytdlpProgress && (
                  <p className="flex items-center gap-1.5 text-paper">
                    <span className="inline-block animate-spin">⟳</span>
                    {ytdlpProgress.stage === "verifying"
                      ? "校验中…"
                      : `下载中 ${formatSpeed(ytdlpProgress.speed_bps)} · ${formatBytes(ytdlpProgress.downloaded)}${
                          ytdlpProgress.total > 0
                            ? `/${formatBytes(ytdlpProgress.total)}（${Math.min(100, Math.floor((ytdlpProgress.downloaded / ytdlpProgress.total) * 100))}%）`
                            : ""
                        }`}
                  </p>
                )}
                {ytdlpMsg && (
                  <p className={ytdlpMsg.ok ? "text-emerald-400" : "text-bad"}>{ytdlpMsg.text}</p>
                )}
              </div>
            )}
          </div>
          {/* FFmpeg：程序内置无需配置（抽帧/封面/时长/音乐合成都用它）；
              卡片与 yt-dlp 同构——常态单行，有更新时版本号右侧出现新版本 tag 与「一键更新」，
              进度/消息才展开第二行（verifying 含 zip 解压，~106MB 下载耐心等） */}
          <div className="col-span-2 rounded border border-sky-400/30 bg-sky-400/10 p-3">
            <div className="flex items-center justify-between">
              <p className="flex items-center text-xs text-mute">
                <span className="font-semibold uppercase tracking-wider">FFmpeg</span>
                <span className="ml-3">
                  当前版本：
                  {ffmpegStatus === null
                    ? "读取中…"
                    : ffmpegStatus.installed
                      ? (ffmpegStatus.current_version ?? "未知")
                      : "未安装"}
                </span>
                {ffmpegLatest?.update_available && ffmpegStatus !== null && (
                  <span className="ml-2 rounded-full bg-accent/15 px-2 py-0.5 text-accent">
                    → {ffmpegLatest.latest_version}
                  </span>
                )}
                {ffmpegLatest && !ffmpegLatest.update_available && ffmpegStatus?.installed && (
                  <span className="ml-2 text-mute/70">已是最新</span>
                )}
                {ffmpegCheckFailed && !ffmpegChecking && !ffmpegLatest && (
                  <span className="ml-2 text-mute/70">检查失败，可点「检查更新」重试</span>
                )}
              </p>
              <div className="flex gap-2">
                <button
                  type="button"
                  disabled={ffmpegChecking || ffmpegActive}
                  onClick={() => void checkFfmpegUpdate()}
                  className="h-7 rounded border border-line px-3 text-xs text-mute hover:text-paper disabled:opacity-50"
                >
                  {ffmpegChecking ? "检查中…" : "检查更新"}
                </button>
                {/* 已是最新不显示更新按钮（不再白下载一轮）；更新中由下方进度行表达状态 */}
                {(ffmpegLatest?.update_available || ffmpegStatus?.installed === false) && !ffmpegActive && (
                  <button
                    type="button"
                    onClick={() => void updateFfmpeg()}
                    className="h-7 rounded border border-accent bg-accent px-3 text-xs font-semibold text-ink hover:brightness-110"
                  >
                    {ffmpegStatus?.installed === false ? "下载安装" : "一键更新"}
                  </button>
                )}
              </div>
            </div>
            {(ffmpegProgress || ffmpegMsg) && (
              <div className="mt-2 space-y-1 text-xs text-mute">
                {ffmpegProgress && (
                  <p className="flex items-center gap-1.5 text-paper">
                    <span className="inline-block animate-spin">⟳</span>
                    {ffmpegProgress.stage === "verifying"
                      ? "解压校验中…"
                      : `下载中 ${formatSpeed(ffmpegProgress.speed_bps)} · ${formatBytes(ffmpegProgress.downloaded)}${
                          ffmpegProgress.total > 0
                            ? `/${formatBytes(ffmpegProgress.total)}（${Math.min(100, Math.floor((ffmpegProgress.downloaded / ffmpegProgress.total) * 100))}%）`
                            : ""
                        }`}
                  </p>
                )}
                {ffmpegMsg && (
                  <p className={ffmpegMsg.ok ? "text-emerald-400" : "text-bad"}>{ffmpegMsg.text}</p>
                )}
              </div>
            )}
          </div>
          {/* 剪映音乐：一键同步本机剪映缓存到 storage/music；数量并入标题行（常态单行），
              同步结果/错误才展开第二行。未找到缓存时禁用同步按钮 */}
          <div className="col-span-2 rounded border border-sky-400/30 bg-sky-400/10 p-3">
            <div className="flex items-center justify-between">
              <p className="flex items-center text-xs text-mute">
                <span className="font-semibold uppercase tracking-wider">剪映音乐</span>
                <span className="ml-3">
                  已同步：{musicStatus === null ? "读取中…" : `${musicStatus.count} 首`}
                </span>
                {musicStatus && !musicStatus.cache_found && (
                  <span
                    className="ml-2 text-mute/70"
                    title="需安装剪映专业版并在剪映里下载过音乐"
                  >
                    未找到剪映缓存
                  </span>
                )}
              </p>
              <div className="flex gap-2">
                <button
                  type="button"
                  onClick={() => void openMusicDir()}
                  className="h-7 rounded border border-line px-3 text-xs text-mute hover:text-paper"
                >
                  打开目录
                </button>
                <button
                  type="button"
                  disabled={musicSyncing || !musicStatus?.cache_found}
                  onClick={() => void syncMusic()}
                  className="h-7 rounded border border-accent bg-accent px-3 text-xs font-semibold text-ink hover:brightness-110 disabled:opacity-50"
                >
                  {musicSyncing ? "同步中…" : "一键同步"}
                </button>
              </div>
            </div>
            {musicMsg && (
              <div className="mt-2 space-y-1 text-xs text-mute">
                <p className={musicMsg.ok ? "text-emerald-400" : "text-bad"}>{musicMsg.text}</p>
              </div>
            )}
          </div>
          <ConditionTags
            hit={settings.hit_conditions}
            exclude={settings.exclude_conditions}
            onChangeHit={(hit_conditions) => set({ hit_conditions })}
            onChangeExclude={(exclude_conditions) => set({ exclude_conditions })}
          />
          <label className="col-span-2 flex cursor-pointer items-center gap-2 border-t border-line pt-3">
            <input
              type="checkbox"
              checked={settings.publish_declaration}
              onChange={(e) => set({ publish_declaration: e.target.checked })}
              className="h-3.5 w-3.5 accent-emerald-500"
            />
            <span className="text-xs text-mute">
              发布时自动设置「自主声明 → 内容为转载信息 → 取材站外」
            </span>
          </label>
          <div className="col-span-2 border-t border-line pt-3">
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
          <label className="col-span-2 block">
            <span className="text-xs text-mute">文案风格 Prompt</span>
            <textarea
              rows={5}
              value={settings.account_style}
              onChange={(e) => set({ account_style: e.target.value })}
              placeholder="例如：用轻松幽默的口吻，加入网络热梗和夸张语气"
              className="w-full resize-y rounded border border-line bg-ink px-2 py-1.5 text-sm text-paper focus:border-accent/60 focus:outline-none"
            />
          </label>
          <p className="col-span-2 text-[11px] leading-relaxed text-mute">
            采集配额保存后，下一次采集立即生效。
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
      {whisperManagerOpen && (
        <WhisperModelManager
          models={whisperModels}
          onClose={() => setWhisperManagerOpen(false)}
          onOpenDownload={(name) => setWhisperDialogFor(name)}
          onRefresh={() => void refreshWhisperModels()}
        />
      )}
      {whisperDialogFor && (
        <WhisperModelDialog
          modelName={whisperDialogFor}
          info={whisperModels.find((m) => m.name === whisperDialogFor) ?? null}
          onClose={() => setWhisperDialogFor(null)}
          onInstalled={(name) => set({ whisper_model: name })}
        />
      )}
    </div>
  );
}
