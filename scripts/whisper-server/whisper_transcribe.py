#!/usr/bin/env python
"""Whisper 转写 sidecar。

用法：
    python whisper_transcribe.py <audio.wav> <model>

输出：stdout 打印 JSON { "text": "...", "language": "zh" }
模型：faster-whisper 模型名，如 base / small。
"""

import json
import sys


def main() -> int:
    if len(sys.argv) != 3:
        print(json.dumps({"error": "usage: whisper_transcribe.py <wav> <model>"}))
        return 2
    wav_path, model_name = sys.argv[1], sys.argv[2]
    try:
        from faster_whisper import WhisperModel

        try:
            model = WhisperModel(model_name, device="auto", compute_type="default")
            segments, info = model.transcribe(wav_path, vad_filter=True)
        except Exception:
            # CUDA 库缺失（如 cublas64_12.dll）时回退 CPU，保证本地可用。
            model = WhisperModel(model_name, device="cpu", compute_type="int8")
            segments, info = model.transcribe(wav_path, vad_filter=True)
        text = "".join(seg.text for seg in segments).strip()
        print(json.dumps({"text": text, "language": info.language}, ensure_ascii=False))
        return 0
    except Exception as exc:  # noqa: BLE001 - sidecar 必须把任何异常转成可解析 JSON
        print(json.dumps({"error": str(exc)}, ensure_ascii=False))
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
