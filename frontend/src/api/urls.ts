import { convertFileSrc } from "@tauri-apps/api/core";

/** 本地文件路径 -> 前端可播放/展示的 URL（asset protocol）。 */
export const videoUrl = (path: string) => convertFileSrc(path);

export const coverUrl = (path: string | null | undefined) =>
  path ? convertFileSrc(path) : "";

export const frameUrl = (path: string) => convertFileSrc(path);

/** 本地音频文件 -> 可播放 URL（试听用，asset protocol）。 */
export const audioUrl = (path: string) => convertFileSrc(path);
