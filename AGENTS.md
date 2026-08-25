# Repository Guidelines

Windows desktop app (Tauri 2 + Rust) collecting short videos from TikTok / Instagram Reels, analyzing them with local AI (Ollama + Whisper), and assisting publishing to Douyin. Human-in-the-loop: publishing is always user-confirmed.

## Project Structure & Module Organization

- `frontend/` - React 18 + TypeScript + TailwindCSS (Vite). UI only; IPC goes through `src/api/client.ts`, state in Zustand stores under `src/stores/`.
- `src-tauri/` - Rust backend with all business logic: `commands/` (IPC), `agents/` (task orchestration), `analysis/` (pipeline), `db/` (SQLite + migrations), `domain/` (models, state machine). Integration tests in `tests/`, examples in `examples/`.
- `scripts/` - Node/Python sidecars spawned by Rust agents: `collector/`, `douyin-publisher/`, `douyin-sync/`, `whisper-server/`.
- `resources/prompts/` - AI prompt templates; `docs/` - design docs (01-05); `storage/` - runtime data, gitignored.

## Build, Test, and Development Commands

```powershell
pnpm tauri dev            # dev: launches Vite + Tauri app
pnpm tauri build          # release bundle in src-tauri/target/release/bundle/
cd src-tauri
cargo test                # all Rust tests
cargo test --test agents_test   # single test file
cargo clippy --all-targets      # lint (zero warnings; stop app first)
cd ..\frontend
pnpm dev                  # Vite, port 1420
pnpm test                 # Vitest suite
pnpm build                # tsc + Vite build (no ESLint)
```

## Coding Style & Naming Conventions

- Rust: `snake_case`; clippy must pass with zero warnings. Persisted enums use `SCREAMING_SNAKE_CASE` (e.g. `LOW_PRIORITY`).
- TypeScript/React: `camelCase` variables/functions, `PascalCase` components; type correctness enforced by `tsc` in `pnpm build`.
- Match surrounding style; don't reformat unrelated code.

## Testing Guidelines

- Rust: integration tests in `src-tauri/tests/*_test.rs` plus inline unit tests. Tests must not depend on real external services (Ollama, FFmpeg, Chrome); reuse `tests/fixtures/` stubs and isolate storage with `tempfile::tempdir()`.
- Frontend: Vitest, `*.test.ts` files colocated next to source (e.g. `src/lib/format.test.ts`).
- Add regression tests for fragile behavior (collector stats, publisher heartbeat, schedule selection).

## Commit & Pull Request Guidelines

- Commit messages are Chinese, formatted `type(scope): description`, e.g. `feat(分析): LLM 逐条判定命中/排除条件`. Types: `feat`, `fix`, `test`, `docs`, `refactor`, `perf`, `chore`.
- Keep each commit scoped to one logical change. Pull requests state what changed and why and note any manual verification.

## Agent-Specific Instructions

- Read `CLAUDE.md` before touching business logic; it records architecture contracts and pitfalls.
- Never automate clicking Douyin's final publish button; publishing stays human-confirmed.
- PowerShell inline scripts with Chinese text break under GBK; write them to files and read UTF-8 docs with `Get-Content -Encoding UTF8`.
