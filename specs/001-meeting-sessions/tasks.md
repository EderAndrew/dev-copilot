---

description: "Task list for feature 001-meeting-sessions"
---

# Tasks: Sessões de reunião e assistência técnica incremental

**Input**: Design documents from `/specs/001-meeting-sessions/`
**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/, quickstart.md

**Tests**: Tests are REQUIRED (Constitution Art. VIII): unit tests for domain invariants and use cases, integration/contract tests for adapter boundaries, and security acceptance tests (Art. IV.16). Write tests first and confirm they fail before implementing.

**Organization**: Tasks are grouped by user story. Story phases follow spec priority (P1 first; US8 is P2). US9 (pausa, P1 after clarification) is placed after the stories it extends.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies on incomplete tasks)
- **[Story]**: User story label (US1…US9)

## Path Conventions

- **Rust core**: `crates/{domain,application,audio,intelligence,persistence}/src/`, unit tests in `#[cfg(test)]` modules, integration tests in `crates/<crate>/tests/`
- **Composition root / IPC**: `apps/desktop/src-tauri/src/`, capabilities in `apps/desktop/src-tauri/capabilities/`
- **Frontend**: `apps/desktop/src/`
- **Feature docs/contracts**: `specs/001-meeting-sessions/`
- **ADRs**: `docs/adr/`

Global rules for every task: no `unwrap`/`expect` on recoverable paths; session content never logged (use `Redacted<T>`); every IPC command validates input in Rust; UI strings go through i18n (pt-BR + en).

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Toolchain, dependencies, CI and baseline configuration

- [X] T001 Add `[workspace.dependencies]` to root `Cargo.toml` for tokio, tokio-util, serde, serde_json, thiserror, tracing, tracing-subscriber, tracing-appender, uuid (v7), time, cpal, rtrb, rubato, whisper-rs, reqwest (rustls-tls, json), rusqlite (bundled-sqlcipher), keyring, jsonschema, sha2, wiremock, tempfile; pin exact versions after checking `cargo audit`/license
- [X] T002 [P] Configure crate dependencies per architecture in `crates/domain/Cargo.toml` (uuid, time, thiserror only), `crates/application/Cargo.toml` (domain, tokio, tokio-util, thiserror, tracing), `crates/audio/Cargo.toml`, `crates/intelligence/Cargo.toml`, `crates/persistence/Cargo.toml`; replace the template `add()` in each `crates/*/src/lib.rs` with module declarations — native deps deferred: `rusqlite`/`keyring` to T011 (S3 picks the SQLCipher crypto backend), `whisper-rs` to T071 (needs CMake); module declarations are added by the tasks that create each module
- [X] T003 [P] Add `rustfmt.toml` and `clippy.toml` at repo root and `[workspace.lints]` in root `Cargo.toml` denying `clippy::unwrap_used`, `clippy::expect_used` (allowed in tests) and `unsafe_code` (to be allowed per-module only where documented)
- [X] T004 [P] Create `deny.toml` at repo root with advisories, license allowlist (MIT, Apache-2.0, BSD, ISC, Unicode, Zlib, MPL-2.0) and bans for duplicate major versions as warnings
- [X] T005 [P] Create `.github/workflows/ci.yml` with matrix macos-latest/windows-latest/ubuntu-latest running cargo fmt --check, cargo clippy --workspace --all-targets -D warnings, cargo test --workspace, cargo audit, cargo deny check, gitleaks, pnpm install --frozen-lockfile, pnpm audit --prod --audit-level high, pnpm --filter appsdesktop check/lint/format:check/test (install Linux deps: libwebkit2gtk-4.1-dev, libasound2-dev, libpulse-dev, libssl-dev)
- [X] T006 [P] Update `turbo.json` adding `test` outputs `[]`, Rust inputs (`crates/**/*.rs`, `Cargo.toml`, `Cargo.lock`) for desktop `build`/`check`, and add `test` script to root `package.json` and `apps/desktop/package.json`
- [X] T007 [P] Add frontend dev/runtime deps (i18next, react-i18next, vitest, @testing-library/react, @testing-library/user-event, jsdom, vitest-axe, eslint config) to `apps/desktop/package.json` and create `apps/desktop/vitest.config.ts` with jsdom environment; add scripts `"lint": "eslint ."` and `"format:check": "prettier --check \"src/**/*.{ts,tsx,json}\""` to `apps/desktop/package.json`, and a root `"format:check"` script mirroring the existing `format`
- [X] T008 Replace `tauri-plugin-opener` with `tauri-plugin-dialog` in `apps/desktop/src-tauri/Cargo.toml`, `apps/desktop/src-tauri/src/lib.rs`, `apps/desktop/package.json`, and set `apps/desktop/src-tauri/capabilities/default.json` permissions to `core:default` + `dialog:allow-save` only
- [X] T009 [P] Add `apps/desktop/src-tauri/Info.plist` with `NSMicrophoneUsageDescription` (pt-BR/en text) and reference it from `apps/desktop/src-tauri/tauri.conf.json` bundle macOS settings
- [X] T010 [P] Remove the template greet UI from `apps/desktop/src/App.tsx`, `apps/desktop/src/App.css`, `apps/desktop/src/assets/react.svg` and the `greet` command in `apps/desktop/src-tauri/src/lib.rs`

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Core types, ports, encrypted storage, IPC plumbing, i18n shell. Blocks all stories.

**⚠️ CRITICAL**: No user story work can begin until this phase is complete

### Spike S3 (blocking) and ADR

- [X] T011 Spike S3: build a throwaway test in `crates/persistence/tests/spike_sqlcipher.rs` opening an SQLCipher DB with a key stored via `keyring` on macOS, Windows and Linux CI runners; record results and the decision in `docs/adr/ADR-004.md` (armazenamento local cifrado: context, options, decision, tradeoffs, review triggers, deletion limitations)

### Domain primitives

- [X] T012 [P] Implement id newtypes (SessionId, SegmentId, RecordId, ChangeId, QuestionId, AnalysisRunId, ProviderId, ExportId) over UUID v7 in `crates/domain/src/ids.rs`
- [X] T013 [P] Implement `Timestamp` and `Duration` value types and `Clock` trait-free helpers in `crates/domain/src/time.rs`
- [X] T014 [P] Implement `Redacted<T>` with `Debug`/`Display` printing `[redacted]` plus unit tests in `crates/domain/src/redacted.rs`
- [X] T015 [P] Implement `DomainError` (InvalidTransition, InvalidInput{field}, InvariantViolation) in `crates/domain/src/error.rs`
- [X] T016 [P] Implement bounded text value types (Title 1–200, RecordText 1–2000, SegmentText 1–10000, QuestionText 1–2000) rejecting control characters, with unit tests, in `crates/domain/src/text.rs`
- [X] T017 [P] Implement `SessionLanguage` (pt-BR, en) and `UiLanguage` in `crates/domain/src/language.rs`
- [X] T018 [P] Implement Provider entity (role, locality, adapter kind, endpoint with https-required-for-remote validation, model, credential_ref, data_categories) with unit tests in `crates/domain/src/provider.rs`

### Application ports and test support

- [X] T019 Define all ports from `specs/001-meeting-sessions/contracts/provider-ports.md` (AudioSource, Transcriber, AnalysisProvider, RemoteGate, SessionRepository, SegmentRepository, RecordRepository, QuestionRepository, ProviderRepository, SecretStore, Clock, EventPublisher) in `crates/application/src/ports.rs`
- [X] T020 [P] Define `SessionEvent` and related DTO-neutral event types matching `contracts/ipc-events.md` in `crates/application/src/events.rs`
- [X] T021 [P] Define `ApplicationError` mapping domain/port errors to stable codes (invalid_input, not_found, invalid_state, permission_denied, source_unavailable, no_transcriber, secret_store_unavailable, authorization_required, provider_error, storage_error, internal) in `crates/application/src/error.rs`
- [X] T022 Implement in-memory fakes for every port (fake clock, scripted audio source, scripted transcriber, scripted analysis provider, in-memory repos, recording event publisher, toggleable secret store) behind feature `test-support` in `crates/application/src/testing.rs`

### Encrypted persistence

- [X] T023 Implement `SecretStore` over `keyring` (db key and provider credentials, distinct error for unavailable store) in `crates/persistence/src/secret_store.rs`
- [X] T024 Implement DB open/create with 256-bit random key from SecretStore, `PRAGMA key`, `secure_delete=ON`, `journal_mode=WAL`, `foreign_keys=ON`, user-only file permissions in app data dir, in `crates/persistence/src/db.rs`
- [X] T025 Implement migration runner via `PRAGMA user_version` and migration v1 creating all tables from data-model.md (sessions, session_sources, segments, records, record_evidence, record_changes, analysis_runs, questions, question_evidence, providers, provider_authorizations, exports, settings) with `ON DELETE CASCADE` in `crates/persistence/src/migrations/mod.rs` and `crates/persistence/src/migrations/v1.sql`
- [X] T026 [P] Integration test: DB refuses to open/write when SecretStore is unavailable (FR-039a) in `crates/persistence/tests/secret_store_unavailable.rs`
- [X] T027 [P] Integration test: migrations apply idempotently on a fresh and an existing DB in `crates/persistence/tests/migrations.rs`
- [X] T028 [P] Implement ProviderRepository (providers only; authorizations added in US7) and settings repository in `crates/persistence/src/repos/provider_repo.rs` and `crates/persistence/src/repos/settings_repo.rs`

### Provider and settings services (local only)

- [X] T029 Implement `provider_service` basics (list, upsert local providers, set default per role, resolve usable transcriber/analyzer) in `crates/application/src/provider_service.rs` with tests using fakes
- [X] T030 [P] Implement `settings_service` (ui_language, consent_reminder_dismissed, analysis_limits defaults per research R7: trigger 6 finals / 90 s, min interval 30 s, 40 calls per hour, per-session token cap per provider) in `crates/application/src/settings_service.rs` with tests

### Tauri plumbing

- [X] T031 Implement composition root wiring repositories, secret store, services and tracing (file appender in app log dir, no content fields) in `apps/desktop/src-tauri/src/composition.rs` and `apps/desktop/src-tauri/src/lib.rs`
- [X] T032 [P] Implement `IpcError` (code, messageKey, details) and `From<ApplicationError>` in `apps/desktop/src-tauri/src/commands/error.rs`
- [X] T033 [P] Implement `EventPublisher` bridge to per-session `tauri::ipc::Channel<SessionEvent>` with seq numbering in `apps/desktop/src-tauri/src/events.rs`
- [X] T034 Implement settings and provider listing commands (`settings_get`, `settings_update`, `provider_list`, `provider_upsert` for local, `provider_set_default`) with input validation in `apps/desktop/src-tauri/src/commands/settings.rs` and `apps/desktop/src-tauri/src/commands/providers.rs`
- [X] T035 [P] Create shared contract fixtures directory with sample DTO/event JSON in `specs/001-meeting-sessions/contracts/fixtures/` and a Rust test deserializing all fixtures in `apps/desktop/src-tauri/tests/contract_fixtures.rs`

### Frontend shell

- [X] T036 [P] Create IPC types matching contracts and `invoke` wrappers returning typed `IpcError` in `apps/desktop/src/ipc/types.ts` and `apps/desktop/src/ipc/client.ts`, plus a Vitest contract test reading `specs/001-meeting-sessions/contracts/fixtures/` in `apps/desktop/src/ipc/contract.test.ts`
- [X] T037 [P] Set up i18next with `apps/desktop/src/i18n/index.ts`, `apps/desktop/src/i18n/pt-BR.json`, `apps/desktop/src/i18n/en.json` and a completeness test asserting identical key sets (SC-019) in `apps/desktop/src/i18n/i18n.test.ts`
- [X] T038 Create app shell with persistent header slot for the capture indicator, navigation (Sessões, Provedores, Configurações) and language switcher in `apps/desktop/src/App.tsx` and `apps/desktop/src/components/Layout.tsx`
- [X] T039 [P] Implement settings screen (UI language, analysis limits) in `apps/desktop/src/features/settings/SettingsPage.tsx`

**Checkpoint**: Encrypted DB, ports, fakes, IPC and UI shell ready

**Phase 2 notes** (2026-10-28):
- Contract vocabulary pulled forward so ports/events compile: `SessionState`/`FailureReason` in `crates/domain/src/session.rs` (T046 adds the `Session` entity and transitions) and `GapReason` in `crates/domain/src/transcript.rs` (T068 adds `TranscriptSegment`).
- Ports for story entities grow with their stories: `SessionRepository` (T048), `SegmentRepository` (T069), `RecordRepository` (T088), `QuestionRepository` (T113), `AnalysisProvider::analyze/answer` (T089–T091, T114); a `SettingsRepository` port was added for T028/T030.
- T011: ADR-004 stays *Proposto* until the Windows and Linux CI rows of spike S3 pass (macOS verified locally).

---

## Phase 3: User Story 1 - Controlar uma sessão com captura de microfone autorizada (Priority: P1) 🎯 MVP

**Goal**: Create/start/stop sessions with authorized microphone capture, visible capture state, failures handled without hidden recording.

**Independent Test**: In the app, create → start (permission prompt) → stop on each OS; deny permission and confirm no capture; with application fakes, verify state machine, single active session and stop timing.

### Tests for User Story 1 (REQUIRED) ⚠️

- [X] T040 [P] [US1] Exhaustive state-machine tests (all valid/invalid transitions incl. paused, failure reasons, terminal states) in `crates/domain/src/session.rs` test module
- [X] T041 [P] [US1] Use-case tests: start blocked without permission/transcriber/secret store, consent reminder required, single active session (FR-006), stop emits indicator off and cancels sources (SC-002 with fake clock), cancel during `starting` → `idle` with sources closed (FR-003), sole source loss → failed with content kept, permission revoked mid-capture stops that source and emits `source_status: permission_revoked` (FR-010), default-device change never switches to another device (FR-011, Art. V.4), OS suspend/lock handled as source loss without auto-resume, in `crates/application/tests/session_service.rs`
- [X] T042 [P] [US1] Startup recovery test: non-terminal sessions become `failed(interrupted)` and get a trailing `gap(interrupted)` after the last persisted final (FR-007, US2.3) in `crates/application/tests/recovery.rs`
- [X] T043 [P] [US1] Repository tests for sessions and sources in `crates/persistence/tests/session_repo.rs`
- [X] T044 [P] [US1] IPC command validation tests (bad ids, title bounds, unknown source kind, start without consent, `session_cancel_start` outside `starting`) in `apps/desktop/src-tauri/tests/session_commands.rs`
- [X] T045 [P] [US1] Frontend tests: CaptureIndicator shows text+icon, `aria-live`, axe clean; controls keyboard-operable, in `apps/desktop/src/features/sessions/CaptureIndicator.test.tsx` and `apps/desktop/src/features/sessions/SessionControls.test.tsx`

### Implementation for User Story 1

- [X] T046 [P] [US1] Implement Session entity, SessionState, FailureReason and transition functions in `crates/domain/src/session.rs`
- [X] T047 [P] [US1] Implement AudioSourceKind, Availability, UnavailableReason, PermissionState in `crates/domain/src/audio_source.rs` (pulled into Phase 2: the T019 `AudioSource` port needs it)
- [X] T048 [US1] Implement SessionRepository (create, get, update_state, list, mark_interrupted_on_startup — appending `gap(interrupted)` once T069 lands) in `crates/persistence/src/repos/session_repo.rs`
- [X] T049 [US1] Implement `capture_pipeline` skeleton: open sources into bounded sink, per-session CancellationToken, source-loss handling, indicator events, in `crates/application/src/capture_pipeline.rs`
- [X] T050 [US1] Implement `session_service` (create, start with checks FR-009/FR-012/FR-039a/FR-045a, stop returning immediately, recovery on startup) in `crates/application/src/session_service.rs`
- [X] T051 [P] [US1] Implement cpal microphone adapter (device enumeration, permission query/request, callback writing only to `rtrb` producer, no silent device switch FR-011) in `crates/audio/src/cpal_mic.rs`
- [X] T052 [P] [US1] Implement resampler/downmixer to 16 kHz mono f32 outside the callback in `crates/audio/src/resample.rs` with unit tests on synthetic signals
- [X] T053 [P] [US1] Add ignored-by-default device integration test for mic capture in `crates/audio/tests/mic_device.rs`
- [X] T054 [US1] Implement commands `session_create`, `session_start`, `session_cancel_start`, `session_stop`, `session_subscribe`, `audio_sources_list`, `audio_permission_request`, `audio_permission_help` in `apps/desktop/src-tauri/src/commands/sessions.rs` and `apps/desktop/src-tauri/src/commands/audio.rs`
- [X] T055 [P] [US1] Implement session event store/reducer (applies events by seq, resubscribe on reconnect) in `apps/desktop/src/features/sessions/sessionStore.ts`
- [X] T056 [P] [US1] Implement NewSessionForm (title, language, source selection with availability reasons) in `apps/desktop/src/features/sessions/NewSessionForm.tsx`
- [X] T057 [P] [US1] Implement ConsentReminderDialog (FR-012, "não mostrar novamente") in `apps/desktop/src/features/sessions/ConsentReminderDialog.tsx`
- [X] T058 [US1] Implement SessionControls (start/stop, states, failure messages) and CaptureIndicator in header in `apps/desktop/src/features/sessions/SessionControls.tsx` and `apps/desktop/src/features/sessions/CaptureIndicator.tsx`
- [X] T059 [P] [US1] Implement PermissionHelp per OS (FR-010) in `apps/desktop/src/features/sessions/PermissionHelp.tsx`
- [X] T060 [US1] Add US1 strings to `apps/desktop/src/i18n/pt-BR.json` and `apps/desktop/src/i18n/en.json`

**Checkpoint**: Sessions start/stop with mic capture under test fakes; full manual flow requires a transcriber from US2 (FR-045a)

**Phase 3 notes** (2026-10-28):
- `AudioSource::start` gained a `SourceFaultNotifier` argument: adapters report `SourceFault::{Lost, PermissionRevoked}` and stop; the pipeline closes that source and never reopens or switches device (FR-010, FR-011). Default-device changes and OS suspend/lock surface as `Lost`.
- `SessionRepository` is `create/get/update/list/list_active/save_interrupted`; `save_interrupted` writes the trailing `gap(interrupted)` directly into `segments` until T069 adds `SegmentRepository`.
- Synchronous start checks (another active session, consent, secret store, transcriber, unavailable source, already denied/revoked permission) return an error and keep the session `idle`; a prompt denied during `starting` or a source failing to open ends in `failed(permission_denied | source_unavailable)`.
- Until T073 the capture pipeline drains and discards the audio queue (`capture_pipeline::discard_audio`); nothing is persisted.
- `session_subscribe` takes `channel` as a top-level command argument (Tauri cannot deserialize a `Channel` inside `request`) and returns the current `SessionDto`; contract updated.
- `cpal` has no permission API: `CpalMicrophone` tracks the last observed state and `request_permission` opens a short probe stream (triggers the macOS prompt). On macOS an undetermined permission may yield silence rather than an error; verify in the acceptance matrix (T162).
- System audio is registered as `UnavailableSource(not_validated_on_platform)` until spike S1 (US8).

---

## Phase 4: User Story 2 - Acompanhar a transcrição progressiva (Priority: P1)

**Goal**: Progressive transcript with partial/final segments, timestamps, source, gaps; finals persisted immediately.

**Independent Test**: Play a known non-confidential audio during a session; verify partial→final, timestamps, gaps on overload/provider failure, and finals preserved after forced kill.

### Tests for User Story 2 (REQUIRED) ⚠️

- [X] T061 [P] [US2] Domain tests for TranscriptSegment (final/gap, offsets, source required with 2 sources) in `crates/domain/src/transcript.rs` test module
- [X] T062 [P] [US2] Pipeline tests: partials never persisted, finals persisted on arrival, overload drops oldest and emits `gap(overload)`, provider failure emits `gap(provider_failure)`, stop drains ≤ 5 s then `gap(cancelled_on_stop)`, controls respond while transcriber hangs (SC-006), `transcription_lag` emitted above 15 s and cleared on recovery (US2.4), storage write failure (disk full) emits `storage_error` and stops capture as `failed(storage_error)` with prior finals intact, in `crates/application/tests/transcription_pipeline.rs`
- [X] T063 [P] [US2] Segment repository tests incl. crash-safety (write, reopen, read) in `crates/persistence/tests/segment_repo.rs`
- [X] T064 [P] [US2] Model download tests: rejects hash mismatch, requires explicit call, no session data in request, in `crates/intelligence/tests/model_download.rs`
- [X] T065 [P] [US2] Whisper adapter test on a short non-confidential fixture (ignored unless model path env var set) in `crates/intelligence/tests/whisper_local.rs`
- [X] T066 [P] [US2] Frontend transcript tests: partial styling distinct, gap rendering, lag indicator, timestamps, screen-reader labels in `apps/desktop/src/features/transcript/TranscriptView.test.tsx`

### Implementation for User Story 2

- [X] T067 [US2] Spike S2: measure partial/final latency of whisper models on reference hardware per OS; record reference hardware in `specs/001-meeting-sessions/quickstart.md` and decision in `docs/adr/ADR-003.md` (provedores de transcrição e IA)
- [X] T068 [P] [US2] Implement TranscriptSegment, SegmentKind, GapReason (incl. `interrupted`) in `crates/domain/src/transcript.rs`
- [X] T069 [P] [US2] Implement SegmentRepository (append_final, append_gap in transactions, list_by_session) in `crates/persistence/src/repos/segment_repo.rs`
- [X] T070 [P] [US2] Implement energy-based VAD and sliding-window chunker (partial every ~2 s, final on pause or ~25 s) in `crates/intelligence/src/vad.rs` with unit tests
- [X] T071 [US2] Implement `Transcriber` over whisper-rs (model load from user-selected path, language from session, cancellation) in `crates/intelligence/src/whisper_local.rs`
- [X] T072 [P] [US2] Implement explicit model download with pinned URL list and SHA-256 verification in `crates/intelligence/src/model_download.rs`
- [X] T073 [US2] Extend `capture_pipeline` with resample → bounded chunk channel (~30 s) → transcriber → partial/final/gap events and persistence in `crates/application/src/capture_pipeline.rs`
- [X] T074 [US2] Wire whisper provider registration and `local_model_download` command with progress events in `apps/desktop/src-tauri/src/composition.rs` and `apps/desktop/src-tauri/src/commands/providers.rs`
- [X] T075 [P] [US2] Implement TranscriptView (auto-scroll with pause on user scroll, partial/final/gap, lag indicator from `transcription_lag`, source tags) in `apps/desktop/src/features/transcript/TranscriptView.tsx`
- [X] T076 [P] [US2] Implement LocalModels screen (choose/download model, shows origin and size) in `apps/desktop/src/features/providers/LocalModels.tsx`
- [X] T077 [P] [US2] Show `provider_in_use` (local/remote per stage: transcription, analysis, questions) in `apps/desktop/src/features/sessions/ProviderBadge.tsx` (FR-050)
- [X] T078 [US2] Add US2 strings to `apps/desktop/src/i18n/pt-BR.json` and `apps/desktop/src/i18n/en.json`

**Checkpoint**: US1 + US2 form the first demonstrable increment (capture + live transcript)

**Phase 4 notes** (2026-10-28):
- The transcription stage lives in `crates/application/src/transcription.rs` (spawned by `session_service`, fed by the `capture_pipeline` audio queue) rather than inside `capture_pipeline.rs`. Resampling stays in the audio adapter (`audio::resample`, T052): chunks reach the pipeline already 16 kHz mono.
- New ports: `TranscriberFactory` (cheap build of the resolved provider's transcriber; `Unavailable` → `no_transcriber` at start) and `SegmentRepository::{append, list_by_session, next_seq}` (one append for finals and gaps, each its own transaction).
- Overload policy: backlog ≤ 30 s; on overflow the oldest 15 s are dropped at once and `gap(overload)` is written immediately. Lag = audio captured but not yet handed to the transcriber (> 15 s reported, cleared at ≤ 5 s). Provider failure writes `gap(provider_failure)` and restarts the transcriber; 3 failures in a row without a final fail the session as `no_transcriber`. Losing the only source writes `gap(source_lost)` for unfinalized audio.
- Whisper models come from a pinned catalog (revision `5359861c…`, SHA-256 per file) and are stored in `<app data>/models`; "user-selected" model = chosen from the catalog in `LocalModels`, so the frontend needs no `fs`/open-dialog capability. Commands `local_model_list` and `local_model_download` (progress over a `Channel`) are in `contracts/ipc-commands.md`.
- `whisper-rs` 0.16 `set_abort_callback_safe` is broken (every inference aborts with −6); not used. Cancel returns immediately and the window in flight finishes in the background (ADR-003).
- T067: ADR-003 stays *Proposto* until spike S2 runs on Windows and Linux reference hardware (macOS/M4 measured: partial ≈ 2.3 s after speech onset, final ≈ 0.2 s after the window closes, `base` model).
- Build now needs CMake (whisper.cpp); `deny.toml` allows Unlicense for `whisper-rs`/`whisper-rs-sys` only.
- Fixtures `crates/intelligence/tests/fixtures/audio/*.wav` are synthetic macOS TTS voices, not recordings of people.

---

## Phase 5: User Story 3 - Receber análise técnica incremental estruturada (Priority: P1)

**Goal**: Incremental structured proposals with nature, status, provenance and history; confirmed records never overwritten by AI.

**Independent Test**: With reference transcripts and a fake provider, verify kinds, natures, evidence, status transitions, conflict handling and prompt-injection resistance.

### Tests for User Story 3 (REQUIRED) ⚠️

- [X] T079 [P] [US3] Domain tests: record status transitions, `observed` requires evidence, AI actor cannot change existing records, user changes produce RecordChange, in `crates/domain/src/record.rs` test module
- [X] T080 [P] [US3] Output validation tests against `specs/001-meeting-sessions/contracts/analysis-output.schema.json` (unknown fields, observed without evidence, oversize, wrong version → InvalidOutput; evidence references keep the original segment text, never translated — FR-053b) in `crates/intelligence/tests/output_validation.rs`
- [X] T081 [P] [US3] Prompt-injection suite (≥ 20 cases pt-BR/en in `crates/intelligence/tests/fixtures/injection/`) asserting zero status changes, deletions or extra effects (SC-010) in `crates/application/tests/prompt_injection.rs`
- [X] T082 [P] [US3] Analysis service tests: scheduling (6 finals or 90 s, min 30 s, 1 in flight, hourly cap, per-session token cap → `rate_limited`), conflict → new proposed flagged, duplicate flagged, evidence ids from other sessions discarded, provider failure leaves records intact and capture unaffected, in `crates/application/tests/analysis_service.rs`
- [X] T083 [P] [US3] Evaluation harness with annotated non-confidential transcripts in `crates/intelligence/tests/fixtures/eval/` measuring (a) recall of explicit requirements/decisions ≥ 80%, (b) grounding: 100% of `observed` records cite supporting segments, (c) invented-item rate (records with no basis in the transcript), per SC-008 and Art. VI.6. Runs against the scripted provider by default (harness self-test) and against each reference provider (Ollama, remote) when `DEVCOPILOT_EVAL_PROVIDER` is set; results recorded in `docs/adr/ADR-003.md`, in `crates/intelligence/tests/evaluation.rs`
- [X] T084 [P] [US3] Record repository tests (insert proposed, apply user change with history, list) in `crates/persistence/tests/record_repo.rs`
- [X] T085 [P] [US3] Frontend tests: record cards show kind/nature/status/conflict badges, confirm/reject/pending via keyboard, in `apps/desktop/src/features/records/RecordCard.test.tsx`

### Implementation for User Story 3

- [X] T086 [P] [US3] Implement TechnicalRecord, RecordKind (incl. assumption), Nature, RecordStatus, Origin, RecordFlags and transitions in `crates/domain/src/record.rs`
- [X] T087 [P] [US3] Implement RecordChange and AnalysisRun in `crates/domain/src/record_change.rs` and `crates/domain/src/analysis_run.rs`
- [X] T088 [US3] Implement RecordRepository and AnalysisRun repository in `crates/persistence/src/repos/record_repo.rs` and `crates/persistence/src/repos/analysis_run_repo.rs`
- [X] T089 [P] [US3] Implement prompt builder (delimited untrusted data, existing-record summary, session language, no tools) in `crates/intelligence/src/prompt.rs`
- [X] T090 [P] [US3] Implement JSON schema validation and conversion to domain proposals in `crates/intelligence/src/output_validation.rs` (embed schema from contracts)
- [X] T091 [US3] Implement Ollama `AnalysisProvider` (local `127.0.0.1` endpoint configured by user, timeouts, cancellation) in `crates/intelligence/src/ollama.rs`
- [X] T092 [US3] Implement `analysis_service` (scheduler, limits from settings, dedup by normalized text, create proposals only) in `crates/application/src/analysis_service.rs`
- [X] T093 [US3] Implement `record_service` (user status changes, history) in `crates/application/src/record_service.rs`
- [X] T094 [US3] Hook analysis scheduler into the session pipeline without blocking capture in `crates/application/src/capture_pipeline.rs`
- [X] T095 [US3] Implement commands `record_set_status` in `apps/desktop/src-tauri/src/commands/records.rs` and emit `record_proposed`/`analysis_status` events
- [X] T096 [P] [US3] Implement RecordsPanel and RecordCard (badges, evidence links scrolling to transcript segments, conflict/duplicate markers) in `apps/desktop/src/features/records/RecordsPanel.tsx` and `apps/desktop/src/features/records/RecordCard.tsx`
- [X] T097 [US3] Add US3 strings to `apps/desktop/src/i18n/pt-BR.json` and `apps/desktop/src/i18n/en.json`

**Checkpoint**: Live session produces reviewable structured proposals

**Phase 5 notes** (2026-10-28):
- T081 lives in `crates/intelligence/tests/prompt_injection.rs` (22 cases in `fixtures/injection/cases.json`) instead of `crates/application/tests/`: each attack runs through the real prompt builder and schema validation and then the real `AnalysisService` with in-memory stores; `application` cannot depend on `intelligence`.
- T094: like the transcription stage (Phase 4 notes), analysis is hooked in `transcription.rs` (persisted finals go to an `AnalysisFeed` without waiting) and `session_service.rs` (attach on `capturing`; on stop or failure scheduling stops and a run in flight gets `DRAIN_DEADLINE` before it is cancelled) rather than `capture_pipeline.rs`.
- New ports: `AnalysisProvider::analyze`, `AnalysisProviderFactory`, `RecordRepository` (`insert` with the creation entry, `update` with history, `get`, `list_by_session`, `history`) and `AnalysisRunRepository`. Runs and records start only in `capturing` (no analysis without an analysis provider; capture is unaffected).
- Scheduling (R7): 6 finals or 90 s of speech, ≥ 30 s apart, one run in flight; the hourly cap is shared across sessions (in memory) and the token budget is per session and provider (from provider-reported usage). A capped run is recorded once as `rate_limited`; failed runs (`provider_error`, `invalid_output`) keep their segments pending for the next run; at most 60 segments per run.
- Proposal rules: evidence outside the session is dropped and an `observed` proposal left without evidence becomes `inferred`; `relatesTo` to a record of the session sets `duplicate_of`/`conflicts_with` (`refines` sets nothing); a proposal with the same normalized text and kind as an existing record is grouped (not created) — FR-027.
- `RecordDto` defined in `contracts/ipc-commands.md`; fixtures `RecordDto/`, `RecordSetStatusDto/`, `SessionEvent/record-proposed.json`. Records reach the UI through `record_proposed` and command results only; a reconnect does not replay them until `session_get` returns them (US4).
- Remote analysis adapters (Anthropic, OpenAI-compatible) and the `RemoteGate` check arrive with US7; `AnalysisAdapterFactory` returns `Unavailable` for them meanwhile.
- T083: the harness runs as a self-test by default; ADR-003 keeps the reference-provider table pending (no Ollama on the implementation machine).

---

## Phase 6: User Story 4 - Revisar o plano de desenvolvimento e consultar sessões salvas (Priority: P1)

**Goal**: Consolidated plan per session, editable, persisted; session history list and detail including failed sessions.

**Independent Test**: End a session with records, review/edit plan, restart app, reopen session; everything intact.

### Tests for User Story 4 (REQUIRED) ⚠️

- [X] T098 [P] [US4] Plan derivation tests (grouping by kind, ordering by status/date, user-origin marking) in `crates/application/tests/plan_view.rs`
- [X] T099 [P] [US4] Tests for user edits/manual records with history and final analysis only on ended/failed sessions (FR-033) in `crates/application/tests/record_edits.rs`
- [X] T100 [P] [US4] Persistence round-trip test (create session with segments/records, reopen DB, compare) in `crates/persistence/tests/round_trip.rs`
- [X] T101 [P] [US4] Frontend tests for SessionHistory (list, open, failed badge) and PlanView editing via keyboard in `apps/desktop/src/features/sessions/SessionHistory.test.tsx` and `apps/desktop/src/features/records/PlanView.test.tsx`

### Implementation for User Story 4

- [X] T102 [US4] Implement plan view assembly (DevelopmentPlan) in `crates/application/src/plan.rs`
- [X] T103 [US4] Extend `record_service` with edit text/rationale/criteria/alternatives and manual creation (origin user) in `crates/application/src/record_service.rs`
- [X] T104 [US4] Implement final analysis trigger in `crates/application/src/analysis_service.rs`
- [X] T105 [US4] Implement commands `plan_get`, `record_edit`, `record_create`, `analysis_run_final`, `session_list` (cursor pagination), `session_get`, `session_rename` in `apps/desktop/src-tauri/src/commands/records.rs` and `apps/desktop/src-tauri/src/commands/sessions.rs`
- [X] T106 [P] [US4] Implement PlanView (sections per kind, status filters, inline edit, add item) in `apps/desktop/src/features/records/PlanView.tsx`
- [X] T107 [P] [US4] Implement SessionHistory and SessionDetail pages (title, date, active duration, state, failure reason) in `apps/desktop/src/features/sessions/SessionHistory.tsx` and `apps/desktop/src/features/sessions/SessionDetail.tsx`
- [X] T108 [US4] Add US4 strings to `apps/desktop/src/i18n/pt-BR.json` and `apps/desktop/src/i18n/en.json`

**Checkpoint**: Full P1 core loop: capture → transcript → proposals → plan review → history

**Phase 6 notes** (2026-10-28):
- Plan (T102): derived on demand by `DevelopmentPlan::assemble`; always six sections in the order task, requirement, decision, assumption, risk, open_question (empty ones included), items by status (`confirmed`, `pending`, `proposed`, `rejected`) then creation. Served by `RecordService::plan` / `plan_get`.
- Edits (T103): `TechnicalRecord::edit` writes one `RecordChange` per field that actually changed (lists stored in the history one item per line); status is untouched; an empty edit is `invalid_input`. User records (`record_create`) are `origin = user`, `nature = suggested`, any initial status, no evidence. `RecordService` now also takes the `SessionRepository` (unknown session → `not_found`).
- Final analysis (T104): `AnalysisService::run_final` — only `ended`/`failed`, one at a time per session (`invalid_state`), `provider_error` without a usable analysis provider; sends up to 400 most recent finals plus the record summary; counts against the hourly cap (a capped request is recorded and reported as `rate_limited`); the per-session token budget is not tracked across restarts, so it does not apply. Results arrive as session events; `SessionDetail` subscribes for ended/failed sessions.
- History (T105): `session_list` pages in the service (`cursor` = last id of the previous page, default 50, max 100); `session_get` returns `{ session, transcript, records }` (questions join with US5); `session_rename` also renames the active session in memory. New contract fixtures for every new DTO.
- The live session store now loads records through `session_get` after each (re)subscription, closing the Phase 5 gap where a reconnect lost earlier proposals.
- A new "Histórico" navigation entry hosts `SessionHistory` → `SessionDetail` (rename, failure reason, transcript, plan, final analysis).

---

## Phase 7: User Story 5 - Fazer perguntas sobre o contexto da sessão (Priority: P1)

**Goal**: Grounded answers with citations or explicit absence of evidence; no side effects on records.

**Independent Test**: Ask questions with and without answers in a reference transcript; verify citations, `no_evidence`, and unchanged records.

### Tests for User Story 5 (REQUIRED) ⚠️

- [X] T109 [P] [US5] Question service tests: invalid evidence ids dropped → `no_evidence`, answers never mutate records, conversion to proposed/suggested record, runs concurrently with capture without blocking, cited evidence shown in its original language (FR-053b), in `crates/application/tests/question_service.rs`
- [X] T110 [P] [US5] Injection cases inside questions and transcript for Q&A path in `crates/application/tests/prompt_injection.rs`
- [X] T111 [P] [US5] Frontend tests for QuestionsPanel (pending/answered/failed, certainty labels, citation links) in `apps/desktop/src/features/questions/QuestionsPanel.test.tsx`
- [X] T111a [P] [US5] Q&A evaluation on the T083 fixtures with answerable and unanswerable questions: 100% of answers cite evidence or declare `no_evidence`, and 0 invented facts on unanswerable questions (SC-011); same `DEVCOPILOT_EVAL_PROVIDER`-gated real-provider mode, in `crates/intelligence/tests/qa_evaluation.rs`

### Implementation for User Story 5

- [X] T112 [P] [US5] Implement ContextQuestion, EvidenceRef, Certainty in `crates/domain/src/question.rs` with unit tests
- [X] T113 [P] [US5] Implement QuestionRepository in `crates/persistence/src/repos/question_repo.rs`
- [X] T114 [US5] Implement answer prompt and output parsing (answer, certainty, evidence refs) in `crates/intelligence/src/prompt.rs` and `crates/intelligence/src/output_validation.rs`, and `answer` in `crates/intelligence/src/ollama.rs`
- [X] T115 [US5] Implement `question_service` in `crates/application/src/question_service.rs`
- [X] T116 [US5] Implement commands `question_ask`, `question_to_record` and `question_answered` event in `apps/desktop/src-tauri/src/commands/questions.rs`
- [X] T117 [P] [US5] Implement QuestionsPanel in `apps/desktop/src/features/questions/QuestionsPanel.tsx`
- [X] T118 [US5] Add US5 strings to `apps/desktop/src/i18n/pt-BR.json` and `apps/desktop/src/i18n/en.json`

**Phase 7 notes** (2026-10-28):
- Domain (T112): `ContextQuestion` goes `pending` → `answered` | `failed`. `Answer::grounded` drops evidence outside the session, repeats and anything beyond 20 references; a `no_evidence` answer cites nothing, and an answer left without evidence becomes `no_evidence` (SC-011). Answers are `AnswerText` (1–4 000 characters). `ContextQuestion::to_record` builds the FR-037 conversion: a new record with `origin = user`, `status = proposed`, `nature = suggested`, the question as rationale and the cited segments as evidence; answers longer than a record are cut with `…`.
- Provider output (T114): new contract `contracts/answer-output.schema.json` (`answer`, `certainty`, `evidenceSegmentIds`, `evidenceRecordIds`). The question travels inside `<session_data>` like the transcript, with fixed per-language instructions. Ollama `answer` sends the answer schema as `format`.
- Service (T115): `QuestionService::ask` stores the question as `pending` and answers it in a spawned task, so capture and transcription never wait (US5.4). Only one question per session is in flight (`invalid_state` otherwise); sessions that never started are `invalid_state`; without a usable analysis provider it is `provider_error`. The provider gets the session's 400 most recent finals plus the record summary. Questions are not counted against the analysis hourly cap. Questions left `pending` by an unexpected exit fail on startup.
- IPC (T116): `question_ask` returns the `pending` `QuestionDto` instead of `{ questionId }` (contract updated) so the UI shows it with the core's timestamp. `question_answered` carries failures too (`status = failed`). `session_get` now includes `questions`. New fixtures: `QuestionDto`, `QuestionAskDto`, `QuestionToRecordDto` and `SessionEvent/question-answered`.
- T110 and T111a live in `crates/intelligence/tests/` (`prompt_injection.rs`, with 16 Q&A cases in `fixtures/injection/qa_cases.json`; `qa_evaluation.rs`), not `application/tests`, for the same reason as T081: attacks must go through the real validation. T111a extends the T083 fixtures with 13 annotated questions (7 answerable, 6 unanswerable with forbidden terms). The real-provider run is gated by `DEVCOPILOT_EVAL_PROVIDER`, like T083; its metrics are still to be recorded in ADR-003.
- UI (T117): `QuestionsPanel` appears on the live session page (any state after `idle`) and in `SessionDetail` (ended/failed). Certainty is shown in words; cited segments are quoted as stored, in their original language (FR-053b), and link to the transcript; cited records show kind and text. Conversion to a proposed item asks for the kind. `showSegment` moved to `features/transcript/showSegment.ts`.

---

## Phase 8: User Story 6 - Exportar e excluir sessões (Priority: P1)

**Goal**: Markdown + JSON export to a user-chosen path with unencrypted warning; deletion with warnings and complete removal.

**Independent Test**: Export a session and validate files; delete it and confirm it is absent from lists, questions, analysis and exports.

### Tests for User Story 6 (REQUIRED) ⚠️

- [X] T119 [P] [US6] Export tests: JSON matches `contracts/export-format.md` v1 (fixture comparison), Markdown snapshot, content selection, no providerId/credentials/paths, export during an in-flight analysis contains only persisted content, in `crates/application/tests/export_service.rs`
- [X] T120 [P] [US6] Deletion tests: cascade removes all rows, deleted content absent from list/get/questions/analysis/export (SC-012), active session deletion rejected (FR-044), in-flight analysis/question runs cancelled and their results discarded, WAL checkpoint executed, in `crates/persistence/tests/delete_session.rs` and `crates/application/tests/delete_session.rs`
- [X] T121 [P] [US6] Command test: export writes only to the dialog-returned path and cancellation writes nothing, in `apps/desktop/src-tauri/tests/export_command.rs`
- [X] T122 [P] [US6] Frontend tests for ExportDialog warning and DeleteSessionDialog confirmation in `apps/desktop/src/features/sessions/ExportDialog.test.tsx` and `apps/desktop/src/features/sessions/DeleteSessionDialog.test.tsx`

### Implementation for User Story 6

- [X] T123 [US6] Implement `export_service` (Markdown with UI-language labels, JSON v1, ExportRecord) in `crates/application/src/export_service.rs`
- [X] T124 [P] [US6] Implement export record repository and `delete_cascade` with checkpoint in `crates/persistence/src/repos/export_repo.rs` and `crates/persistence/src/repos/session_repo.rs`
- [X] T125 [US6] Implement session deletion use case in `crates/application/src/session_service.rs`
- [X] T126 [US6] Implement `export_session` (native save dialog in Rust, write only to returned path) and `session_delete` commands in `apps/desktop/src-tauri/src/commands/export.rs` and `apps/desktop/src-tauri/src/commands/sessions.rs`
- [X] T127 [P] [US6] Implement ExportDialog and DeleteSessionDialog (secure-deletion, backup and exported-file warnings) in `apps/desktop/src/features/sessions/ExportDialog.tsx` and `apps/desktop/src/features/sessions/DeleteSessionDialog.tsx`
- [X] T128 [US6] Add US6 strings to `apps/desktop/src/i18n/pt-BR.json` and `apps/desktop/src/i18n/en.json`

**Phase 8 notes** (2026-10-28):
- Domain: `export.rs` adds `ExportFormat` (`markdown`/`json`), `ExportContent` (`plan`/`transcript`) and `ExportRecord` (session, formats, content, instant — never the path).
- Export (T123): `ExportService::file_names` → save dialog per format → `render` (from what is persisted at that moment) → files written by the command → `record`. JSON is `devcopilot.session-export` v1 built with `serde_json` (now a dependency of `application`; it is pure data formatting, no I/O). Markdown labels follow the UI language; session text is escaped so it cannot add structure, links or HTML. Choices recorded in `contracts/export-format.md`: plan sections follow the UI plan order, each item carries an origin marker (`[IA]`/`[usuário]`) besides status and nature (US6.1 "proveniência"), only answered questions are exported, provider ids appear nowhere. Example outputs live in `contracts/examples/` and are the T119 fixtures (`DEVCOPILOT_UPDATE_SNAPSHOTS=1` rewrites them).
- Deletion (T124/T125): `SessionRepository::delete` deletes the `sessions` row (everything else cascades) and runs `PRAGMA wal_checkpoint(TRUNCATE)`; a busy checkpoint is reported as `storage_error` instead of a complete deletion. `SessionService::delete` is async: it refuses active sessions (including `starting`/`stopping`, FR-044) and sessions already being deleted (which also cannot start meanwhile), then cancels and awaits — up to `DRAIN_DEADLINE` — the session's final analysis, an incremental run still draining after stop, and its question in flight. This uses a new per-session `InFlight` registry inside `application` (replacing the previous "one in flight" sets); a question cancelled this way is discarded, neither stored nor published. `SessionDeps` gained `questions: Option<QuestionService>`.
- IPC (T126): `export_session` takes the dialog through a `SaveDialog` trait (`NativeSaveDialog` opens `tauri-plugin-dialog` from Rust), asks for every path first and writes nothing if any dialog is cancelled; the same path for two formats is `invalid_input` (`path`); write failures are `storage_error` without the path. Because the dialog is opened by the core, `capabilities/default.json` drops `dialog:allow-save`: the webview has `core:default` only. `session_delete` requires `confirmed: true` and returns `{}`. New fixtures: `ExportSessionDto`, `ExportResultDto`, `SessionDeleteDto`.
- UI (T127): `SessionDetail` has Export and Delete actions. `ExportDialog` shows the unencrypted-file warning (also the dialog's description), formats and content, and reports the files written or that nothing was written. `DeleteSessionDialog` is an `alertdialog` listing the secure-deletion, backup, exported-file and irreversibility warnings, starts focused on Cancel, and for an active session only asks to stop it first; after deletion the view returns to the history.
- Also fixed a `tsc` error from Phase 7 in `SessionsPage.test.tsx` (`questionAsk` mock inferred as `Promise<never>`).

---

## Phase 9: User Story 7 - Autorizar explicitamente um provedor remoto (Priority: P1)

**Goal**: Remote transcription/analysis only after informed authorization; revocation, no unauthorized fallback, credentials in OS vault.

**Independent Test**: Configure a remote provider pointing at a fake server; deny → zero requests; authorize → only declared categories; revoke → requests stop; failures do not fall back to other remote providers.

### Tests for User Story 7 (REQUIRED) ⚠️

- [X] T129 [P] [US7] RemoteGate tests with `wiremock`: zero requests without authorization, endpoint/category change invalidates authorization, revocation stops new requests, https required (SC-013), in `crates/application/tests/remote_gate.rs`
- [X] T130 [P] [US7] Fallback tests: remote failure never switches to another remote provider; local fallback allowed with notice (FR-045b), in `crates/application/tests/provider_fallback.rs`
- [X] T131 [P] [US7] Adapter contract tests against `wiremock` for OpenAI-compatible STT, Anthropic and OpenAI-compatible chat (request shape, timeouts, malformed responses → errors) in `crates/intelligence/tests/remote_adapters.rs`
- [X] T132 [P] [US7] Credential tests: secrets stored only in SecretStore, absent from DB, logs and export (SC-014) in `crates/persistence/tests/credentials.rs`
- [X] T133 [P] [US7] Frontend tests for AuthorizationDialog (destination, categories, retention text; accept/decline) in `apps/desktop/src/features/providers/AuthorizationDialog.test.tsx`

### Implementation for User Story 7

- [X] T134 [P] [US7] Implement ProviderAuthorization and coverage rule in `crates/domain/src/provider.rs`
- [X] T135 [P] [US7] Add authorization persistence to `crates/persistence/src/repos/provider_repo.rs`
- [X] T136 [US7] Implement authorize/revoke/preview and `RemoteGate` in `crates/application/src/provider_service.rs`, plus local fallback with notice in `crates/application/src/capture_pipeline.rs`
- [X] T137 [P] [US7] Implement OpenAI-compatible STT adapter (finals only, chunk upload, gate check per request) in `crates/intelligence/src/openai_stt.rs`
- [X] T138 [P] [US7] Implement Anthropic adapter (analyze + answer, configurable model, gate check) in `crates/intelligence/src/anthropic.rs`
- [X] T139 [P] [US7] Implement OpenAI-compatible chat adapter in `crates/intelligence/src/openai_chat.rs`
- [X] T140 [US7] Implement commands `provider_upsert` (remote), `provider_set_secret`, `provider_authorization_preview`, `provider_authorize`, `provider_revoke` in `apps/desktop/src-tauri/src/commands/providers.rs`
- [X] T141 [P] [US7] Implement ProvidersPage, ProviderForm and AuthorizationDialog in `apps/desktop/src/features/providers/ProvidersPage.tsx`, `apps/desktop/src/features/providers/ProviderForm.tsx`, `apps/desktop/src/features/providers/AuthorizationDialog.tsx`
- [X] T142 [US7] Add US7 strings to `apps/desktop/src/i18n/pt-BR.json` and `apps/desktop/src/i18n/en.json`


**Phase 9 notes** (2026-10-28):
- Domain (T134): `ProviderAuthorization` (`granted`/`revoked`) snapshots destination (host and explicit port), categories and the retention text shown; `covers(provider, categories)` holds only while granted, for the same provider, destination and categories, and for categories the provider declares. `AuthorizationStatus` (`not_required`, `configured`, `authorized`, `revoked`) is the spec's provider state; `Provider::changes_destination_or_data` decides when an edit revokes.
- Persistence (T135): `ProviderRepository` gained `latest_authorization` and `save_authorization`; granting adds a `provider_authorizations` row, revoking updates the latest one, so the history stays in storage. No migration was needed (the table was in v1).
- Service (T136): `ProviderService::new` now also takes the secret store and clock. `upsert` handles local and remote providers (`upsert_local` stays as a guard); an existing provider keeps its credential reference, and changing adapter, endpoint or categories revokes its authorization. `set_secret` writes only to the OS credential store. The retention notice is a fixed text in the UI language: Dev Copilot cannot know a third party's actual retention, so it says the destination's own terms apply and revocation does not delete what was already sent. Resolution: the default when usable, otherwise local providers only; another remote is picked only when no default is set; with only unauthorized remotes the error is `authorization_required` naming the provider (`ApplicationError::AuthorizationRequired { provider }`, sent as `details.providerId`). `ProviderService::gate()` is the `RemoteGate`: it reads storage on every check, so revocation applies to the next request, and any storage failure blocks the request.
- Fallback: `FallbackTranscriber` (in `capture_pipeline.rs`) wraps a remote transcriber when a local one can transcribe; the first remote failure, including a revoked authorization, records the gap as usual and moves the session to the local provider for good, with a new `provider_fallback` event followed by `provider_in_use`. Without a local provider, the session fails as `no_transcriber` after the usual three failures. FR-045b is a MAY: analysis and questions do not fall back; a failed run or question is reported as `provider_error`.
- Adapters (T137–T139): `remote.rs` holds what they share: `RemoteTarget` (https only, TLS verified, no redirects, key read from the credential store and sent as a sensitive header), `strip_analysis`/`strip_question`, which remove categories the provider was not configured for before the gate is asked for exactly what remains, and `RemoteAnalysisProvider` over a `ChatBackend`. Anthropic uses `POST {endpoint}/v1/messages` with `anthropic-version: 2026-10-21`, no `temperature` (that version rejects it) and no API-side JSON schema (the contract schema uses length limits the API does not support); local validation stays the only gate, `refusal` is a provider error and `max_tokens` is invalid output. OpenAI-compatible chat uses `response_format: json_object` and sends `Authorization` only when a key is stored. STT uploads each final window (pause 1.2 s, at most 25 s) as a 16-bit WAV in a hand-built multipart body (no new reqwest feature), finals only. `factory.rs` now holds `TranscriberAdapterFactory` and `AnalysisAdapterFactory` (moved out of `ollama.rs`).
- Composition: the factories are built before storage, so they hold a `DeferredGate` that refuses everything until `compose_core` connects it to `ProviderService::gate()`, and keeps refusing if storage never opens.
- Tests: T129 is split. `crates/application/tests/remote_gate.rs` covers the gate rules over storage, and `crates/intelligence/tests/remote_gate.rs` covers zero requests to a `wiremock` server through the real adapters before authorizing, after revoking and after reconfiguring, plus a production target pointed at plain HTTP (TLS fails, the server parses nothing). The second file lives in `intelligence` for the same reason as T081/T110. T132 captures `tracing` output with `tracing-subscriber` (new dev-dependency of `persistence`) and scans every text column of the database.
- IPC (T140): new commands `provider_set_secret`, `provider_authorization_preview`, `provider_authorize`, `provider_revoke`; `provider_upsert` accepts remote providers. `ProviderDto` gained `authorization` and `hasCredential`. New fixtures: `ProviderSetSecretDto`, `ProviderIdDto`, `ProviderAuthorizeDto`, `AuthorizationPreviewDto`, `ProviderAuthorizationDto`, `SessionEvent/provider-fallback`, `IpcError/authorization-required`, and remote `ProviderDto`/`ProviderUpsertDto` examples.
- UI (T141): `ProvidersPage` replaces the bare section on the Providers screen. It lists providers with role, locality, endpoint, authorization status and whether a key is stored, and offers make default, edit, authorize and revoke, plus `ProviderForm` and the local models list. The API key field is write-only. `AuthorizationDialog` shows destination, categories and retention, starts focused on Decline, and authorizes only on Accept. The sessions page opens it when `session_start` returns `authorization_required`, then starts again after acceptance, and announces `provider_fallback` in a status message.

---

## Phase 10: User Story 9 - Pausar e retomar a captura (Priority: P1)

**Goal**: Pause stops capture and processing; resume re-checks permissions/sources; paused interval recorded as gap.

**Independent Test**: Pause a capturing session; verify no audio captured/processed until resume; gap `paused` in transcript; stop from paused works.

### Tests for User Story 9 (REQUIRED) ⚠️

- [X] T143 [P] [US9] Use-case tests: pause closes streams and halts transcription, resume re-checks permission/source/transcriber, stop from paused, active_duration excludes pauses, gap `paused` recorded, in `crates/application/tests/pause_resume.rs`
- [X] T144 [P] [US9] Frontend test: pause/resume controls and indicator "pausada" announced in `apps/desktop/src/features/sessions/SessionControls.test.tsx`

### Implementation for User Story 9

- [X] T145 [US9] Implement pause/resume in `crates/application/src/session_service.rs` and pause gating in `crates/application/src/capture_pipeline.rs`
- [X] T146 [US9] Implement commands `session_pause`, `session_resume` in `apps/desktop/src-tauri/src/commands/sessions.rs`
- [X] T147 [US9] Add pause/resume to `apps/desktop/src/features/sessions/SessionControls.tsx` and paused state to `apps/desktop/src/features/sessions/CaptureIndicator.tsx`, with strings in `apps/desktop/src/i18n/pt-BR.json` and `apps/desktop/src/i18n/en.json`


**Phase 10 notes** (2026-10-28):
- Domain: `Session::pause`/`resume` already existed (T046). Only the service, the commands and the UI were missing.
- Service (T145): `pause` closes every source under the session lock before returning (nothing more can be captured), publishes `capture_indicator { paused: true }` with the session's sources, and lets the transcription of audio captured before the pause drain in the background (bounded by `DRAIN_DEADLINE`). `resume` re-checks the credential store, the transcriber (`no_transcriber`, `authorization_required`) and each source's availability and permission. A source that fails is reported by `source_status` and stays closed; with none left, the session stays `paused` and the error is `permission_denied` or `source_unavailable`. Then it returns `capturing` while a background task waits for the pre-pause drain, stores `gap(paused)` and reopens the sources, so segments stay in `seq` order. Stop from `paused` waits for that drain and never reopens anything. Incremental analysis stays attached across the pause and only receives finals.
- Offsets: segment offsets are now wall-clock time since the session first captured (data-model: "relativos ao início da sessão") instead of the accumulated active duration, so `gap(paused)` has the length of the pause. The gap starts at the later of the pause instant and the end of the audio already captured. `active_duration` still excludes pauses (FR-040).
- Each capture runs under its own `capture` token, a child of the session token, which pause and stop cancel, so a closed pipeline's fault supervisor never acts on a later one. "Pause gating" (T145) is this closing of the pipeline; `capture_pipeline.rs` itself needed no change. `Transcription::captured_end` was added for the gap start.
- Pausing while a resume is still reopening sources is refused with `invalid_state`.
- IPC (T146): `session_pause` and `session_resume` call the service directly. The contract table now lists the background reopen, `source_status` for skipped sources, and the `authorization_required`/`secret_store_unavailable` errors.
- UI (T147): `SessionControls` offers Pause while capturing and Resume while paused, with Stop in both, and shows a paused hint. `CaptureIndicator` already rendered the paused state ("Captura pausada", ❚❚); the paused gap uses the existing transcript gap label.

---

## Phase 11: User Story 8 - Capturar áudio do sistema quando a plataforma permitir (Priority: P2)

**Goal**: System audio source per OS only where validated; clear unavailability reason elsewhere; dual-source tagging.

**Independent Test**: On each OS in the matrix, the option appears only if validated; permission requested; dual-source segments tagged; system source failure keeps mic running.

### Tests for User Story 8 (REQUIRED) ⚠️

- [ ] T148 [P] [US8] Pipeline tests with two scripted sources: per-source tagging, one source failing keeps the other (scenario 3), in `crates/application/tests/dual_source.rs`
- [ ] T149 [P] [US8] Availability tests: unvalidated platforms report `not_validated_on_platform` in `crates/audio/tests/availability.rs`

### Implementation for User Story 8

- [ ] T150 [US8] Spike S1 on real hardware for macOS (Core Audio taps vs ScreenCaptureKit), Windows (WASAPI loopback) and Linux (monitor source via libpulse vs PipeWire); record results in `docs/adr/ADR-002.md` (captura de áudio)
- [ ] T151 [P] [US8] Implement Windows loopback adapter (cfg windows) in `crates/audio/src/system/windows.rs` if S1 approves
- [ ] T152 [P] [US8] Implement Linux monitor-source adapter (cfg linux) in `crates/audio/src/system/linux.rs` if S1 approves
- [ ] T153 [P] [US8] Implement macOS system audio adapter (cfg macos) with isolated `unsafe` FFI, `// SAFETY:` comments and focused tests in `crates/audio/src/system/macos.rs` if S1 approves
- [ ] T154 [US8] Implement per-source transcription streams and source tagging in `crates/application/src/capture_pipeline.rs`
- [ ] T155 [US8] Register system sources with availability per platform in `apps/desktop/src-tauri/src/composition.rs` and update `apps/desktop/src-tauri/Info.plist`/entitlements only as required by the S1 decision
- [ ] T156 [US8] Show system source availability/reasons and dual selection in `apps/desktop/src/features/sessions/NewSessionForm.tsx` with strings in `apps/desktop/src/i18n/pt-BR.json` and `apps/desktop/src/i18n/en.json`

---

## Phase 12: Polish & Cross-Cutting Concerns

**Purpose**: Security acceptance, platform verification, docs

- [ ] T157 [P] Log redaction test running a scripted session and scanning log files for known phrases and secrets (SC-014) in `apps/desktop/src-tauri/tests/log_redaction.rs`
- [ ] T158 [P] Storage scan test searching DB/WAL files for known phrases (SC-018), and asserting no audio files or raw PCM are written to the app data, cache or temp dirs during a scripted session incl. remote STT chunk upload (FR-020), in `crates/persistence/tests/at_rest_scan.rs`
- [ ] T159 [P] Soak test: 2 h simulated session with scripted audio and fake providers asserting bounded queues and RSS at most 10% above the 10-minute warm-up value (SC-015), ignored by default, in `crates/application/tests/soak.rs`
- [ ] T160 [P] Accessibility pass: axe checks on all pages and keyboard-only flow test (SC-016) in `apps/desktop/src/a11y.test.tsx`
- [ ] T161 Security review applying, by relevance, OWASP Top 10, OWASP ASVS (IPC/trust-boundary controls), OWASP Top 10 for LLM Applications, CWE Top 25 and NIST SSDF (Art. IV.9); verify the plan.md threat model table, `apps/desktop/src-tauri/capabilities/default.json` and CSP remain least-privilege; record findings per catalog with disposition and the explicit merge approval (Art. IV.10) in `specs/001-meeting-sessions/security-review.md`
- [ ] T162 [P] Create platform acceptance matrix following quickstart §4 in `specs/001-meeting-sessions/acceptance-matrix.md` and execute it on macOS, Windows and Linux, including: timed new-user create+start (SC-001) and find-previous-session (SC-017) runs, screen-reader announcement of capture state with VoiceOver, Narrator/NVDA and Orca (SC-016), default-device change (Bluetooth connect/disconnect, FR-011) and suspend/lock during capture
- [ ] T163 [P] Document local data format, migration procedure (`PRAGMA user_version`, forward-only upgrades, behavior on newer-than-app schema, backup before migrate), data classification, retention/deletion limitations and OS permissions in `docs/local-data.md` and `docs/permissions.md`
- [ ] T164 [P] Update `README.md` and `apps/desktop/README.md` status sections to reflect implemented (and only implemented) capabilities (Art. IX.4)
- [ ] T165 Run all CI gates locally (fmt, clippy, test, audit, deny, gitleaks, frontend check/test) and fix findings
- [ ] T166 Run `specs/001-meeting-sessions/quickstart.md` end-to-end validation

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: no dependencies
- **Foundational (Phase 2)**: depends on Setup; T011 (spike S3) gates T023–T028 — BLOCKS all stories
- **US1 (Phase 3)**: after Foundational
- **US2 (Phase 4)**: after US1 (extends `capture_pipeline`; FR-045a needs a transcriber for manual runs)
- **US3 (Phase 5)**: after US2 (needs final segments)
- **US4 (Phase 6)**: after US3 (plan built from records)
- **US5 (Phase 7)**: after US3 (reuses the analysis adapter, prompt builder and output validation from T089–T091, and the record domain from T086 for `question_to_record`)
- **US6 (Phase 8)**: after US2 (transcript export); plan export complete after US3
- **US7 (Phase 9)**: after US3 (remote analysis) — remote STT part only needs US2
- **US9 (Phase 10)**: after US2
- **US8 (Phase 11, P2)**: after US2; implementation tasks conditional on spike S1
- **Polish (Phase 12)**: after all desired stories

### Story completion graph

```text
Setup → Foundational → US1 → US2 ─┬─▶ US3 ─┬─▶ US4
                                  │        ├─▶ US5
                                  │        └─▶ US7
                                  ├─▶ US6 (plan export after US3)
                                  ├─▶ US9
                                  └─▶ US8 (P2, spike-gated)
```

### Within Each User Story

- Tests first (must fail), then domain → repositories → services → commands → UI → strings
- Tasks touching the same file (`capture_pipeline.rs`, `session_service.rs`, i18n JSON, `commands/sessions.rs`) are sequential

### Parallel Opportunities

- Setup: T002–T007, T009, T010
- Foundational: T012–T018 (domain primitives), T020/T021, T026–T028, T032/T033, T035–T037, T039
- After US2: US6, US9 and US8 can proceed in parallel; after US3: US4, US5 and US7 in parallel. Stories touching `session_service.rs`, `capture_pipeline.rs` or `prompt_injection.rs` (US6/US7/US8/US9, US3/US5) must serialize those edits
- All test tasks marked [P] within a story

---

## Parallel Example: User Story 3

```bash
# Tests together:
Task: "T079 Domain record tests in crates/domain/src/record.rs"
Task: "T080 Output validation tests in crates/intelligence/tests/output_validation.rs"
Task: "T081 Prompt-injection suite in crates/application/tests/prompt_injection.rs"
Task: "T083 Evaluation harness in crates/intelligence/tests/evaluation.rs"

# Independent implementation files:
Task: "T086 TechnicalRecord in crates/domain/src/record.rs"
Task: "T089 Prompt builder in crates/intelligence/src/prompt.rs"
Task: "T090 Output validation in crates/intelligence/src/output_validation.rs"
```

---

## Implementation Strategy

### MVP First

1. Phase 1 + Phase 2 (spike S3 must pass)
2. US1 + US2 → first demo: authorized capture with live transcript (local whisper)
3. US3 + US4 → core value: structured proposals and plan review
4. **STOP and VALIDATE** against quickstart §4 steps 1–5 on all three OSes

**Conditional path**: if spike S2 (T067) rejects local transcription on an OS, or no local analysis provider (Ollama) is available on the reference setup, US7 moves ahead of US3 for that OS (remote STT tasks T137/T140 after US2; remote analysis T138/T139 with US3), since FR-045 requires at least one working path per OS.

### Incremental Delivery

1. + US5 (questions), US6 (export/delete), US9 (pause) → complete P1 local-only MVP
2. + US7 → remote providers under authorization (required for OSes where S2 fails)
3. + US8 → system audio where S1 approved
4. Polish → security review, acceptance matrix, docs

### Feature Definition of Done

All P1 stories done, Polish complete, CI green on the three OSes, acceptance matrix signed off, ADR-002/003/004 accepted.

Distribution (packaging, code signing/notarization, verified updates, traceable build artifacts — Art. IV.15) is **out of scope for 001** and MUST be delivered by a dedicated release feature before any external distribution.

---

## Notes

- [P] tasks = different files, no dependencies
- Commit after each task or logical group
- Never use real confidential meeting data in fixtures or manual tests
- Spike outcomes may remove tasks (S1) or move a story's default path (S2); update this file accordingly
