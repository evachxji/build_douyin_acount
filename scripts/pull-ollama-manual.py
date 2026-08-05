#!/usr/bin/env python
"""Ollama 模型手动安装脚本（curl 断点续传）。

背景：本机代理链路会掐断 Go HTTP/2 的 TLS 连接，导致 `ollama pull` 报
`Get https://registry.ollama.ai/...: EOF`，而 curl 的 HTTP/1.1 正常。
本脚本用 curl 拉取 registry 的 manifest 与 blobs（公开模型匿名可拉），
直接写入 Ollama 模型目录，绕过 ollama 自带的下载器。
网络抖动时通过 `-C -` 断点续传自动重试。

用法：
    python pull-ollama-manual.py [model=library/qwen2.5:7b ...]

模型目录：%USERPROFILE%/.ollama/models
"""

import hashlib
import json
import os
import subprocess
import sys
import time
import urllib.request

REGISTRY = "https://registry.ollama.ai"
MODELS = [
    ("library/qwen2.5", "7b"),
    ("library/qwen2.5-vl", "7b"),
]


def sha256_file(path: str) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def download_blob(url: str, dest: str, expected_sha: str, expected_size: int, retries: int = 40) -> None:
    """curl -L -C - 断点续传下载，失败自动重试；完成后校验 sha256。"""
    if os.path.exists(dest) and os.path.getsize(dest) > 0:
        try:
            if sha256_file(dest) == expected_sha:
                print(f"  skip (already complete): {os.path.basename(dest)}")
                return
        except OSError:
            pass
    proxy = os.environ.get("OLLAMA_DL_PROXY", "")
    for attempt in range(1, retries + 1):
        size = os.path.getsize(dest) if os.path.exists(dest) else 0
        print(f"  [{attempt}/{retries}] {os.path.basename(dest)} ({size/1e6:.0f} MB)", flush=True)
        cmd = [
            "curl.exe",
            "-sS",
            "-L",
            "-C",
            "-",
            "--retry",
            "3",
            "--retry-all-errors",
            "--retry-delay",
            "3",
            "--connect-timeout",
            "20",
            "--max-time",
            "600",
        ]
        if proxy:
            cmd += ["-x", proxy]
        cmd += ["-o", dest, url]
        proc = subprocess.run(
            cmd,
            capture_output=True,
            text=True,
        )
        if proc.returncode != 0:
            print(f"    retry: {proc.stderr.strip()[-160:]}", flush=True)
            continue
        # 先按大小判断：不完整就续传，避免把大文件删掉重下。
        size = os.path.getsize(dest) if os.path.exists(dest) else 0
        if size > expected_size:
            # 续传错位导致文件偏大（常见于断连后 Range 被 CDN 忽略而追加数据）：
            # 截断到期望大小，保留前面的有效数据。
            print(f"    fix: truncating {size} -> {expected_size}", flush=True)
            with open(dest, "r+b") as f:
                f.truncate(expected_size)
            size = expected_size
        if size < expected_size:
            print(f"    retry: size {size} < expected {expected_size}", flush=True)
            continue
        # 大小一致才做 sha256 终验。
        actual = sha256_file(dest)
        if actual == expected_sha:
            print(f"  complete: {os.path.basename(dest)}", flush=True)
            return
        print(f"    retry: sha mismatch at full size {size}", flush=True)
        for _ in range(3):
            try:
                os.remove(dest)
                break
            except PermissionError as exc:
                print(f"    delete blocked ({exc}); retry in 3s", flush=True)
                time.sleep(3)
        else:
            raise RuntimeError(f"cannot remove corrupt blob {dest}")
    raise RuntimeError(f"failed to download {dest} after {retries} attempts")


def install_model(repo: str, tag: str, base_dir: str) -> None:
    print(f"== installing {repo}:{tag} ==")
    manifest_url = f"{REGISTRY}/v2/{repo}/manifests/{tag}"
    with urllib.request.urlopen(manifest_url, timeout=30) as resp:
        manifest = json.loads(resp.read())
    blobs_dir = os.path.join(base_dir, "blobs")
    os.makedirs(blobs_dir, exist_ok=True)

    entries = [("config", manifest["config"])] + [("layer", l) for l in manifest["layers"]]
    for kind, entry in entries:
        digest = entry["digest"]
        assert digest.startswith("sha256:")
        sha = digest[len("sha256:"):]
        dest = os.path.join(blobs_dir, f"sha256-{sha}")
        url = f"{REGISTRY}/v2/{repo}/blobs/{digest}"
        print(f"  [{kind}] {digest} ({entry.get('size', '?')} bytes)")
        download_blob(url, dest, sha, int(entry.get("size", 0)))

    manifest_dir = os.path.join(base_dir, "manifests", REGISTRY.removeprefix("https://"), repo, tag)
    os.makedirs(manifest_dir, exist_ok=True)
    manifest_path = os.path.join(manifest_dir, tag)
    with open(manifest_path, "w", encoding="utf-8") as f:
        json.dump(manifest, f)
    print(f"  manifest written: {manifest_path}")


def main() -> int:
    base_dir = os.path.join(os.path.expanduser("~"), ".ollama", "models")
    models = MODELS
    if len(sys.argv) > 1:
        models = []
        for arg in sys.argv[1:]:
            repo, _, tag = arg.partition(":")
            if not tag:
                tag = "7b"
            models.append((repo, tag))
    for repo, tag in models:
        install_model(repo, tag, base_dir)
    print("all done")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
