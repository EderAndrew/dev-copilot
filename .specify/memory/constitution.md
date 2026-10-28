<!--
Sync Impact Report
==================
Version change: (unversioned template) → 1.1.0
  - First concrete ratification. The version number 1.1.0 is supplied by the project owner
    and preserved as-is; there is no earlier filled-in constitution in this repository.

Modified principles (template placeholders → concrete articles):
  - [PRINCIPLE_1_NAME] → I. Idiomatic Rust and Clean Code
  - [PRINCIPLE_2_NAME] → II. SOLID, Modular Architecture, and Dependency Direction
  - [PRINCIPLE_3_NAME] → III. Patterns and Domain Modeling
  - [PRINCIPLE_4_NAME] → IV. Security, Privacy, and Trust Boundaries
  - [PRINCIPLE_5_NAME] → V. Cross-Platform Audio and Concurrency
Added principles:
  - VI. AI Analysis and Human Control
  - VII. Monorepo and Dependency Governance
  - VIII. Testing and Definition of Done
  - IX. Documentation and Specification Workflow
Added sections:
  - Purpose and Scope (preamble, incl. normative language and exception process)
  - Initial Architecture Decisions (v1.0)   [was SECTION_2]
  - Exception Process                       [was SECTION_3]
  - Governance (authority, amendment, versioning, compliance, review cadence)
Removed sections: none

Templates requiring updates:
  - ✅ .specify/templates/plan-template.md   (Constitution Check gates, real monorepo layout)
  - ✅ .specify/templates/spec-template.md   (Out of Scope, Failure Cases, Security & Privacy,
                                              Platform Impact sections)
  - ✅ .specify/templates/tasks-template.md  (tests mandatory, security/threat-model and
                                              platform-acceptance tasks, monorepo paths)
  - ✅ .specify/templates/checklist-template.md (no principle-specific content; no change needed)
  - ✅ README.md, apps/desktop/README.md — rewritten for Dev Copilot; state current
                   (skeleton-only) status per Article IX.4.

Follow-up TODOs:
  - ✅ docs/adr/ADR-001.md records the Initial Architecture Decisions.
  - Add CI workflow implementing Article VIII.2–3 (fmt, clippy -D warnings, tests,
    cargo audit, cargo deny, secret scanning, TS checks).
-->

# Dev Copilot Constitution

**Status**: Active

## Purpose and Scope

Dev Copilot is a cross-platform desktop engineering assistant for macOS, Windows, and Linux.
It captures user-authorized system/microphone audio, transcribes technical conversations,
identifies requirements and risks, maintains an incremental development plan, and supports
contextual questions. It does not integrate with meeting platforms.

This constitution governs the Rust core, Tauri application, React/TypeScript frontend,
monorepo tooling, tests, documentation, releases, and future adapters.

The terms MUST, MUST NOT, SHOULD, and MAY are normative. An exception to a MUST requires the
process defined in [Exception Process](#exception-process); exceptions cannot override legal
or organizational requirements.

## Core Principles

### I. Idiomatic Rust and Clean Code

1. Production Rust MUST follow the Rust API Guidelines and favor ownership, borrowing,
   explicit lifetimes when necessary, enums, pattern matching, `Result`, and composition over
   class-like hierarchies.
2. Each module and function MUST have a coherent responsibility and expressive names. Avoid
   speculative abstractions, unnecessary generic parameters, and premature crate splitting.
3. Recoverable errors MUST be represented and propagated using typed errors or appropriate
   error contexts. `unwrap()`/`expect()` MUST NOT be used for recoverable failures in
   production paths. Panic is reserved for broken invariants and documented unrecoverable
   conditions.
4. `unsafe` MUST be avoided. Every necessary `unsafe` block MUST include a `// SAFETY:`
   explanation of invariants, minimal scope, focused tests, and review of the FFI boundary.
5. Public interfaces MUST document behavioral guarantees, failure modes, and cancellation
   semantics where applicable. Comments explain intent and non-obvious constraints, not
   obvious syntax.
6. Formatting MUST use `cargo fmt`; Clippy warnings MUST be treated as failures in CI, with
   narrow documented allowances when justified.

### II. SOLID, Modular Architecture, and Dependency Direction

1. The codebase MUST remain a modular monolith for the MVP. A network backend, microservice,
   or additional process MUST NOT be introduced without a documented requirement and ADR.
2. The `domain` crate MUST contain pure business entities, value objects, invariants, and
   rules. It MUST NOT depend on Tauri, platform audio APIs, databases, or AI SDKs.
3. The `application` crate MUST own use cases and the ports needed by those use cases.
   Infrastructure crates MUST implement ports and be wired at the composition root
   (`apps/desktop/src-tauri`).
4. Apply SOLID pragmatically: single responsibility, extension through stable interfaces,
   substitutable implementations, focused traits, and dependency inversion. Do not introduce
   traits for single implementations without a testing or variation need.
5. Favor explicit constructor injection, traits/generics, and composition. Use `dyn Trait`,
   `Arc`, and synchronization primitives only when required by runtime polymorphism, shared
   ownership, or concurrency.
6. Circular crate dependencies are prohibited. A dependency change crossing architectural
   boundaries MUST be reviewed.

### III. Patterns and Domain Modeling

1. Strategy MUST be considered for interchangeable AI and transcription providers; Adapter
   for OS-specific audio APIs; Repository for persistence; Factory for validated runtime
   construction; and publish/subscribe for UI/domain events. Use a pattern only when it
   removes concrete coupling or complexity.
2. Session lifecycle MUST be modeled explicitly (e.g., idle, starting, capturing, stopping,
   stopped, failed), with valid transitions and cancellation behavior tested.
3. Requirements, technical decisions, risks, questions, and development tasks MUST be
   represented as structured domain data rather than only free-form generated prose.
4. Each AI-suggested plan change MUST preserve provenance (session/transcript reference when
   available), timestamp, rationale, and status such as proposed, confirmed, rejected, or
   pending. A suggestion MUST NOT silently overwrite a confirmed decision.
5. The assistant MUST distinguish observed statements, inferred requirements, and proposed
   solutions. Missing information MUST be presented as uncertainty or an open question, not
   fabricated fact.

### IV. Security, Privacy, and Trust Boundaries

1. Audio capture MUST require explicit user initiation and applicable OS permissions; capture
   state MUST remain visibly indicated. Stopping MUST halt capture and processing promptly.
   No covert capture or automatic recording by default.
2. The application MUST support compliance with organizational meeting policies, participant
   notification/consent requirements, confidentiality obligations, and applicable privacy law
   (including LGPD where relevant). It MUST NOT bypass OS or corporate security controls.
3. Local-first is the default: session content MUST remain local unless the user explicitly
   configures and authorizes an external provider. Before any remote transfer, the UI MUST
   clearly identify the destination/provider and data categories sent.
4. Secrets MUST be stored using appropriate OS credential storage, not plaintext
   configuration, logs, source control, or exported plans. Sensitive session content MUST be
   excluded from diagnostic logs by default.
5. Tauri capabilities and commands MUST use least privilege. All inputs crossing
   frontend/native, process, filesystem, provider, and persistence boundaries MUST be
   validated. Avoid unnecessary localhost servers; if introduced, require authentication,
   binding restrictions, and threat modeling.
6. Transcripts, documents, and LLM outputs MUST be treated as untrusted data. Prompt
   injection MUST NOT grant access to tools, files, secrets, or network actions. Generated
   commands and code are suggestions requiring explicit human approval before execution.
7. Storage MUST have documented retention, deletion, and export controls. Encryption at rest
   MUST be evaluated against the threat model before handling sensitive corporate data; do
   not claim encryption where none is implemented.
8. Dependency and release security MUST include vulnerability and license review, lockfiles,
   scoped permissions, and platform-specific packaging/signing controls as appropriate.

#### Mandatory security standards and verification

9. Security reviews MUST use OWASP Top 10 for web-facing risks, OWASP ASVS as an adaptable
   verification catalog for applicable UI/API and trust-boundary controls, OWASP Top 10 for
   LLM Applications for AI-specific risks, CWE Top 25 for common implementation weaknesses,
   and NIST SSDF (SP 800-218) for secure-development practices. These references MUST be
   applied by relevance rather than represented as desktop-specific certification. OWASP
   MASVS MAY inform local data protection, but is mobile-specific and is not a desktop
   compliance standard.
10. Every feature that accesses audio, files, network, credentials, OS APIs, or AI tools MUST
    document a threat model: assets, actors, trust boundaries, abuse cases, permissions,
    mitigations, and verification tests. Security-sensitive features MUST NOT merge without
    an explicit review.
11. Tauri 2 capabilities and permissions MUST be scoped to the smallest required window,
    command, plugin, and filesystem/network surface. Native IPC commands MUST validate input,
    enforce authorization independently of frontend state, and return safe errors. Arbitrary
    shell execution, unrestricted filesystem access, and permissive content security
    policies MUST NOT be enabled by default.
12. Sensitive local data MUST have a documented classification, retention period,
    secure-deletion limitations, and backup/export policy. Encryption-at-rest requirements
    MUST be determined before storing confidential meeting content; encryption keys MUST NOT
    be bundled with ciphertext or hard-coded in the application. OS keychain/credential vault
    mechanisms SHOULD protect secrets.
13. Remote provider requests MUST require explicit configuration and informed authorization,
    use authenticated TLS, minimize transmitted content, and disclose destination and
    retention implications where known. Network failures MUST fail safely without silent
    fallback to an unapproved provider.
14. AI inputs and outputs MUST be treated as untrusted. Prompt injection, indirect
    instructions in transcripts, output parsing, sensitive-information disclosure, and
    excessive tool permissions MUST be tested. The model MUST NOT independently authorize
    file modification, command execution, or exfiltration; any future action capability
    requires scoped permissions and human confirmation.
15. The release pipeline MUST include `cargo audit` or equivalent Rust advisory checks, Rust
    lint/test checks, JavaScript dependency vulnerability review, secret scanning, and
    reproducible or traceable build artifacts. Distribution MUST use platform-appropriate
    signing and verified update mechanisms when supported. Exceptions MUST document risk,
    owner, mitigation, and expiry.
16. Security acceptance criteria MUST include at minimum: denied-permission behavior,
    start/stop capture enforcement, IPC validation, unauthorized file/network access
    rejection, secret redaction in logs, retention/deletion behavior, and prompt-injection
    resistance for AI features. Tests MUST avoid using real confidential meeting data.

### V. Cross-Platform Audio and Concurrency

1. macOS, Windows, and Linux MUST share application/domain behavior through ports;
   OS-specific capture implementations MUST be isolated behind adapters.
2. Platform implementations MAY use ScreenCaptureKit (macOS), WASAPI loopback (Windows), and
   PipeWire/PulseAudio (Linux), subject to platform capability and permission validation. A
   build passing is not proof that audio capture works on a platform.
3. Audio pipelines MUST use bounded buffers/channels, explicit backpressure policies,
   cancellation, and resource cleanup. They MUST avoid blocking UI and real-time audio
   callbacks with network or heavy inference work.
4. Start/stop, permission denial, device changes, provider failures, and partial
   transcription MUST have predictable, testable behavior. Failure in one adapter MUST NOT
   corrupt the persisted session or confirmed plan.
5. Cross-platform releases MUST be validated on actual macOS, Windows, and Linux
   runners/devices. Linux display/audio stacks and macOS permissions require dedicated
   acceptance testing.

### VI. AI Analysis and Human Control

1. The MVP MUST support both automatic technical analysis and interactive questions using the
   current authorized session context.
2. Providers MUST be replaceable through stable ports; vendor-specific payloads MUST NOT leak
   into domain types.
3. Analysis MUST be incremental, deduplicated where possible, and rate/cost bounded. Model
   latency or failure MUST NOT block capture controls.
4. AI-generated plans MUST identify assumptions, unresolved questions, risks, alternatives,
   and acceptance criteria. Recommendations are advisory, not authoritative.
5. User-confirmed decisions MUST take precedence over new suggestions until the user
   explicitly revises them. No autonomous repository changes, code execution, network calls,
   or publication of meeting content without separate user authorization.
6. Model evaluation MUST include representative technical conversations, regression
   fixtures, factual grounding checks, and checks against invented requirements.

### VII. Monorepo and Dependency Governance

1. The repository MUST use Turborepo for task orchestration, pnpm Workspace for
   JavaScript/TypeScript packages, and Cargo Workspace for Rust crates. Cargo remains the
   source of truth for Rust compilation and dependencies.
2. The workspace root MUST contain `Cargo.toml`, `Cargo.lock`, `package.json`,
   `pnpm-workspace.yaml`, `pnpm-lock.yaml`, and `turbo.json`; lockfiles MUST be committed.
3. `apps/desktop` owns the React/Tauri application. `crates/` owns Rust modules. `packages/`
   contains genuinely shared TypeScript packages; `tooling/` contains build orchestration
   wrappers. Do not create shared packages without multiple consumers or a clear
   architectural boundary.
4. Tauri commands MUST remain thin adapters. Use cases and business rules MUST NOT accumulate
   in `src-tauri` or React components.
5. Turbo task caching MUST declare accurate inputs/outputs; persistent development tasks MUST
   NOT be cached. Rust's Cargo build cache MUST NOT be confused with Turbo cache portability
   across OS targets.
6. Workspace dependency versions SHOULD be centralized where useful. Every added dependency
   MUST have a clear purpose and acceptable maintenance/security posture.

### VIII. Testing and Definition of Done

1. Every feature MUST have testable acceptance criteria before implementation. Domain
   invariants and use cases MUST have unit tests; adapter boundaries MUST have integration or
   contract tests.
2. CI MUST run `cargo fmt --all -- --check`,
   `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, and
   appropriate TypeScript formatting, type checking, linting, and tests.
3. CI MUST include dependency vulnerability checks (`cargo audit` or equivalent),
   dependency/license policy checks (`cargo deny` or equivalent), and secret scanning.
   Tooling failures MUST be investigated, not silently bypassed.
4. OS-specific capture MUST have a documented manual or automated acceptance matrix covering
   macOS, Windows, and Linux. Tests MUST cover permission denial, stop/cancel, provider
   errors, and data deletion.
5. A feature is Done only when tests pass, architectural boundaries remain intact, error
   paths are handled, security/privacy impact is assessed, relevant docs are updated, and
   applicable platform behavior is verified.
6. No arbitrary universal coverage percentage is mandated for v1.0; coverage MUST be
   risk-driven and critical business/security paths MUST be directly tested.

### IX. Documentation and Specification Workflow

1. New features MUST follow the Spec Kit workflow: constitution → specification → plan →
   tasks → implementation, with scope-appropriate review at each stage.
2. Specifications MUST state user outcomes, functional/nonfunctional requirements, explicit
   out-of-scope items, failure cases, and measurable acceptance criteria.
3. Architectural choices with long-term consequences MUST have an ADR documenting context,
   options, decision, tradeoffs, and review triggers.
4. Public APIs, provider contracts, audio permissions, local data formats, and migration
   procedures MUST be documented. Documentation MUST NOT assert functionality that has not
   been implemented or validated.
5. Pull requests MUST describe the requirement, design impact, tests, security/privacy
   considerations, and platform impact. Changes violating this constitution MUST be rejected
   or handled under the formal exception process.

## Initial Architecture Decisions (v1.0)

- **Desktop**: Tauri 2 + React + TypeScript.
- **Native core**: Rust, Cargo Workspace, modular monolith.
- **Monorepo**: Turborepo + pnpm Workspace.
- **Persistence**: SQLite, local-first.
- **Audio**: OS-specific adapters behind an application port.
- **Transcription/LLM**: provider abstractions; local and remote implementations may be added
  only under explicit privacy controls.
- **Out of MVP scope**: no meeting-platform integrations, no mandatory cloud backend, and no
  autonomous code execution.

## Exception Process

An exception to a MUST requires all of the following:

- A documented Architecture Decision Record (ADR) with owner, risk, and rationale.
- Explicit approval.
- A mitigation plan.
- An expiry date or review condition.

Exceptions cannot override applicable law, organizational policy, or platform security
requirements.

## Governance

- **Authority**: This constitution takes precedence over ordinary implementation preferences,
  examples, and feature-level plans. Applicable law, corporate policy, and platform security
  requirements always prevail.
- **Amendment**: Changes require a pull request, rationale, impact assessment, version bump,
  and review of affected specifications/templates. Record significant changes in an ADR.
- **Versioning**: Semantic versioning. MAJOR for incompatible governance changes; MINOR for
  new principles or materially expanded obligations; PATCH for clarifications that do not
  change obligations.
- **Compliance**: Every feature plan and pull request MUST include a constitution check.
  Deviations require an ADR with owner, risk, mitigation, and review/expiry date.
- **Review cadence**: Reassess at major architecture milestones, security incidents, and
  before the first external release.

**Version**: 1.1.0 | **Ratified**: 2026-10-27 | **Last Amended**: 2026-10-27
