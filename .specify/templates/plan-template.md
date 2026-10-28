# Implementation Plan: [FEATURE]

**Branch**: `[###-feature-name]` | **Date**: [DATE] | **Spec**: [link]
**Input**: Feature specification from `/specs/[###-feature-name]/spec.md`

**Note**: This template is filled in by the `/speckit.plan` command. See `.specify/templates/plan-template.md` for the execution workflow.

## Summary

[Extract from feature spec: primary requirement + technical approach from research]

## Technical Context

<!--
  ACTION REQUIRED: Replace the content in this section with the technical details
  for the project. The structure here is presented in advisory capacity to guide
  the iteration process.
-->

**Language/Version**: [e.g., Python 3.11, Swift 5.9, Rust 1.75 or NEEDS CLARIFICATION]  
**Primary Dependencies**: [e.g., FastAPI, UIKit, LLVM or NEEDS CLARIFICATION]  
**Storage**: [if applicable, e.g., PostgreSQL, CoreData, files or N/A]  
**Testing**: [e.g., pytest, XCTest, cargo test or NEEDS CLARIFICATION]  
**Target Platform**: [e.g., Linux server, iOS 15+, WASM or NEEDS CLARIFICATION]
**Project Type**: [e.g., library/cli/web-service/mobile-app/compiler/desktop-app or NEEDS CLARIFICATION]  
**Performance Goals**: [domain-specific, e.g., 1000 req/s, 10k lines/sec, 60 fps or NEEDS CLARIFICATION]  
**Constraints**: [domain-specific, e.g., <200ms p95, <100MB memory, offline-capable or NEEDS CLARIFICATION]  
**Scale/Scope**: [domain-specific, e.g., 10k users, 1M LOC, 50 screens or NEEDS CLARIFICATION]

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

Source: `.specify/memory/constitution.md`. Mark each gate ✅ pass, ❌ violation (justify in
Complexity Tracking with an ADR), or N/A.

- [ ] **I. Idiomatic Rust**: typed errors, no `unwrap`/`expect` on recoverable paths, no
      `unsafe` without `// SAFETY:` + tests, fmt/clippy clean.
- [ ] **II. Architecture**: modular monolith; `domain` free of Tauri/audio/DB/AI deps; use cases
      and ports in `application`; adapters wired in `apps/desktop/src-tauri`; no crate cycles.
- [ ] **III. Domain modeling**: session lifecycle states/transitions explicit; requirements,
      decisions, risks, questions, tasks are structured data; AI suggestions carry provenance
      and status and never overwrite confirmed decisions.
- [ ] **IV. Security & privacy**: threat model present if the feature touches audio, files,
      network, credentials, OS APIs, or AI; Tauri capabilities least-privilege; IPC input
      validated; secrets in OS credential storage; local-first, remote transfer disclosed and
      authorized; retention/deletion defined.
- [ ] **V. Cross-platform audio**: OS specifics behind adapters; bounded channels, backpressure,
      cancellation; macOS/Windows/Linux acceptance plan.
- [ ] **VI. AI & human control**: providers behind ports; no vendor types in domain; analysis
      bounded and non-blocking for capture; no autonomous actions.
- [ ] **VII. Monorepo**: correct placement (`apps/`, `crates/`, `packages/`, `tooling/`); thin
      Tauri commands; accurate Turbo inputs/outputs; each new dependency justified.
- [ ] **VIII. Testing/DoD**: acceptance criteria testable; unit tests for domain/use cases;
      contract/integration tests for adapters; security acceptance cases (Art. IV.16) covered.
- [ ] **IX. Docs**: ADRs for long-term decisions; docs updated without claiming unvalidated
      functionality.

## Project Structure

### Documentation (this feature)

```text
specs/[###-feature]/
├── plan.md              # This file (/speckit.plan command output)
├── research.md          # Phase 0 output (/speckit.plan command)
├── data-model.md        # Phase 1 output (/speckit.plan command)
├── quickstart.md        # Phase 1 output (/speckit.plan command)
├── contracts/           # Phase 1 output (/speckit.plan command)
└── tasks.md             # Phase 2 output (/speckit.tasks command - NOT created by /speckit.plan)
```

### Source Code (repository root)
<!--
  ACTION REQUIRED: Replace the placeholder tree below with the concrete layout
  for this feature. Delete unused options and expand the chosen structure with
  real paths (e.g., apps/admin, packages/something). The delivered plan must
  not include Option labels.
-->

```text
# Dev Copilot monorepo (keep only the paths this feature touches)
apps/desktop/
├── src/                     # React/TypeScript UI (no business rules)
└── src-tauri/               # Composition root + thin Tauri commands, capabilities/

crates/
├── domain/                  # Pure entities, value objects, invariants (no infra deps)
├── application/             # Use cases + ports
├── audio/                   # OS audio adapters (macOS / Windows / Linux)
├── intelligence/            # Transcription/LLM provider adapters
└── persistence/             # SQLite repositories

packages/                    # Shared TS packages (only with multiple consumers)
tooling/                     # Build orchestration wrappers
docs/adr/                    # Architecture Decision Records
```

**Structure Decision**: [Document the selected structure and reference the real
directories captured above]

## Complexity Tracking

> **Fill ONLY if Constitution Check has violations that must be justified.** Each violation
> of a MUST requires an ADR with owner, risk, mitigation, and review/expiry date.

| Violation | Why Needed | Simpler Alternative Rejected Because | ADR |
|-----------|------------|-------------------------------------|-----|
| [e.g., additional process] | [current need] | [why in-process insufficient] | [ADR-NNN] |
| [e.g., unsafe FFI block] | [specific problem] | [why safe binding insufficient] | [ADR-NNN] |
