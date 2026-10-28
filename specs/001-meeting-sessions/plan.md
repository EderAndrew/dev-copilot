# Implementation Plan: Sessões de reunião e assistência técnica incremental

**Branch**: `001-meeting-sessions` | **Date**: 2026-10-27 | **Spec**: [spec.md](./spec.md)
**Input**: Feature specification from `/specs/001-meeting-sessions/spec.md`

**Note**: This template is filled in by the `/speckit.plan` command. See `.specify/templates/plan-template.md` for the execution workflow.

## Summary

MVP desktop do Dev Copilot: sessões iniciadas pelo usuário com captura de microfone (e áudio do sistema onde
viável), transcrição progressiva, análise técnica incremental em registros estruturados com proveniência e
status, perguntas contextuais, plano revisável, exportação (Markdown + JSON) e exclusão — tudo local-first,
com banco SQLite cifrado (chave no cofre do SO) e provedores locais ou remotos substituíveis atrás de portas,
com autorização explícita para qualquer envio remoto.

Abordagem técnica (detalhes em [research.md](./research.md)): núcleo Rust em monólito modular
(`domain` → `application` → `audio`/`intelligence`/`persistence`, composto em `apps/desktop/src-tauri`);
pipeline de áudio com `cpal` + ring buffer limitado + `tokio` com cancelamento; transcrição local via
whisper.cpp (`whisper-rs`) ou remota compatível com OpenAI; análise via Anthropic, OpenAI-compatible ou
Ollama local com saída JSON validada por schema; SQLite com SQLCipher; UI React com i18n pt-BR/en.

## Technical Context

**Language/Version**: Rust estável (edition 2024; toolchain atual 1.99); TypeScript (React 19, Vite)
**Primary Dependencies**: Tauri 2 (+ `tauri-plugin-dialog`), `tokio`/`tokio-util`, `cpal`, `rtrb`, `rubato`,
`whisper-rs`, `reqwest` (rustls), `rusqlite` (SQLCipher), `keyring`, `serde`, `thiserror`, `tracing`,
`jsonschema`, `uuid`; frontend: `react`, `i18next`/`react-i18next`; testes: Vitest, Testing Library,
`vitest-axe`, `wiremock` (Rust)
**Storage**: SQLite local cifrado (SQLCipher), chave de 256 bits no cofre do SO; áudio bruto nunca persistido
**Testing**: `cargo test` (unit, contrato, integração com portas fake), Vitest; matriz de aceitação manual em
hardware real por SO
**Target Platform**: macOS 13+, Windows 10 22H2+/11 (WebView2), Linux (Ubuntu 22.04+/Fedora 39+, WebKitGTK
4.1, PipeWire ou PulseAudio) — mínimos confirmados nos spikes
**Project Type**: desktop-app (monorepo Turborepo + pnpm + Cargo)
**Performance Goals**: parcial ≤ 5 s / final ≤ 15 s p95 com provedor de referência (SC-005); controles ≤ 500 ms
(SC-006); indicador desligado ≤ 1 s após parar e drenagem ≤ 5 s (SC-002)
**Constraints**: offline-capable quando provedores locais configurados; buffers limitados (~2 s no ring,
~30 s de chunks); nenhum envio remoto sem autorização; conteúdo de sessão fora dos logs; sessão de 2 h com
memória estável (SC-015)
**Scale/Scope**: usuário único; uma sessão ativa por vez; sessões de até várias horas; centenas de sessões
salvas; ~10 telas/painéis

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

Source: `.specify/memory/constitution.md`. Mark each gate ✅ pass, ❌ violation (justify in
Complexity Tracking with an ADR), or N/A.

- [x] ✅ **I. Idiomatic Rust**: erros tipados por crate (`thiserror`); sem `unwrap`/`expect` em caminhos
      recuperáveis; `unsafe` restrito ao adaptador macOS de áudio do sistema (S1), com `// SAFETY:`, escopo
      mínimo, testes e ADR-002; fmt/clippy no CI.
- [x] ✅ **II. Architecture**: monólito modular; `domain` sem Tauri/áudio/DB/IA; portas e casos de uso em
      `application`; adaptadores em `audio`/`intelligence`/`persistence`; composição em `src-tauri`;
      dependências acíclicas (`domain` ← `application` ← infra ← `src-tauri`). Sem novo crate.
- [x] ✅ **III. Domain modeling**: máquina de estados da sessão (com pausa) e de status dos registros
      explícitas e testadas; registros estruturados com natureza, status, proveniência e histórico; IA só cria
      propostas.
- [x] ✅ **IV. Security & privacy**: threat model resumido na spec e detalhado abaixo; capabilities mínimas
      (sem shell/fs/http no frontend; só `dialog:allow-save`); validação em todos os comandos IPC; segredos e
      chave do banco no cofre do SO; `RemoteGate` como ponto único de rede; exclusão com avisos; SQLCipher.
- [x] ✅ **V. Cross-platform audio**: microfone via `cpal`; áudio do sistema por adaptador por SO atrás da
      porta, condicionado a spike; ring/canais limitados com política de descarte e lacunas; cancelamento por
      token; matriz de aceitação em hardware real.
- [x] ✅ **VI. AI & human control**: `Transcriber` e `AnalysisProvider` como portas; DTOs de fornecedor
      confinados em `intelligence`; análise com limites de taxa/custo e nunca bloqueando captura; nenhuma
      ferramenta exposta ao modelo; sem ações autônomas.
- [x] ✅ **VII. Monorepo**: código nos diretórios previstos; comandos Tauri finos; dependências justificadas
      em research.md; Turbo `dev` não cacheado. Ação: adicionar task `test` e inputs Rust no `turbo.json`.
- [x] ✅ **VIII. Testing/DoD**: critérios de aceitação mensuráveis na spec; unit tests de domínio/casos de
      uso; contrato/integração em adaptadores; casos de segurança do Art. IV.16 mapeados (ver tabela).
      Ação: criar CI (R17).
- [x] ✅ **IX. Docs**: ADR-002 (áudio), ADR-003 (provedores), ADR-004 (armazenamento cifrado) previstos;
      docs de contratos, formatos locais e permissões nesta pasta.

**Resultado (pré-pesquisa)**: PASS, sem violações. **Re-check pós-design**: PASS — o design (data-model,
contratos) não introduz processos, servidores locais, plugins amplos ou tipos de fornecedor no domínio.
O uso de um servidor Ollama **pré-existente do usuário** é cliente de serviço local, não servidor introduzido
pelo app (Art. IV.5), e exige configuração explícita.

### Threat model (Art. IV.10)

| Ameaça / abuso | Fronteira | Mitigação | Verificação |
|----------------|-----------|-----------|-------------|
| Captura sem início explícito ou após parar | UI ↔ núcleo ↔ SO | só `session_start` abre streams; token de cancelamento fecha streams; indicador derivado do estado do núcleo | SC-002, SC-003, SC-004; testes de aplicação com fonte fake |
| Captura após negativa/revogação | núcleo ↔ SO | consulta de permissão a cada start/resume; erro tipado sem abrir stream | matriz manual passos 1 e 3 |
| Prompt injection via transcrição | provedor IA | dados delimitados; sem ferramentas; schema estrito; IA só cria propostas | SC-010, suíte de injection |
| Envio remoto não autorizado / fallback | núcleo ↔ rede | `RemoteGate` antes de toda requisição; sem fallback remoto; TLS verificado | SC-013 com `wiremock` + proxy manual |
| Vazamento em logs | núcleo ↔ disco | `Redacted<T>`; sem DTO de conteúdo em `tracing` | SC-014 |
| Leitura do banco por terceiros | disco | SQLCipher; chave no cofre do SO; permissões de arquivo do usuário | SC-018 |
| Exclusão incompleta | disco | cascata + `secure_delete` + checkpoint; aviso de limitações | SC-012; teste de persistência |
| Escrita fora do destino de exportação | núcleo ↔ fs | caminho somente do diálogo nativo; sem `fs` no frontend | teste de comando + revisão de capability |
| IPC malicioso/malformado | frontend ↔ núcleo | validação de DTO e de estado no núcleo; erros sem dados sensíveis | testes de contrato com entradas inválidas |
| Modelo local adulterado | download | URL e SHA-256 fixos; download por ação explícita | teste de hash inválido |

## Project Structure

### Documentation (this feature)

```text
specs/001-meeting-sessions/
├── plan.md              # This file (/speckit.plan command output)
├── research.md          # Phase 0 output (/speckit.plan command)
├── data-model.md        # Phase 1 output (/speckit.plan command)
├── quickstart.md        # Phase 1 output (/speckit.plan command)
├── contracts/           # Phase 1 output (/speckit.plan command)
│   ├── ipc-commands.md
│   ├── ipc-events.md
│   ├── provider-ports.md
│   ├── analysis-output.schema.json
│   └── export-format.md
└── tasks.md             # Phase 2 output (/speckit.tasks command - NOT created by /speckit.plan)
```

### Source Code (repository root)

```text
apps/desktop/
├── src/
│   ├── ipc/                 # tipos TS dos DTOs/eventos + wrappers de invoke
│   ├── i18n/                # pt-BR.json, en.json
│   ├── features/
│   │   ├── sessions/        # lista, criação, controles, indicador de captura
│   │   ├── transcript/      # transcrição progressiva, lacunas
│   │   ├── records/         # registros, plano, histórico
│   │   ├── questions/       # perguntas contextuais
│   │   ├── providers/       # configuração e autorização
│   │   └── settings/
│   └── components/          # componentes compartilhados acessíveis
└── src-tauri/
    ├── src/
    │   ├── main.rs / lib.rs # bootstrap
    │   ├── composition.rs   # raiz de composição (wiring de adaptadores)
    │   ├── commands/        # comandos IPC finos + DTOs + validação
    │   └── events.rs        # ponte EventPublisher → Channel
    ├── capabilities/default.json
    └── Info.plist           # NSMicrophoneUsageDescription (macOS)

crates/
├── domain/src/              # session.rs, record.rs, transcript.rs, question.rs, provider.rs, ids.rs, error.rs
├── application/src/
│   ├── ports.rs             # traits do contrato provider-ports
│   ├── session_service.rs   # criar/iniciar/pausar/retomar/parar, recuperação
│   ├── capture_pipeline.rs  # orquestração áudio → transcrição → eventos (limites, lacunas)
│   ├── analysis_service.rs  # agendamento, limites, conversão em propostas
│   ├── question_service.rs
│   ├── record_service.rs
│   ├── export_service.rs    # Markdown + JSON
│   └── provider_service.rs  # configuração, autorização, RemoteGate
├── audio/src/               # cpal_mic.rs, resample.rs, system/{macos,windows,linux}.rs (feature-gated)
├── intelligence/
│   ├── src/                 # whisper_local.rs, openai_stt.rs, anthropic.rs, openai_chat.rs, ollama.rs,
│   │                        # prompt.rs, output_validation.rs, model_download.rs
│   └── tests/fixtures/      # transcrições de avaliação + casos de prompt injection
└── persistence/src/         # db.rs (SQLCipher), migrations/, repos/*.rs, secret_store.rs (keyring)

docs/adr/                    # ADR-002, ADR-003, ADR-004
.github/workflows/ci.yml     # matriz macOS/Windows/Linux (R17)
```

**Structure Decision**: usa a estrutura existente do monorepo (Art. VII.3) sem novos crates nem pacotes
compartilhados. O cofre de segredos fica em `persistence` (armazenamento local) para evitar crate prematuro
(Art. I.2). Adaptadores de áudio do sistema são compilados por `cfg(target_os)` e habilitados em runtime
apenas após aprovação do spike S1.

## Mapeamento de requisitos de segurança (Art. IV.16)

| Critério obrigatório | Requisitos | Teste |
|----------------------|------------|-------|
| Comportamento com permissão negada | FR-009, FR-010 | aplicação (fonte fake) + matriz manual |
| Enforcement de start/stop | FR-002, FR-005, FR-006a | aplicação + SC-002 |
| Validação de IPC | contracts/ipc-commands.md | testes de comando com entradas inválidas |
| Rejeição de acesso a arquivo/rede não autorizado | FR-046, FR-048, export | `RemoteGate` + `wiremock`; teste de export |
| Redação de segredos em logs | FR-049, FR-051 | SC-014 |
| Retenção/exclusão | FR-042, FR-043 | SC-012 |
| Resistência a prompt injection | FR-029 | SC-010 |

## Riscos e spikes

| Spike | Bloqueia | Fallback se reprovado |
|-------|----------|-----------------------|
| S1 — áudio do sistema por SO | US8 (P2) | opção exibida como indisponível com motivo (FR-014) |
| S2 — transcrição local por SO | default local da US2 | provedor remoto autorizado como caminho do SO (FR-045) |
| S3 — SQLCipher + keyring nos 3 SOs/CI | FR-039a (P1) | bloqueante: sem alternativa aceita sem nova clarificação/ADR |

## Complexity Tracking

> **Fill ONLY if Constitution Check has violations that must be justified.** Each violation
> of a MUST requires an ADR with owner, risk, mitigation, and review/expiry date.

Nenhuma violação. (O `unsafe` previsto no adaptador macOS de áudio do sistema é permitido pelo Art. I.4 com
`// SAFETY:` e testes, e será documentado no ADR-002; não constitui exceção.)
