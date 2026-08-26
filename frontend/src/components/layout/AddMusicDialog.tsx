import { useEffect, useRef, useState } from "react";
import { api } from "../../api/client";
import type { MusicFile, VideoAsset } from "../../api/types";
import { audioUrl, coverUrl, videoUrl } from "../../api/urls";
import { formatBytes } from "../../lib/format";

/** 秒 → m:ss（音乐时长展示）。 */
const fmtDur = (s: number) =>
  `${Math.floor(s / 60)}:${String(Math.floor(s % 60)).padStart(2, "0")}`;

/**
 * 添加音乐弹窗：从音乐库（storage/music）选一首合成进视频。
 * 顶部视频预览：点音乐名后视频+音乐同时从头播放、同时暂停，所见即所得——
 * 勾选「关闭原声」时视频静音（仅音乐发声）；不勾时按下方两个音量滑条混音预览（与合成参数一致）。
 * 合成走后端 ffmpeg（音乐裁到视频长度）；已合成过的素材预览/再合成均从原始视频来，不叠音。
 */
export function AddMusicDialog({
  asset,
  onClose,
  onApplied,
}: {
  asset: VideoAsset;
  onClose: () => void;
  /** 合成成功后回调（父组件关弹窗并刷新素材列表）。 */
  onApplied: () => void;
}) {
  const [files, setFiles] = useState<MusicFile[] | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  // 已合成过的素材：回填上次的开关与音量，便于微调后重新合成；默认原声 30% / 音乐 70%。
  const [mute, setMute] = useState(asset.music_info?.mute_original ?? false);
  const [videoVolume, setVideoVolume] = useState(asset.music_info?.video_volume ?? 30);
  const [musicVolume, setMusicVolume] = useState(asset.music_info?.music_volume ?? 70);
  const [playing, setPlaying] = useState<string | null>(null);
  const [durations, setDurations] = useState<Record<string, number>>({});
  // 视频进度条（悬停显示、可拖动定位）。
  const [curTime, setCurTime] = useState(0);
  const [videoDur, setVideoDur] = useState(0);
  const [applying, setApplying] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const audioRef = useRef<HTMLAudioElement | null>(null);
  const videoRef = useRef<HTMLVideoElement | null>(null);

  // 预览源：已合成过的素材从原始视频预览（与再次合成的真实输入一致）。
  const previewSrc = videoUrl(asset.music_info?.origin_file_path ?? asset.file_path);

  useEffect(() => {
    void api
      .listMusicFiles()
      .then(setFiles)
      .catch(() => setFiles([]));
    // 后台增量补封面（只查缺的几首，不阻塞打开）；有新封面则刷新列表。
    void api
      .ensureMusicCovers()
      .then((r) => {
        if (r.downloaded > 0) {
          void api
            .listMusicFiles()
            .then(setFiles)
            .catch(() => {});
        }
      })
      .catch(() => {});
    const v = videoRef.current;
    return () => {
      audioRef.current?.pause();
      audioRef.current = null;
      v?.pause();
    };
  }, []);

  // React 对 <video muted/volume> 的响应式更新不可靠，用 ref 直接同步：
  // 关原声预览 = 视频静音、音乐全量（与合成一致：关原声时滑条不生效）；
  // 保留原声时滑条实时改变两路音量（与合成参数一致）。
  useEffect(() => {
    if (videoRef.current) {
      videoRef.current.muted = mute;
      videoRef.current.volume = videoVolume / 100;
    }
    if (audioRef.current) audioRef.current.volume = mute ? 1 : musicVolume / 100;
  }, [mute, videoVolume, musicVolume]);

  // 每首音乐用 preload=metadata 的隐藏 Audio 懒读时长（只取头几 KB，几十首也是毫秒级）。
  useEffect(() => {
    if (!files) return;
    const probes: HTMLAudioElement[] = [];
    for (const f of files) {
      const a = new Audio();
      a.preload = "metadata";
      a.src = audioUrl(f.path);
      a.onloadedmetadata = () => {
        setDurations((d) => (f.path in d ? d : { ...d, [f.path]: a.duration }));
      };
      probes.push(a);
    }
    return () => {
      probes.forEach((a) => {
        a.src = "";
      });
    };
  }, [files]);

  const stopPlay = () => {
    audioRef.current?.pause();
    audioRef.current = null;
    videoRef.current?.pause();
    setPlaying(null);
  };

  // 点音乐名：视频+音乐同时从头播放；再点一次同时暂停；点别的歌从头切换。
  const togglePlay = (path: string) => {
    if (playing === path) {
      stopPlay();
      return;
    }
    audioRef.current?.pause();
    const a = new Audio(audioUrl(path));
    // 预览贴合合成结果：关原声=音乐全量铺底；保留原声=音乐按滑条音量。
    a.volume = mute ? 1 : musicVolume / 100;
    a.onended = stopPlay; // 音乐先播完：同时暂停（与「同时暂停」语义一致）
    audioRef.current = a;
    const v = videoRef.current;
    if (v) v.currentTime = 0;
    setPlaying(path);
    void a.play().catch(() => stopPlay()); // 个别格式（如 wma）WebView2 播不了：回退静止
    if (v) void v.play().catch(() => {});
  };

  // 点视频画面：暂停/播放；音乐试听中与音乐同步暂停/继续。
  const toggleVideo = () => {
    const v = videoRef.current;
    if (!v) return;
    if (v.paused) {
      void v.play().catch(() => {});
      if (audioRef.current) void audioRef.current.play().catch(() => {});
    } else {
      v.pause();
      audioRef.current?.pause();
    }
  };

  // 拖动进度条定位；音乐试听中同步音乐进度（预览从 0 起播，两路时间轴一致）。
  const seekVideo = (t: number) => {
    const v = videoRef.current;
    if (!v || !Number.isFinite(t)) return;
    v.currentTime = t;
    setCurTime(t);
    if (playing && audioRef.current) {
      try {
        audioRef.current.currentTime = t;
      } catch {
        /* 超出音乐长度时忽略，onended 会统一收尾 */
      }
    }
  };

  const apply = async () => {
    if (!selected) return;
    setApplying(true);
    setError(null);
    try {
      await api.addMusicToVideo(asset.id, selected, mute, videoVolume, musicVolume);
      stopPlay();
      onApplied();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setApplying(false);
    }
  };

  return (
    <div className="fixed inset-0 z-[60] flex items-center justify-center bg-black/70 backdrop-blur-sm">
      <div className="w-[min(1280px,96vw)] rounded-xl border border-accent/40 bg-panel p-6 shadow-2xl">
        <div className="flex items-center justify-between">
          <h3 className="text-sm font-bold text-paper">添加音乐</h3>
          <button onClick={onClose} className="text-sm text-mute hover:text-paper">
            ✕
          </button>
        </div>

        {asset.music_info && (
          <p className="mt-3 rounded border border-accent/25 bg-accent/5 px-3 py-2 text-xs text-paper/85">
            当前音乐：{asset.music_info.music_name}
            {asset.music_info.mute_original ? "（已关闭原声）" : "（保留原声）"}
            ，再次合成将从原始视频重来
          </p>
        )}

        <div className="mt-3 flex gap-4">
          {/* 左栏：视频预览 + 原声开关（所见即所得） */}
          <div className="min-w-0 flex-1">
            <div className="group relative">
              <video
                ref={videoRef}
                src={previewSrc}
                muted={mute}
                onEnded={stopPlay}
                onClick={toggleVideo}
                onTimeUpdate={(e) => setCurTime(e.currentTarget.currentTime)}
                onLoadedMetadata={(e) => setVideoDur(e.currentTarget.duration || 0)}
                playsInline
                title="点击暂停/播放"
                className="aspect-video w-full cursor-pointer rounded-lg border border-line bg-black"
              />
              {/* 悬停浮现的进度条：可点击/拖动定位（音乐试听中同步音乐进度） */}
              <div className="pointer-events-none absolute inset-x-1.5 bottom-1.5 flex items-center gap-2 rounded bg-black/55 px-2 py-1 opacity-0 transition-opacity duration-150 group-hover:opacity-100">
                <input
                  type="range"
                  min={0}
                  max={videoDur || 0}
                  step={0.1}
                  value={Math.min(curTime, videoDur || 0)}
                  onChange={(e) => seekVideo(Number(e.target.value))}
                  className="pointer-events-auto h-1 flex-1 cursor-pointer accent-emerald-500"
                />
                <span className="shrink-0 text-[10px] tabular-nums text-paper/85">
                  {fmtDur(curTime)} / {fmtDur(videoDur)}
                </span>
              </div>
            </div>
            <div className="mt-3 grid grid-cols-2 gap-2">
              <button
                type="button"
                onClick={() => setMute(false)}
                className={`flex items-center justify-center gap-2 rounded-lg border px-3 py-2 text-xs transition-colors ${
                  !mute
                    ? "border-accent bg-accent/15 text-accent"
                    : "border-line text-mute hover:border-accent/50 hover:text-paper"
                }`}
              >
                <span className="text-sm">🔊</span>
                <span className="text-left leading-tight">
                  保留原声
                  <span className={`block text-[10px] ${!mute ? "text-accent/70" : "text-mute/70"}`}>
                    按下方滑条音量混音
                  </span>
                </span>
              </button>
              <button
                type="button"
                onClick={() => setMute(true)}
                className={`flex items-center justify-center gap-2 rounded-lg border px-3 py-2 text-xs transition-colors ${
                  mute
                    ? "border-accent bg-accent/15 text-accent"
                    : "border-line text-mute hover:border-accent/50 hover:text-paper"
                }`}
              >
                <span className="text-sm">🔇</span>
                <span className="text-left leading-tight">
                  关闭原声
                  <span className={`block text-[10px] ${mute ? "text-accent/70" : "text-mute/70"}`}>
                    合成后仅音乐发声
                  </span>
                </span>
              </button>
            </div>

            {/* 音量滑条：保留原声时的混音增益（预览与合成参数一致）；关原声时不生效 */}
            <div className="mt-2 space-y-2 rounded-lg border border-line px-3 py-2.5">
              <div className="flex items-center gap-2 text-xs">
                <span className="w-14 shrink-0 text-mute">原声音量</span>
                <input
                  type="range"
                  min={0}
                  max={100}
                  step={5}
                  value={videoVolume}
                  disabled={mute}
                  onChange={(e) => setVideoVolume(Number(e.target.value))}
                  className="h-1 flex-1 cursor-pointer accent-emerald-500 disabled:cursor-not-allowed disabled:opacity-30"
                />
                <span className="w-10 shrink-0 text-right tabular-nums text-paper">
                  {mute ? 0 : videoVolume}%
                </span>
              </div>
              <div className="flex items-center gap-2 text-xs">
                <span className="w-14 shrink-0 text-mute">音乐音量</span>
                <input
                  type="range"
                  min={0}
                  max={100}
                  step={5}
                  value={musicVolume}
                  disabled={mute}
                  onChange={(e) => setMusicVolume(Number(e.target.value))}
                  className="h-1 flex-1 cursor-pointer accent-emerald-500 disabled:cursor-not-allowed disabled:opacity-30"
                />
                <span className="w-10 shrink-0 text-right tabular-nums text-paper">
                  {musicVolume}%
                </span>
              </div>
              {mute && (
                <p className="text-[10px] text-mute/70">
                  关闭原声时滑条不生效：音乐按原音量铺底
                </p>
              )}
            </div>
          </div>

          {/* 右栏：音乐选择（点击歌名即与视频同步试听） */}
          <div className="flex w-[620px] shrink-0 flex-col">
            <p className="mb-1.5 text-[11px] font-semibold uppercase tracking-wider text-mute">
              选择音乐（点击即试听）
            </p>
            <div className="max-h-[480px] overflow-y-auto pr-0.5">
              {files === null ? (
                <p className="rounded border border-line p-4 text-xs text-mute">读取音乐库…</p>
              ) : files.length === 0 ? (
                <p className="rounded border border-line p-4 text-xs text-mute">
                  音乐库为空：请先到「设置 → 剪映音乐」一键同步，或把音频文件放进
                  storage/music 目录。
                </p>
              ) : (
                <div className="grid grid-cols-2 gap-3">
                  {files.map((f) => {
                    const active = selected === f.path;
                    return (
                      <div
                        key={f.path}
                        onClick={() => {
                          // 点音乐名即选中并与视频同步从头试听（再点暂停；点别的歌切换）。
                          setSelected(f.path);
                          togglePlay(f.path);
                        }}
                        title={f.file_name}
                        className={`flex h-[84px] cursor-pointer items-center gap-3 rounded-lg border p-3 text-sm ${
                          active
                            ? "border-accent bg-accent/15 text-accent"
                            : "border-line text-paper/85 hover:border-accent/50 hover:bg-white/5"
                        }`}
                      >
                        {f.cover_path ? (
                          <img
                            src={coverUrl(f.cover_path)}
                            alt=""
                            className="h-14 w-14 shrink-0 rounded-md object-cover"
                          />
                        ) : (
                          <span
                            className={`flex h-14 w-14 shrink-0 items-center justify-center rounded-md border border-line/60 text-xl ${
                              active ? "text-accent" : "text-mute"
                            }`}
                          >
                            ♪
                          </span>
                        )}
                        <span className="min-w-0 flex-1 self-stretch py-0.5">
                          {/* 歌名最多两行完整展示，超出才省略 */}
                          <span className="line-clamp-2 leading-snug break-all">{f.name}</span>
                          <span className="mt-1 block text-[11px] text-mute">
                            {durations[f.path] ? fmtDur(durations[f.path]) : "--:--"} ·{" "}
                            {formatBytes(f.size_bytes)}
                          </span>
                        </span>
                        <button
                          type="button"
                          onClick={(e) => {
                            e.stopPropagation();
                            setSelected(f.path);
                            togglePlay(f.path);
                          }}
                          title={playing === f.path ? "暂停" : "试听"}
                          className={`flex h-7 w-7 shrink-0 items-center justify-center rounded-full border text-xs ${
                            playing === f.path
                              ? "border-accent bg-accent text-ink"
                              : "border-line text-mute hover:border-accent/60 hover:text-paper"
                          }`}
                        >
                          {playing === f.path ? "⏸" : "▶"}
                        </button>
                      </div>
                    );
                  })}
                </div>
              )}
            </div>
          </div>
        </div>

        {error && <p className="mt-3 text-xs text-bad">{error}</p>}

        <div className="mt-4 flex justify-end gap-2">
          <button
            type="button"
            onClick={onClose}
            disabled={applying}
            className="h-9 rounded border border-line px-4 text-sm text-mute hover:text-paper disabled:opacity-40"
          >
            取消
          </button>
          <button
            type="button"
            onClick={() => void apply()}
            disabled={!selected || applying}
            className="inline-flex h-9 items-center justify-center gap-1.5 rounded border border-accent bg-accent px-4 text-sm font-medium text-ink hover:brightness-110 disabled:cursor-not-allowed disabled:opacity-40"
          >
            {applying ? (
              <>
                <span className="h-3.5 w-3.5 animate-spin rounded-full border-2 border-ink/40 border-t-ink" />
                合成中…
              </>
            ) : (
              "确认合成"
            )}
          </button>
        </div>
      </div>
    </div>
  );
}
