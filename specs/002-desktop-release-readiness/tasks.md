---

description: "Task list for 002-desktop-release-readiness"
---

# Tasks: Prontidão para o primeiro release desktop

**Input**: Design documents from `/specs/002-desktop-release-readiness/`
**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/ (ipc-updates, csp-policy, release-artifacts, release-evidence), quickstart.md

**Tests**: REQUIRED (Constitution Art. VIII). Every code change has unit, contract or configuration tests.
Platform compatibility is proven only by the manual tasks on real hardware; CI never counts as platform
evidence (spec FR-014).

**Organization**: one phase per user story. Phases are ordered by **build dependency**, not strictly by
priority. Everything that ships inside the binary (US2 CSP, US8 updater, the metrics mode) has to be merged
before the release candidate is built in US1, and all real-hardware validation (US1 install, US3, US4, US5,
US8 manual cases) runs on that same signed candidate. Priority labels are kept in each phase header.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: can run in parallel (different files, no dependency on incomplete tasks)
- **[Story]**: user story from spec.md (US1…US8)
- **Manual** tasks are done by a person on real hardware or on accounts and services. Their output is a
  versioned document under `docs/release/`.

## Path Conventions

- Rust core: `crates/application/src/`, tests in `crates/application/tests/`
- Composition root / IPC: `apps/desktop/src-tauri/src/`, config tests in `apps/desktop/src-tauri/tests/`
- Frontend: `apps/desktop/src/`
- Release tooling: `tooling/`, `.github/workflows/`
- Release docs and evidence: `docs/release/`, `docs/release/v0.1.0/`
- ADRs: `docs/adr/`

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: dependencies, workspace members, lint rules and the release documentation skeleton.

- [X] T001 Create `docs/release/README.md`. It indexes the permanent release docs (checklist, rollback,
  signing-and-distribution, verify-downloads) and the per-release folder `docs/release/v0.1.0/` (with
  `evidence/.gitkeep`). It also defines the GitHub issue labels `release-0.1.0`, `severity:critical|high|medium|low`,
  `platform:macos|windows|linux-wayland|linux-x11` and `release-blocker`, using the severity scale from
  spec.md Assumptions.
- [X] T002 [P] Add `tauri-plugin-updater` 2.x to the workspace `Cargo.toml` and to
  `apps/desktop/src-tauri/Cargo.toml`. Run `cargo tree -p devcopilot-desktop -i openssl-sys` and
  `cargo deny check` to confirm the plugin uses verified TLS and that its TLS stack is consistent with the
  existing `reqwest` (rustls). Enable the plugin's `rustls-tls` feature if needed. Record the finding under
  "Verificar" in `specs/002-desktop-release-readiness/research.md` R5.
- [X] T003 [P] Create the binary crate `tooling/perf-sampler/Cargo.toml` (`publish = false`, dependencies
  `sysinfo`, `clap`, `csv`, `anyhow`) with an empty `src/main.rs`, and add it to `[workspace].members` in the
  root `Cargo.toml`. It must not be a dependency of any app crate.
- [X] T004 [P] Add the rules `react/forbid-dom-props` (forbid `style`) and `react/no-danger` to
  `apps/desktop/eslint.config.js`, adding `eslint-plugin-react` to `apps/desktop/package.json` if it is
  missing. `pnpm --filter appsdesktop lint` must stay green, because the current code has no inline styles.
  *Done with the core rule `no-restricted-syntax` (selectors for the `style` and `dangerouslySetInnerHTML` JSX
  attributes) instead of `eslint-plugin-react`. That gives the same protection without a new dependency
  (Art. VII.6) and avoids the plugin's uncertain support for ESLint 10.*

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: signing identities and keys (the critical path with external lead time), the evidence
templates, and measurement hooks that must already be in the release candidate.

**⚠️ CRITICAL**: T005–T009 start on day one. US1 and US8 cannot finish without them. Phases 3 and 4 can run
in parallel with them.

- [ ] T005 Manual: enroll in the Apple Developer Program and issue a *Developer ID Application* certificate
  and an App Store Connect API key for notarization. Check eligibility for Azure Trusted Signing. If not
  eligible, buy an OV code-signing certificate with cloud HSM signing usable from CI (research R3). Record the
  chosen Windows provider, the certificate subject and the expiry dates in `docs/adr/ADR-005.md`.
- [ ] T006 Manual: generate the minisign updater key pair with `pnpm --filter appsdesktop tauri signer generate`.
  Store the private key and its password as secrets of the GitHub environment `release`, plus an encrypted
  offline backup held by the release owner. Record the public key fingerprint in `docs/adr/ADR-005.md`.
- [ ] T007 Manual: generate the OpenPGP release key, kept on the release owner's machine or token and never
  in CI. Commit the public key as `docs/release/release-key.asc` and publish its fingerprint in `README.md`
  and `docs/adr/ADR-005.md`.
- [ ] T008 Manual: create the GitHub environment `release`:
  - Required reviewer: the release owner.
  - Deployment limited to tags `v*.*.*`.
  - Secrets per the key table in `specs/002-desktop-release-readiness/contracts/release-artifacts.md`.
  Document the setup (names only, never values) in `docs/release/signing-and-distribution.md`, section
  "Segredos".
- [X] T009 Write `docs/adr/ADR-005.md` (status *Proposto*): distribution, signing and updates. It records the
  research decisions R1–R6 (CSP, macOS, Windows, Linux, updater, pipeline), the alternatives, the
  macOS Keychain/identity finding, the update rollback limits (no downgrade; key rotation), the fact that
  builds are not bit-for-bit reproducible (so the final artifacts get a smoke test instead of inheriting all rc
  evidence, T095), the justification for placing the measurement tool `perf-sampler` under `tooling/`
  (Art. VII.3: development tooling, never shipped), and the review triggers. Include a fenced line
  `updater-pubkey-id: <key id>` once T006 is done (read by T039a).
  *Written with status Proposto. The identity/key table holds placeholders, and the `updater-pubkey-id` line
  is left as a commented instruction, until the manual tasks T005–T007 record real values.*
- [X] T010 [P] Write `apps/desktop/src-tauri/tests/version_consistency.rs`. It asserts that the `version` in
  `apps/desktop/src-tauri/tauri.conf.json`, in `apps/desktop/package.json` and in
  `apps/desktop/src-tauri/Cargo.toml` are equal, and, when `RELEASE_TAG` is set, that it equals `v<version>`
  or `v<version>-rc.N`.
- [X] T011 [P] Create the evidence templates from
  `specs/002-desktop-release-readiness/contracts/release-evidence.md`:
  - `docs/release/checklist.md` (12 items).
  - `docs/release/v0.1.0/candidate.md`.
  - `docs/release/v0.1.0/measurement-plan.md`.
  - `docs/release/v0.1.0/measurements.md`.
  - `docs/release/v0.1.0/evidence/_template.md` (sections Ambiente, I1–I4, C1–C2, matrix cases 1–13/3b/3c/11b,
    U1–U4, Limitações, Problemas).
  - `docs/release/v0.1.0/decision.md`.
- [X] T012 Add the port `MetricsSink` (`fn record(&self, timing: SegmentTiming)`) and the value
  `SegmentTiming { kind: Partial|Final, speech_start_ms, speech_end_ms, emitted_ms, provider: ProviderId }`
  (`ProviderId` from `devcopilot_domain::ids`; the application layer has no model identifier, so the model in use
  is recorded in the measurement plan, not per row)
  to `crates/application/src/ports.rs`. Do this with no text field at all, so content cannot leak by design.
  Emit one timing per partial and final from `crates/application/src/transcription.rs` through an optional
  `Option<Arc<dyn MetricsSink>>` (no-op when absent).
- [X] T013 [P] Unit tests in `crates/application/tests/transcription_metrics.rs`, using a scripted transcriber
  and a recording sink:
  - One timing per partial and final, in order.
  - `speech_end_ms ≥ speech_start_ms`.
  - No sink → no work.
  - Timings stop after stop/cancel.
- [X] T014 Implement `CsvMetricsSink` in `apps/desktop/src-tauri/src/metrics.rs`. It writes
  `metrics-<UTC date>.csv` to the app log directory, with columns
  `kind,speech_start_ms,speech_end_ms,emitted_ms,provider` and a bounded, non-blocking writer. Wire it in
  `apps/desktop/src-tauri/src/composition.rs` only when the environment variable `DEVCOPILOT_METRICS=1` is
  set; otherwise pass `None`.
  Retention: on startup, delete `metrics-*.csv` files older than 7 days. When `DEVCOPILOT_METRICS` is unset,
  create no file and leave existing ones to the same 7-day cleanup. Unit-test the cleanup with a temp dir in
  `apps/desktop/src-tauri/src/metrics.rs` (`#[cfg(test)]`) (Art. IV.7, IV.12).
- [X] T015 Extend `apps/desktop/src-tauri/tests/log_redaction.rs`: run the scripted session with
  `DEVCOPILOT_METRICS=1` and assert that the metrics CSV contains none of the synthetic phrases or secrets and
  only the five declared columns (SC-014 of 001).
  *The test turns the mode on through `metrics::sink_for(Some("1"), …)`, the same path `DEVCOPILOT_METRICS`
  takes, so it does not mutate the process environment. A partial with a synthetic phrase was added so
  the CSV check also covers partials.*
- [X] T016 [P] Implement `tooling/perf-sampler/src/main.rs`, with unit tests in the same crate for argument
  parsing, the CSV writer, and the percentile/summary math:
  - `sample --process <name>|--pid <pid> --interval <s> --out <csv>` writes the columns
    `utc,cpu_percent_of_total,rss_bytes`. CPU is normalized to total capacity, so 100 % means all cores.
  - `summarize --metrics <csv>` prints p50/p95/max of first-partial latency (`emitted − speech_start`) and of
    final latency (`emitted − speech_end`), plus the count.
  - `summarize --samples <csv> --warmup-min 10` prints mean CPU and RSS growth versus the 10-minute value.

**Checkpoint**: keys and identities in progress or done; evidence templates exist; metrics mode and
perf-sampler ready to ship in the candidate.

---

## Phase 3: User Story 2 — Interface funcionando sob política de conteúdo estrita (Priority: P1)

**Goal**: production CSP without `'unsafe-inline'`, the UI unchanged, and violations visible in the signed
build (closes F-1 in code).

**Independent Test**: production build on each OS, all screens and dialogs walked through, zero
`csp_violation` lines in the log, screenshots match the baseline (quickstart §2).

### Tests for User Story 2 (REQUIRED) ⚠️

- [X] T017 [P] [US2] Write `apps/desktop/src-tauri/tests/csp_policy.rs`. It parses `app.security.csp` from
  `tauri.conf.json` and enforces invariants 1–4 of
  `specs/002-desktop-release-readiness/contracts/csp-policy.md`: no unsafe keywords or wildcards, the exact
  `connect-src` allowlist, `object-src 'none'`/`base-uri 'self'`/`form-action 'none'` present, and
  `'sha256-…'` in `style-src` only if it is in an in-test exception list (initially empty) that cites an ADR.
  It must fail on the current config.
- [X] T018 [P] [US2] Write `apps/desktop/scripts/check-dist.mjs`. It fails if any HTML file in
  `apps/desktop/dist/` contains a `<style` element or a `style=` attribute. Add the script
  `"test:dist": "node scripts/check-dist.mjs"` to `apps/desktop/package.json`, and a step
  `pnpm --filter appsdesktop build && pnpm --filter appsdesktop test:dist` to the frontend job in
  `.github/workflows/ci.yml`.
- [X] T018a [P] [US2] Add a `test:dist` task to `turbo.json` with `dependsOn: ["build"]`,
  `inputs: ["dist/**", "scripts/check-dist.mjs"]` and no outputs. Keep the release and perf tasks out of
  `turbo.json`, because they must not be cached (Art. VII.5).
- [X] T019 [P] [US2] Write frontend tests in `apps/desktop/src/diagnostics/cspReporter.test.ts`. A
  `SecurityPolicyViolationEvent` is mapped to `CspViolationDto`:
  - The directive maps to its enum value, and an unknown directive becomes `other`.
  - `blockedURI` is reduced to its scheme, or to `inline`/`eval`.
  - `sample` and the full URL are never sent.
  - An IPC failure is swallowed and never throws.
- [X] T020 [P] [US2] Write Rust tests for the CSP report command in
  `apps/desktop/src-tauri/src/commands/diagnostics.rs` (`#[cfg(test)]`):
  - Unknown fields are rejected (`deny_unknown_fields`).
  - A `blockedScheme` that does not match `^[a-z][a-z0-9+.-]{0,15}$|^inline$|^eval$` returns `invalid_input`
    without echoing the input.
  - At most 20 log lines per minute, with a summary line counting dropped reports.

### Implementation for User Story 2

- [ ] T021 [US2] Manual: build the current production app (commit `29786c7`) and capture baseline
  screenshots of every screen and dialog (Sessões, nova sessão, consentimento, capturando, pausada, Histórico,
  Detalhe, Exportação, Exclusão, Provedores, Configurações) on macOS, Windows and Linux into
  `docs/release/v0.1.0/evidence/csp-visual/baseline/<os>/`, as PNGs of at most 500 KB each. Rendering differs
  across OSes, so each OS is compared only with its own baseline.
- [X] T022 [US2] Remove `'unsafe-inline'` from `style-src` in `app.security.csp` of
  `apps/desktop/src-tauri/tauri.conf.json`, so the value equals the contract in `contracts/csp-policy.md`.
  Leave `devCsp` unchanged. T017 must now pass.
- [X] T023 [US2] Implement the command `diagnostics_report_csp_violation` in
  `apps/desktop/src-tauri/src/commands/diagnostics.rs`:
  - DTO `CspViolationDto` per `contracts/ipc-updates.md`.
  - Rate limiter of 20 per minute.
  - `tracing::warn!(directive, scheme, disposition, "csp_violation")`.
  Register it in `apps/desktop/src-tauri/src/commands/mod.rs` and in `generate_handler!` in
  `apps/desktop/src-tauri/src/lib.rs`.
  Note: the limiter uses fixed 60 s windows; the dropped-report summary line is written when the next
  window opens with a new report. The `{}` output is `EmptyDto`, with a fixture under
  `contracts/fixtures/EmptyDto/`.
- [X] T024 [US2] Add the `CspViolationDto` type and the `reportCspViolation` method in
  `apps/desktop/src/ipc/types.ts` and `apps/desktop/src/ipc/client.ts`. Add contract fixtures under
  `specs/002-desktop-release-readiness/contracts/fixtures/CspViolationDto/` and extend
  `apps/desktop/src/ipc/contract.test.ts` to load fixtures from the 002 folder as well as the 001 one.
- [X] T025 [US2] Implement `apps/desktop/src/diagnostics/cspReporter.ts`. It installs a
  `securitypolicyviolation` listener on `document` that sends the minimal DTO. Call it once from
  `apps/desktop/src/main.tsx` before rendering. T019 must pass.
- [ ] T026 [US2] Manual: verify research R1 on all three OSes with a local production build
  (`pnpm --filter appsdesktop tauri build`) following quickstart §2. Capture "after" screenshots into
  `docs/release/v0.1.0/evidence/csp-visual/<os>/` and compare each OS with its own baseline. Record the log excerpt
  showing zero `csp_violation` lines and the R1 verification result in
  `specs/002-desktop-release-readiness/research.md`.
  If a runtime-injected style is found: add its specific `'sha256-…'` to the CSP and to the T017 exception
  list, write an ADR exception per the Constitution's Exception Process, and open a `release-blocker` issue
  until the owner approves it.

**Checkpoint**: F-1 is closed in code. Final evidence (C1/C2) is collected on the signed candidate in US1/US3.

---

## Phase 4: User Story 8 — Receber atualizações com verificação criptográfica (Priority: P2)

**Goal**: core-driven updater with a first-run notice, a toggle, confirmation, blocking during sessions, and
signature verification (clarification Q2).

**Independent Test**: in a test channel, N → N+1 legitimate update installs after confirmation; tampered,
other-key and lower-version packages are rejected; zero requests while disabled or while the notice is
pending (quickstart §6).

### Tests for User Story 8 (REQUIRED) ⚠️

- [X] T027 [P] [US8] Write unit tests in `crates/application/tests/update_service.rs`, using a fake
  `AppUpdater`, a fake `SessionActivity`, an in-memory `SettingsRepository` and a manual clock. They cover
  data-model §1.3:
  - With `check_enabled=false`: zero calls, state `disabled`.
  - With `notice_acknowledged=false`: zero calls, state `notice_pending`.
  - `check` → `available`/`up_to_date`/`failed{reason}`.
  - `install(v)` with a different version → `version_mismatch`.
  - `install` while a session is `starting`/`capturing`/`paused`/`stopping` → `blocked_by_session`. The check
    is repeated right before `installing`: a session that starts during the download blocks the install.
  - A concurrent `check`/`install` → `invalid_state`.
  - Disabling cancels a check or download in progress.
  - Every failure leaves the "installed version unchanged" flag intact.
  - The scheduler fires at startup and every 24 h only when enabled and acknowledged.
- [X] T028 [P] [US8] Add unit tests in `crates/application/src/settings_service.rs` (`#[cfg(test)]`) for the
  keys `updates.check_enabled` (default true), `updates.notice_acknowledged` (default false) and
  `updates.last_checked_at` (invalid → absent).
- [X] T029 [P] [US8] Write adapter contract tests in `apps/desktop/src-tauri/tests/updater_adapter.rs`, with
  `wiremock` serving `latest.json` and packages signed by a test minisign key generated into
  `apps/desktop/src-tauri/tests/fixtures/updater/`, and `tauri::test` mock runtime. Cases:
  - A valid newer update → `AvailableUpdate`.
  - Same or lower version → none.
  - Bad signature, or signature from another key → `signature_invalid`.
  - Malformed manifest → `manifest_invalid`.
  - Truncated download → `download_interrupted`.
  - A non-HTTPS endpoint or URL is rejected.
  - `notes` longer than 4 000 characters is truncated.
  First confirm that `tauri-plugin-updater` works under the `tauri::test` mock runtime. If it does not, move the
  conversion, HTTPS and notes rules into pure functions in `apps/desktop/src-tauri/src/updater.rs` and test them
  there (`#[cfg(test)]`). The signature and download cases are then covered against the real plugin by T047, and
  the decision is recorded in `research.md` R5.
  *The plugin works under the mock runtime, so these tests run against the real plugin (no fallback to
  pure functions). Debug builds of the plugin accept `http://` endpoints, so `TauriUpdater` validates the
  config itself before any request. Recorded in research R5.*
- [X] T030 [P] [US8] Write a capability guard test in `apps/desktop/src-tauri/tests/capabilities.rs`: every
  file in `apps/desktop/src-tauri/capabilities/` grants exactly `core:default` and no `updater:*`, `dialog:*`,
  `fs:*`, `http:*` or `shell:*` permission.
- [X] T031 [P] [US8] Write frontend tests in `apps/desktop/src/features/updates/updates.test.tsx`, with
  `vitest-axe` on each state:
  - `UpdateNotice` lists the destinations from `contracts/csp-policy.md`, and "Entendi" calls
    `update_notice_acknowledge`.
  - `UpdateBanner` shows version and notes, and notes containing `<b>x</b>` render as literal text.
  - `UpdateConfirmDialog` is keyboard-only operable, traps focus, closes with Escape, and calls
    `update_install` with the displayed version.
  - The `blocked_by_session` and `failed` messages are announced in a polite live region.
  - The Settings toggle calls `settings_update({ updateCheckEnabled })`.
- [X] T032 [P] [US8] Add contract fixtures for `UpdateStatusDto` (every variant), `UpdateEvent` and the
  extended `SettingsDto` under `specs/002-desktop-release-readiness/contracts/fixtures/`, and the
  corresponding cases in `apps/desktop/src/ipc/contract.test.ts`.
  *The extended `SettingsDto` fixtures stay in `specs/001-meeting-sessions/contracts/fixtures/SettingsDto/`
  (the DTO has one fixture directory; a second one under 002 would be shadowed). `UpdateStatusDto`,
  `UpdateEvent` and `UpdateInstallDto` live under 002.*

### Implementation for User Story 8

- [X] T033 [US8] Add to `crates/application/src/ports.rs`:
  - The trait `AppUpdater` (`async fn check(&self) -> Result<Option<AvailableUpdate>, UpdateFailure>`;
    `async fn download_and_install(&self, update: &AvailableUpdate, progress: &dyn Fn(u64, Option<u64>), cancel: CancellationToken) -> Result<(), UpdateFailure>`;
    `fn restart(&self)`).
  - The value `AvailableUpdate { version, notes, published_at }`.
  - The enum `UpdateFailure { Network, SignatureInvalid, ManifestInvalid, DownloadInterrupted, InstallFailed }`.
  - The trait `SessionActivity { fn has_active_session(&self) -> bool }`.
  *Deviation: the port has separate `download` (download and verify) and `install` steps instead of
  `download_and_install`, so the use case can re-check sessions right before installing (T027, data-model
  §1.3). Recorded in research R5.*
- [X] T034 [US8] Add the settings keys and accessors (`update_check_enabled`, `set_update_check_enabled`,
  `update_notice_acknowledged`, `acknowledge_update_notice`, `update_last_checked_at`) to
  `crates/application/src/settings_service.rs`. T028 must pass.
- [X] T035 [US8] Implement `SessionActivity` for `SessionService` in
  `crates/application/src/session_service.rs`: true when any session is `starting`, `capturing`, `paused` or
  `stopping`, using the existing `is_active()` plus `stopping`.
- [X] T036 [US8] Add the error variants `UpdatesDisabled`, `NoticePending` and `VersionMismatch` to
  `crates/application/src/error.rs`, and their codes `updates_disabled`, `notice_pending` and
  `version_mismatch` to the IPC error mapping in `apps/desktop/src-tauri/src/commands/error.rs`.
- [X] T037 [US8] Implement `crates/application/src/update_service.rs` (exported from
  `crates/application/src/lib.rs`):
  - The `UpdateStatus` state machine from data-model §1.3.
  - Single-flight operations with a cancellation token.
  - Session check at confirmation and again before installing.
  - `last_checked_at` updates.
  - A status publisher for subscribers.
  - `run_scheduler(cancel)`: a check at startup and every 24 h while enabled and acknowledged.
  T027 must pass.
- [X] T038 [US8] Implement `TauriUpdater` in `apps/desktop/src-tauri/src/updater.rs`:
  - Wraps `tauri_plugin_updater::UpdaterExt` and maps plugin errors to `UpdateFailure`.
  - Rejects non-HTTPS endpoints and URLs.
  - Truncates notes to 4 000 characters.
  - Restarts via `AppHandle::restart`.
  Register the plugin with `tauri_plugin_updater::Builder` in `apps/desktop/src-tauri/src/lib.rs` **without**
  adding any capability permission. T029 and T030 must pass.
- [ ] T039 [US8] Configure `plugins.updater` in `apps/desktop/src-tauri/tauri.conf.json`:
  - `pubkey`: the public key from T006.
  - `endpoints`: `["https://github.com/EderAndrew/dev-copilot/releases/latest/download/latest.json"]`.
  - Windows `installMode` `passive`.
  - `bundle.createUpdaterArtifacts: true`.
  *Done except `pubkey`: endpoints, `installMode: passive`, `createUpdaterArtifacts` and
  `requireSignedVersion: true` are set. `pubkey` holds the marker `PENDING-T006-updater-public-key` until T006.
  With the marker, every check fails with `manifest_invalid` and makes no request, and `updater_config.rs`
  fails when `RELEASE_TAG` is set.*
- [X] T039a [US8] Add `apps/desktop/src-tauri/tests/updater_config.rs`. It asserts that
  `plugins.updater.endpoints` is exactly
  `["https://github.com/EderAndrew/dev-copilot/releases/latest/download/latest.json"]`, that every endpoint is
  HTTPS, and that the key ID of `plugins.updater.pubkey` equals the `updater-pubkey-id` line recorded in
  `docs/adr/ADR-005.md`. A wrong key in production would only surface at 0.1.1, when updates would already be
  broken for everyone.
- [X] T040 [US8] Implement the thin commands `update_status_get`, `update_check`, `update_install`,
  `update_cancel`, `update_notice_acknowledge` and the channel `update_subscribe` in
  `apps/desktop/src-tauri/src/commands/updates.rs` per `contracts/ipc-updates.md`. Add `updateCheckEnabled`,
  `updateNoticeAcknowledged` and `updateLastCheckedAt` to `SettingsDto` in
  `apps/desktop/src-tauri/src/dto.rs` and `apps/desktop/src-tauri/src/commands/settings.rs`, with
  `settings_update` rejecting the two read-only fields with `invalid_input`. Register everything in
  `apps/desktop/src-tauri/src/commands/mod.rs` and `apps/desktop/src-tauri/src/lib.rs`.
- [X] T041 [US8] Wire `UpdateService` in `apps/desktop/src-tauri/src/composition.rs`: inject `TauriUpdater`,
  `SessionService` as `SessionActivity` and the settings repository, and spawn `run_scheduler` with the app
  shutdown token. A failure in the updater must never fail app startup.
- [X] T042 [P] [US8] Add the types `UpdateStatusDto`, `UpdateEvent` and the extended `SettingsDto`, plus the
  client methods and the `subscribeUpdates` channel helper, in `apps/desktop/src/ipc/types.ts` and
  `apps/desktop/src/ipc/client.ts`. T032 must pass.
- [X] T043 [US8] Implement `apps/desktop/src/features/updates/UpdateNotice.tsx`, `UpdateBanner.tsx`,
  `UpdateConfirmDialog.tsx` and `useUpdateStatus.ts` (presentation only; every rule stays in the core). Mount
  the notice and banner in `apps/desktop/src/App.tsx`.
- [X] T044 [US8] Add the "Atualizações" section to `apps/desktop/src/features/settings/SettingsPage.tsx`:
  toggle, "Verificar agora", last check time, and the destinations text. Update
  `apps/desktop/src/features/settings/SettingsPage.test.tsx`.
- [X] T045 [US8] Add every new string to `apps/desktop/src/i18n/pt-BR.json` and `apps/desktop/src/i18n/en.json`
  (notice, destinations, banner, confirmation, blocked, failure reasons, settings). The existing i18n
  completeness test must pass (SC-019 of 001). T031 must pass.
- [X] T046 [US8] Update `specs/001-meeting-sessions/contracts/ipc-commands.md` (`SettingsDto` fields) and
  `docs/local-data.md` (new `updates.*` settings keys, non-sensitive; the opt-in `metrics-<date>.csv`
  diagnostic file: location, columns, no session content, 7-day retention, how to delete it) to reference
  `specs/002-desktop-release-readiness/contracts/ipc-updates.md`.

### Validation for User Story 8 (Manual, on the signed candidate — depends on T057–T061)

- [ ] T047 [US8] Manual: in the test channel (fork with test identities, quickstart §6), run U1–U4 on macOS,
  Windows and Linux AppImage, with a local proxy (mitmproxy):
  - U1: zero update requests before the notice is dismissed; N → N+1 after confirmation, with data intact.
  - U2: tampered, other-key and lower-version packages are rejected and the installed version is unchanged.
  - U3: zero requests with the check disabled.
  - U4: install blocked while capturing or paused.
  Record the results in each `docs/release/v0.1.0/evidence/<platform>.md`.

**Checkpoint**: updater code merged and unit/contract/UI tests green. Manual U1–U4 pending the candidate.

---

## Phase 5: User Story 1 — Instalar um build assinado e verificável (Priority: P1) 🎯 MVP

**Goal**: signed, notarized and verifiable artifacts from a fail-closed pipeline, plus the release candidate
on which every validation runs (closes F-4 together with US8).

**Independent Test**: on a clean user profile of each OS, verify and install only from the public docs,
without bypassing any OS security control; a tampered artifact fails verification (quickstart §4).

### Tests for User Story 1 (REQUIRED) ⚠️

- [X] T048 [P] [US1] Write `apps/desktop/src-tauri/tests/bundle_config.rs`, which asserts in `tauri.conf.json`:
  - `bundle.targets` is exactly `["app","dmg","nsis","deb","appimage"]` (no MSI).
  - `bundle.macOS.minimumSystemVersion == "13.0"`.
  - `bundle.macOS.entitlements` points to `Entitlements.plist`, and that plist contains **only**
    `com.apple.security.device.audio-input`.
  - `bundle.windows.nsis.installMode == "currentUser"` and `webviewInstallMode.type == "embedBootstrapper"`.
  - `bundle.createUpdaterArtifacts == true`.
  - `Info.plist` keeps `NSMicrophoneUsageDescription`.
- [X] T049 [P] [US1] Write `tooling/release/make-manifest.test.mjs` (Node test runner) for T052:
  - It produces `latest.json` per `contracts/release-artifacts.md` with HTTPS URLs only.
  - It omits a platform when its `.sig` is missing.
  - `SHA256SUMS` covers every asset except `SHA256SUMS*`.
  - It fails if an asset is missing from the sums.
  Add a step `node --test tooling/release/` to the Policy job in `.github/workflows/ci.yml` and to the `gates`
  job of `.github/workflows/release.yml` (T051).
  *Done: the step is `node --test "tooling/release/*.test.mjs"` (a bare directory is not a test pattern). It
  runs in the Policy job, which `release.yml` reuses through `workflow_call`. Also covers `collect-bundle.mjs` and
  `windows-sign.mjs`.*

### Implementation for User Story 1

- [X] T050 [US1] Create `apps/desktop/src-tauri/Entitlements.plist` with only
  `com.apple.security.device.audio-input`. Update the `bundle` section of
  `apps/desktop/src-tauri/tauri.conf.json`:
  - targets `["app","dmg","nsis","deb","appimage"]`.
  - macOS: `minimumSystemVersion "13.0"`, `entitlements`, hardened runtime.
  - Windows: `nsis.installMode "currentUser"`, `webviewInstallMode { type: "embedBootstrapper" }`,
    `signCommand` calling the T005 provider tool with the `%1` placeholder, timestamped.
  - Linux: `deb.depends` `libwebkit2gtk-4.1-0` and `libssl3`, and `deb.recommends`
    `gnome-keyring | kwalletmanager`. A Secret Service provider is recommended, not required, because the app
    already blocks with an explanation when none is running (001/FR-039a, matrix case 13).
  T048 must pass.
  *Done: `signCommand` runs `node ../../../tooling/release/windows-sign.mjs %1`. The provider comes from
  `WINDOWS_SIGN_PROVIDER`, and only `artifact-signing` (Azure Artifact Signing, formerly Trusted Signing) is
  implemented, through `artifact-signing-cli`. The script fails closed in CI or a release, and verifies with
  `Get-AuthenticodeSignature`. If T005 picks OV, add that provider.*
- [X] T051 [US1] Write `.github/workflows/release.yml`:
  - Trigger: tags `v*.*.*` (including `-rc.N`).
  - Job `gates` reuses the `ci.yml` steps, plus `RELEASE_TAG=$GITHUB_REF_NAME cargo test --test version_consistency`.
  - Matrix `build` (`macos-15` aarch64 and x86_64, `windows-latest`, `ubuntu-22.04`) under
    `environment: release`.
  - macOS: import the certificate into a temporary keychain, sign, notarize with `notarytool` via the Tauri
    env vars, staple.
  - Windows: provider signing via `signCommand`.
  - Updater `.sig` with `TAURI_SIGNING_PRIVATE_KEY`.
  - `actions/attest-build-provenance` on every artifact.
  - A final job runs `tooling/release/make-manifest.mjs` and creates a **draft** GitHub release with all
    assets.
  - Every signing step fails the job on error, and no step echoes secrets (FR-005, FR-006).
  *Done: `gates` is `ci.yml` via `workflow_call`. `release-config` runs `version_consistency`, `updater_config`
  and `bundle_config` with `RELEASE_TAG`, so it fails until T006. Credentials are checked before the build, and
  every signature, notarization and staple is checked after it. The DMG is signed, notarized and stapled
  explicitly. Assets get canonical names via `collect-bundle.mjs`. No build cache is restored. Actions are
  pinned by tag, like `ci.yml`.*
- [X] T052 [US1] Implement `tooling/release/make-manifest.mjs`, which builds `latest.json` and `SHA256SUMS`
  from the downloaded build artifacts. T049 must pass.
- [X] T053 [P] [US1] Write `docs/release/signing-and-distribution.md`: prerequisites, each identity and key
  and where it lives (names only), how to cut a tag, what each workflow job does, how to sign `SHA256SUMS`
  locally (`gpg --detach-sign --armor`), how to renew certificates, and how to rotate keys. It must be detailed
  enough for SC-012.
- [X] T054 [P] [US1] Write `docs/release/verify-downloads.md` per the user procedure in
  `contracts/release-artifacts.md`: commands for macOS (`shasum`, `spctl -a -vv`), Windows (`Get-FileHash`,
  file properties/UAC) and Linux (`gpg --verify`, `sha256sum -c --ignore-missing`), and the release key
  fingerprint. It must not contain any instruction to disable Gatekeeper, SmartScreen or other controls
  (FR-009).
- [ ] T055 [US1] Manual: rehearse the pipeline in a test fork with test identities (quickstart §3):
  - Tag `v0.0.1-test.1` → draft with all assets.
  - Jobs wait for environment approval.
  - Remove one signing secret → the job fails and no assets are attached.
  Record the results in `docs/release/v0.1.0/pipeline-rehearsal.md` and fix `release.yml`.
  *Note: use `v0.0.1-rc.1` with the fork's version set to `0.0.1`. The version gate rejects `-test.N`
  (`docs/release/signing-and-distribution.md`, "Ensaio do pipeline").*
- [ ] T056 [US1] Manual: confirm on Windows 10 that the signed `-setup.exe` installs without admin rights,
  installs WebView2 via the embedded bootstrapper when it is missing, and shows the verified publisher in UAC.
  Record in `docs/release/v0.1.0/pipeline-rehearsal.md`. Depends on T005.
- [ ] T057 [US1] Tag `v0.1.0-rc.1` after US2 and US8 code and T012–T016 are merged (the version files already
  say `0.1.0`; rc builds report `0.1.0` internally, so testers on an rc are not offered `v0.1.0` by the
  updater and must reinstall it manually; note this in `candidate.md`). Before tagging, sign a throwaway file
  with the production minisign key (`pnpm --filter appsdesktop tauri signer sign`) and verify it against the
  public key in the candidate's `tauri.conf.json`. Record the result in `candidate.md`; it proves the shipped
  key matches the one that will sign 0.1.1. After approving the `release` environment, download the draft, verify every
  signature, sign `SHA256SUMS` locally and attach `SHA256SUMS.asc`. Fill `docs/release/v0.1.0/candidate.md`
  (commit, workflow URL, SHA-256, signing/notarization/timestamp status, attestation links).
- [ ] T058 [P] [US1] Manual: run installation cases I1–I4 on a clean user profile on macOS Apple Silicon
  (quickstart §4): `spctl` shows `Notarized Developer ID`; no "Open anyway"; tampered copy fails; the first
  microphone prompt shows the app name and capture works. Record in
  `docs/release/v0.1.0/evidence/macos-arm64.md`.
- [ ] T059 [P] [US1] Manual: run I1–I4 on clean Windows 10 22H2 and Windows 11 profiles (UAC verified
  publisher; record any SmartScreen reputation prompt as a limitation, never bypass it). Record in
  `docs/release/v0.1.0/evidence/windows-10.md` and `windows-11.md`.
- [ ] T060 [P] [US1] Manual: run I1–I4 for the AppImage and `.deb` on Ubuntu LTS, in both a Wayland and an
  X11 session (`gpg --verify`, `sha256sum -c`; app opens and microphone prompt or behavior works). Record in
  `docs/release/v0.1.0/evidence/linux-wayland.md` and `linux-x11.md`.
- [ ] T061 [P] [US1] Manual, optional: if Intel Mac hardware is available, run I1–I4 for the x64 build into
  `docs/release/v0.1.0/evidence/macos-x64.md`. If not, mark the Intel build "not published" in
  `candidate.md` and list it as a limitation (FR-032).

**Checkpoint**: signed candidate `rc.1` exists and installs cleanly on every declared platform. US3, US4,
US5 and T047 can start.

---

## Phase 6: User Story 3 — Validar captura e sessões em hardware real em cada SO (Priority: P1)

**Goal**: the 001 acceptance matrix fully executed on the signed candidate per platform, with evidence and
limitations (closes F-6, T162, T166 of 001).

**Independent Test**: one platform's evidence file has a result and evidence for every case 1–13, 3b, 3c,
11b and the timed measurements, with zero open critical failures.

- [ ] T062 [P] [US3] Manual: macOS Apple Silicon, clean profile. Run matrix cases 1–13, 3b (Bluetooth),
  3c (sleep and lock), 4 (force quit → `failed (interrupted)`, finals preserved), 12 (system audio unavailable
  with reason), first-run transcription model download behind a restrictive network or corporate proxy (the
  app must explain the failure; record it as a limitation if there is no workaround), the timed
  SC-001/SC-017/SC-006 runs, and C1 (zero `csp_violation` lines on the full walk) and
  C2 (screenshots vs baseline). Record in `docs/release/v0.1.0/evidence/macos-arm64.md`. Open issues with
  severity labels for failures.
- [ ] T063 [P] [US3] Manual: Windows 10 22H2. Same scope as T062, including the Windows microphone privacy
  setting for desktop apps. Record in `docs/release/v0.1.0/evidence/windows-10.md`.
- [ ] T064 [P] [US3] Manual: Windows 11. Same scope as T062. Record in
  `docs/release/v0.1.0/evidence/windows-11.md`.
- [ ] T065 [P] [US3] Manual: Ubuntu LTS Wayland (PipeWire). Same scope as T062, plus case 13 (stop the Secret
  Service → the app explains and blocks) and the WebKitGTK blank-screen check (research R4). Record in
  `docs/release/v0.1.0/evidence/linux-wayland.md`.
- [ ] T066 [P] [US3] Manual: Ubuntu LTS X11. Same scope as T065. Record in
  `docs/release/v0.1.0/evidence/linux-x11.md`.
- [ ] T067 [US3] Update `docs/adr/ADR-004.md` "Resultado do spike S3" with the real Windows and Linux results
  from T063–T066 (encrypted DB opens, credential store round trip, `secret_store_unavailable` behavior), and
  change its status accordingly (FR-016).
- [ ] T068 [US3] Fix every `release-blocker` issue found in T062–T066 with a regression test next to the
  affected code, rebuild a new `rc.N` (T057 procedure), and re-run the affected cases. A kept case needs a
  written justification that the commit difference does not affect it (`contracts/release-evidence.md`).
- [ ] T069 [US3] Write `docs/release/v0.1.0/limitations.md`: consolidated per-platform limitations
  (permissions, compatibility, SmartScreen reputation, `.deb` not auto-updating, Keychain identity note for
  former dev-build users, WebKitGTK workaround if needed), each linked to its evidence (FR-015).
- [ ] T070 [US3] Mark T162 and T166 done in `specs/001-meeting-sessions/tasks.md` with a note linking
  `docs/release/v0.1.0/evidence/`. Fill the "Resultado" table of
  `specs/001-meeting-sessions/acceptance-matrix.md` with links to the per-platform evidence files.

**Checkpoint**: every declared platform has complete evidence; F-6 can be dispositioned.

---

## Phase 7: User Story 4 — Validar acessibilidade com teclado e leitores de tela (Priority: P1)

**Goal**: keyboard-only and screen-reader validation on each OS, including the new update UI, with the
capture state announced as text.

**Independent Test**: cases 11 and 11b plus the update dialogs completed without a mouse and with the screen
reader only, with the announcements recorded.

- [ ] T071 [P] [US4] Manual: VoiceOver on macOS. Keyboard-only flow start → consent → pause → resume → stop →
  review record → export, plus the update notice, banner and confirmation dialog. Record what was announced
  for capturing, paused and not capturing in `docs/release/v0.1.0/evidence/macos-arm64.md` (cases 11, 11b).
- [ ] T072 [P] [US4] Manual: Narrator **and** NVDA on Windows 11, same scope as T071, each reader recorded
  separately in `docs/release/v0.1.0/evidence/windows-11.md`. Repeat with Narrator on Windows 10 in
  `windows-10.md`.
- [ ] T073 [P] [US4] Manual: Orca on Ubuntu Wayland and X11, same scope as T071. Record in
  `docs/release/v0.1.0/evidence/linux-wayland.md` and `linux-x11.md`.
- [ ] T074 [US4] Fix the accessibility defects found in T071–T073 in the affected components under
  `apps/desktop/src/features/`, extending `apps/desktop/src/a11y.test.tsx` with a regression check for each.
  An unannounced capture state is a `release-blocker` unless the owner approves a documented workaround.

**Checkpoint**: SC-010 evidence complete on every platform.

---

## Phase 8: User Story 5 — Medir desempenho e qualidade contra critérios definidos antes (Priority: P2)

**Goal**: frozen measurement plan, then latency, CPU, memory and analysis-quality results on representative
hardware, each classified.

**Independent Test**: `measurement-plan.md` approval date precedes every result; every metric has a value or
an explicit "not measured" with a reason, and a classification.

- [ ] T075 [US5] Manual: fill and approve `docs/release/v0.1.0/measurement-plan.md` **before any
  measurement**:
  - Reference and minimum hardware per OS.
  - Targets from the contract template (SC-005, SC-006, SC-015 of 001; CPU ≤ 2 % idle, ≤ 50 % capturing).
  - Rest conditions.
  - Transcription model.
  - Ollama version and the analysis model name and digest.
  - Sample sizes.
  Commit it alone, so its date is auditable.
- [ ] T076 [P] [US5] Manual: record the non-confidential speech set: at least 100 utterances in pt-BR and en
  from written technical scripts, read by the team. Store it outside the repo (draft release attachment) and
  list the files with SHA-256 in `docs/release/v0.1.0/measurement-plan.md`. Document virtual input setup
  (BlackHole, VB-CABLE, PipeWire/Pulse null-sink monitor) in `docs/release/v0.1.0/measurements.md` §Método.
- [ ] T077 [P] [US5] Manual: transcription latency on macOS reference (M4) and minimum hardware. Run with
  `DEVCOPILOT_METRICS=1`, play the speech set through the virtual input, plus one live run, then
  `perf-sampler summarize --metrics`. Record p50/p95/max in `docs/release/v0.1.0/measurements.md` and the
  "Resultados do spike S2" table of `docs/adr/ADR-003.md`.
- [ ] T078 [P] [US5] Manual: same as T077 on Windows reference and minimum hardware.
- [ ] T079 [P] [US5] Manual: same as T077 on Linux reference and minimum hardware.
- [ ] T080 [P] [US5] Manual: a 2-hour continuous capture with local transcription on the installed app, on
  each OS's reference hardware, with `perf-sampler sample --interval 5`. Also run 30 minutes idle. Record mean
  CPU, RSS growth versus 10 minutes, and control/indicator latency (p95 of 20 runs per control, on **both** the reference and the minimum hardware
  of each OS) in
  `docs/release/v0.1.0/measurements.md`, attaching the CSVs to the draft release.
- [ ] T081 [US5] Manual: analysis evaluation with Ollama and the planned model:
  `DEVCOPILOT_EVAL_PROVIDER=ollama:<model> cargo test -p devcopilot-intelligence --release -- --ignored evaluation prompt_injection qa_evaluation`.
  Record recall, grounding, invented-item rate, injection and Q&A results against SC-008/SC-010/SC-011 in
  `docs/release/v0.1.0/measurements.md` and in the reference-provider table of `docs/adr/ADR-003.md`. A
  provider error or timeout is recorded as such, never counted as a pass.
- [ ] T082 [US5] Classify every result in `docs/release/v0.1.0/measurements.md` as `ok`, `limitação`
  (with a recommended model or hardware profile, also added to `docs/release/v0.1.0/limitations.md`) or
  `bloqueador` (open a `release-blocker` issue). A target changed after T075 needs a history row in
  `measurement-plan.md` with a justification and approver.

**Checkpoint**: SC-006 through SC-009 evidenced or turned into documented limitations.

---

## Phase 9: User Story 7 — Decidir o suporte a áudio do sistema por plataforma (Priority: P3)

**Goal**: spike S1 executed per the ADR-002 protocol and its results recorded. Nothing ships (clarification
Q1).

**Independent Test**: ADR-002 has a per-OS row with protocol results or "adiado"; matrix case 12 shows the
source unavailable on every platform.

- [ ] T083 [US7] Create the branch `spike/s1-system-audio` from `002-desktop-release-readiness` for throwaway
  prototypes (Core Audio taps / ScreenCaptureKit, WASAPI loopback, PulseAudio monitor / PipeWire). It is never
  merged. Add a note at the top of `docs/adr/ADR-002.md` stating that rule.
- [ ] T084 [P] [US7] Manual: run S1 protocol items 1–6 of `docs/adr/ADR-002.md` on macOS hardware and record
  OS version, hardware, date, executor and per-item result in the ADR-002 table.
- [ ] T085 [P] [US7] Manual: run S1 protocol items 1–5 on Windows 10/11 hardware and record them in
  `docs/adr/ADR-002.md`.
- [ ] T086 [P] [US7] Manual: run S1 protocol items 1–5 on Linux (PipeWire and PulseAudio) and record them in
  `docs/adr/ADR-002.md`. Mark any platform not run as "adiado" with the reason.
- [ ] T087 [US7] Complete `docs/adr/ADR-002.md`:
  - The decision per OS (aprovado, reprovado or adiado).
  - Permission and compatibility limitations (minimum OS versions, entitlements or permissions,
    audio-stack combinations).
  - The criteria a future feature must meet to declare support (FR-026).
  - A statement that release 0.1.0 ships microphone only.
  Update the T150–T153 notes in `specs/001-meeting-sessions/tasks.md` to point to the ADR (T150 is done when
  the spike is recorded; T151–T153 are deferred to a future feature).

**Checkpoint**: US7 complete; it does not block the release.

---

## Phase 10: User Story 6 — Decidir a publicação com checklist, evidências e plano de rollback (Priority: P2)

**Goal**: traceable go/no-go decision and a rehearsed rollback.

**Independent Test**: a dry run with an open blocker yields "não publicar"; the rollback rehearsal in the
test channel completes within 2 hours without local data loss.

- [ ] T088 [P] [US6] Write `docs/release/rollback.md`:
  - Triggers: critical defect, security incident, key compromise.
  - Roles.
  - Steps: unpublish or mark assets, republish the previous `latest.json` or remove a platform from it, ship a
    fix-forward N+1, public notice template.
  - Signing identity revocation and rotation (Apple, Windows, OpenPGP).
  - Updater key rotation (a release signed with the old key that embeds the new one) and loss (manual
    reinstall).
  - Local data compatibility (newer schema refusal from `docs/local-data.md`; a rollback must never delete or
    migrate user data).
- [ ] T089 [US6] Manual: rehearse the rollback in the test fork (SC-013). Publish N+1, trigger, suspend via
  `latest.json`, publish the notice, and confirm that installed N clients no longer see the update and that
  local sessions are intact. Time it (≤ 2 h) and record it in `docs/release/v0.1.0/rollback-rehearsal.md`.
- [ ] T090 [US6] Manual: a project member who did not run T055/T057 reproduces a test-channel release using
  only `docs/release/signing-and-distribution.md` (SC-012). Record the gaps found and fix the doc in
  `docs/release/v0.1.0/reproduction.md`.
- [ ] T091 [US6] Update `specs/001-meeting-sessions/security-review.md`:
  - Final disposition of F-1 (T017/T022/T026/C1), F-4 (T050–T061, T047) and F-6 (T062–T066, T071–T073).
  - New findings, if any.
  - The threat model rows from `specs/002-desktop-release-readiness/plan.md`.
  - A new explicit approval section, dated, by the release owner (Art. IV.10, FR-033).
- [ ] T092 [US6] Change `docs/adr/ADR-005.md` from *Proposto* to *Aceito*, with the identities actually used
  and the verification results.
- [ ] T093 [P] [US6] Update `README.md` and `apps/desktop/README.md`: supported platforms and versions **only
  as evidenced**, a download verification link, the release key fingerprint, update behavior and how to turn
  it off, system audio "not supported in 0.1.0", and a link to `docs/release/v0.1.0/limitations.md` (FR-032).
  Draft the release notes in `docs/release/v0.1.0/release-notes.md` with the same content.
- [ ] T094 [US6] Fill `docs/release/v0.1.0/checklist.md` from `docs/release/checklist.md` with every item
  linked to evidence. Rehearse the "não publicar" path: with a deliberately open `release-blocker` test issue,
  the checklist cannot reach 12/12, and that dry run is noted in the file. Then close the test issue.
- [ ] T095 [US6] Manual: with 12/12 items and zero open `release-blocker` issues, tag `v0.1.0` (T057
  procedure; it must produce the same commit as the last validated `rc.N` or re-justify the evidence). Verify
  the draft, sign and attach `SHA256SUMS.asc`, and record `docs/release/v0.1.0/decision.md` (owner, date,
  version, commit, checksums, F-1…F-6 disposition, limitations). Smoke-test the final `v0.1.0` artifacts on one
  machine per declared platform: I1 (verification), I2 (installation), I4 (first microphone prompt or behavior)
  and matrix case 2 (full cycle). Record them in an "Artefatos finais" section of each `evidence/<platform>.md`.
  Any failure blocks publication. Only then publish the GitHub draft release.

**Checkpoint**: release 0.1.0 published with a recorded approval, or a recorded "não publicar".

---

## Phase 11: Polish & Cross-Cutting Concerns

- [ ] T096 [P] Re-run the 001 security acceptance cases on the signed candidate and link them in
  `docs/release/v0.1.0/checklist.md` item 8: denied permission, start/stop enforcement, IPC validation
  (including the new commands), log and metrics redaction, deletion, and prompt injection (Art. IV.16).
- [ ] T097 Run every CI gate locally and in CI on the final commit (`cargo fmt --all -- --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo audit`,
  `cargo deny check`, gitleaks, `pnpm --filter appsdesktop check`, `test:dist`). Record the results in
  `docs/release/v0.1.0/checklist.md` item 1.
- [ ] T098 Run `specs/002-desktop-release-readiness/quickstart.md` §1–§8 end to end, fix any stale command or
  path in the quickstart, and mark the result in `docs/release/v0.1.0/checklist.md`.
- [ ] T099 [P] Update the `CLAUDE.md` "Commands" section with `pnpm --filter appsdesktop test:dist`, the
  `perf-sampler` usage and the release tag procedure link.

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: none.
- **Foundational (Phase 2)**: depends on Setup. T005–T008 (identities and keys) have external lead time and
  block T039 (pubkey), T050 (`signCommand`) and T051–T061. T012–T016 must be merged before T057 (candidate).
- **US2 (Phase 3)** and **US8 code (Phase 4, T027–T046)**: depend only on Setup and Foundational code tasks.
  They can run in parallel with the identity procurement.
- **US1 (Phase 5)**: T048–T056 can start after Setup. T057 (candidate) requires US2, the US8 code, T012–T016
  and T005–T008. T058–T061 require T057.
- **US3, US4, US5 measurements, T047**: require the candidate (T057) and installation (T058–T061).
- **US7 (Phase 9)**: independent of everything (hardware permitting). It does not block the release.
- **US6 (Phase 10)**: T088 can start any time. T089–T095 require US1, US3, US4, US5 and US8 validation to be
  complete.
- **Polish (Phase 11)**: before T095 (T096–T098 feed the checklist).

### User Story Dependencies

- **US2 (P1)**: independent. Its final evidence (C1/C2) is collected on the candidate in T062–T066.
- **US8 (P2)**: code independent. Manual validation (T047) needs US1's candidate.
- **US1 (P1)**: needs US2 and US8 code merged before the candidate, so that the validated binary is the
  shipped binary.
- **US3 (P1), US4 (P1)**: need US1's candidate.
- **US5 (P2)**: needs T075 before any measurement, and the candidate for T077–T080. T081 can run on any
  machine with Ollama.
- **US7 (P3)**: independent.
- **US6 (P2)**: needs everything else to finish.

### Within Each User Story

- Tests are written first and must fail before implementation (T017 fails on the current CSP; T027–T032 fail
  before T033–T045).
- Ports → service → adapter → commands → UI (US8).
- The measurement plan is frozen before any measurement (US5).
- Any fix after a candidate → new `rc.N` → re-run the affected cases (T068).

### Parallel Opportunities

- Setup: T002, T003, T004.
- Foundational: T010, T011, T013, T016 in parallel. T005–T008 in parallel with all code work.
- US2 tests T017–T020 together; US8 tests T027–T032 together.
- US2 (Phase 3) and US8 (Phase 4) in parallel by two people (different files except `lib.rs`/`mod.rs`
  registration: coordinate those two edits).
- US1 docs T053, T054 and tests T048, T049 in parallel with the US8 implementation.
- Per-platform manual tasks are all [P]: T058–T061, T062–T066, T071–T073, T077–T080, T084–T086, on different
  machines.

---

## Parallel Example: User Story 8

```bash
# Tests first, together:
Task: "T027 UpdateService unit tests in crates/application/tests/update_service.rs"
Task: "T029 Updater adapter contract tests in apps/desktop/src-tauri/tests/updater_adapter.rs"
Task: "T030 Capability guard test in apps/desktop/src-tauri/tests/capabilities.rs"
Task: "T031 Update UI tests in apps/desktop/src/features/updates/updates.test.tsx"
Task: "T032 Contract fixtures for UpdateStatusDto/UpdateEvent/SettingsDto"

# Then, after T033–T041, frontend types in parallel with docs:
Task: "T042 IPC types/client in apps/desktop/src/ipc/"
Task: "T046 Update 001 ipc-commands.md and docs/local-data.md"
```

## Parallel Example: real-hardware validation (after T057)

```bash
Task: "T062 [US3] macOS matrix → docs/release/v0.1.0/evidence/macos-arm64.md"
Task: "T063 [US3] Windows 10 matrix → evidence/windows-10.md"
Task: "T065 [US3] Ubuntu Wayland matrix → evidence/linux-wayland.md"
Task: "T080 [US5] 2 h soak per OS → measurements.md"
Task: "T047 [US8] U1–U4 in the test channel per OS"
```

---

## Implementation Strategy

### MVP (smallest publishable release)

All of the following are release-blocking (F-1, F-4, F-6 and clarification Q2). The "MVP" here is the
release itself, not a single story:

1. Phase 1 + Phase 2 (start T005–T008 on day one).
2. Phase 3 (US2) and Phase 4 code (US8) in parallel.
3. Phase 5 (US1): pipeline, candidate, clean installs. **Stop and validate**: installable on every declared
   platform.
4. Phases 6–7 (US3, US4) and T047 on the candidate; fix → `rc.N` → re-run.
5. Phase 8 (US5) measurements and classification.
6. Phase 10 (US6): checklist, rollback rehearsal, decision, publish.

US7 (Phase 9) runs whenever hardware allows and never blocks.

### Incremental Delivery

1. Setup + Foundational → evidence templates, keys and metrics ready.
2. US2 → F-1 closed in code (verifiable on local production builds).
3. US8 code → updater tested with fakes and a mock server.
4. US1 → first signed `rc.1`: the first point where real-hardware evidence counts.
5. US3/US4/US5/T047 → evidence accumulates per platform; each platform can be approved independently.
6. US6 → publish.

### Parallel Team Strategy

- Release owner: T005–T009, T057, T075, T091–T095.
- Developer A: US2, then US1 code (T048–T054).
- Developer B: US8 code, then T012–T016 if not done.
- Testers, one per OS: T058–T066, T071–T073, T077–T080, T047; and T084–T086 when hardware is free.

---

## Notes

- Every manual task writes its result to a versioned file under `docs/release/v0.1.0/` or `docs/adr/`. A
  manual task without a committed record is not done.
- Use only non-confidential test audio and text (Art. IV.16). Evidence screenshots must not show credentials
  or real meeting content.
- Never weaken a control to pass a check: no `'unsafe-inline'`, no unsigned artifact, no "Open anyway" or
  SmartScreen bypass instructions.
- Commit after each task or logical group. Coordinate the shared registration files
  (`apps/desktop/src-tauri/src/lib.rs`, `apps/desktop/src-tauri/src/commands/mod.rs`) between US2 and US8.
