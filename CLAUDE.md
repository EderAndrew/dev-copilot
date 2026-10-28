# dev-copilot Development Guidelines

Auto-generated from all feature plans. Last updated: 2026-10-27

Normative rules: `.specify/memory/constitution.md` (v1.1.0) and `docs/adr/`.

## Active Technologies

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
pnpm --filter appsdesktop check
pnpm --filter appsdesktop tauri dev
```

## Code Style

Rust: Rust API Guidelines, typed errors, no `unwrap`/`expect` on recoverable paths, `unsafe` only with `// SAFETY:`. TypeScript: strict mode; no business rules in React components.

## Recent Changes

- 001-meeting-sessions: Added Rust core stack (Tauri 2, tokio, cpal, whisper-rs, rusqlite/SQLCipher, keyring) and React/i18next frontend

<!-- MANUAL ADDITIONS START -->
<!-- MANUAL ADDITIONS END -->
