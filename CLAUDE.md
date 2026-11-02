# dev-copilot Development Guidelines

Auto-generated from all feature plans. Last updated: 2026-11-01

Normative rules: `.specify/memory/constitution.md` (v1.1.0) and `docs/adr/`.

## Active Technologies
- Rust + Tauri 2.12 with `tauri-plugin-updater` (core-driven, minisign-verified); `sysinfo` in `tooling/perf-sampler` only; release via GitHub Actions draft releases, Developer ID + notarization, Authenticode, OpenPGP-signed SHA256SUMS (002-desktop-release-readiness)
- No new migration: `updates.*` keys in the existing `settings` table; release evidence in `docs/release/` (002-desktop-release-readiness)

- Rust stable (edition 2024) + Tauri 2 (`tauri-plugin-dialog`), `tokio`/`tokio-util`, `cpal`, `rtrb`, `rubato`, `whisper-rs`, `reqwest` (rustls), `rusqlite` (SQLCipher), `keyring`, `serde`, `thiserror`, `tracing`, `jsonschema` (001-meeting-sessions)
- TypeScript + React 19 + Vite, `i18next`/`react-i18next`, Vitest + Testing Library + `vitest-axe` (001-meeting-sessions)
- SQLite local encrypted with SQLCipher, key in OS credential store; raw audio never persisted (001-meeting-sessions)

## Project Structure

```text
apps/desktop/src/          React UI (presentation only)
apps/desktop/src-tauri/    Composition root + thin Tauri commands
crates/domain/             Entities, state machines, invariants (no infra deps)
crates/application/        Use cases + ports
crates/audio/              OS audio adapters
crates/intelligence/       Transcription/LLM adapters
crates/persistence/        SQLCipher repositories + secret store
specs/                     Spec Kit feature specs/plans
docs/adr/                  Architecture Decision Records
```

## Commands

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo audit && cargo deny check
pnpm --filter appsdesktop lint && pnpm --filter appsdesktop format:check
pnpm --filter appsdesktop check
pnpm --filter appsdesktop test
pnpm --filter appsdesktop build && pnpm --filter appsdesktop test:dist   # production bundle has no inline styles
node --test "tooling/release/*.test.mjs"
pnpm --filter appsdesktop tauri dev

# CPU/memory of the installed app (docs/release/v0.1.0/runbook-us5.md)
cargo run -p perf-sampler --release -- sample --process "Dev Copilot" --interval 5 --out samples.csv
cargo run -p perf-sampler --release -- summarize --samples samples.csv --warmup-min 10
cargo run -p perf-sampler --release -- summarize --metrics metrics-<date>.csv   # DEVCOPILOT_METRICS=1 latencies
```

Full CI gate list: `specs/002-desktop-release-readiness/quickstart.md` §1. Release tag procedure:
`docs/release/signing-and-distribution.md` ("Cortar uma tag"); rollback: `docs/release/rollback.md`.

## Code Style

Rust: Rust API Guidelines, typed errors, no `unwrap`/`expect` on recoverable paths, `unsafe` only with `// SAFETY:`. TypeScript: strict mode; no business rules in React components.

## Recent Changes
- 002-desktop-release-readiness: strict CSP (no `'unsafe-inline'`), signed installers per OS, verified in-app updates, release checklist/evidence

- 001-meeting-sessions: Added Rust core stack (Tauri 2, tokio, cpal, whisper-rs, rusqlite/SQLCipher, keyring) and React/i18next frontend

<!-- MANUAL ADDITIONS START -->
<!-- MANUAL ADDITIONS END -->
