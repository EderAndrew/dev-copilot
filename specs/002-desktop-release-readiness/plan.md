# Implementation Plan: Prontidão para o primeiro release desktop

**Branch**: `002-desktop-release-readiness` | **Date**: 2026-11-01 | **Spec**: [spec.md](./spec.md)
**Input**: Feature specification from `/specs/002-desktop-release-readiness/spec.md`

## Summary

Levar o Dev Copilot 0.1.0 do estado "implementado e testado no macOS de desenvolvimento" para "publicado,
assinado e validado em hardware real", fechando os bloqueadores de release da revisão de segurança da 001
(F-1 CSP, F-4 assinatura/atualização, F-6 matriz por SO).

Abordagem técnica (detalhes em [research.md](./research.md)):

1. **CSP estrita**: remover `'unsafe-inline'` de `style-src`, proteger contra regressão (teste de
   configuração, lint, checagem do `dist/`) e acrescentar um canal mínimo de relatório de violação para
   enxergar problemas no build assinado (R1).
2. **Distribuição assinada**:
   - macOS: Developer ID, runtime reforçado, notarização e entitlement de microfone (R2).
   - Windows: Authenticode via `signCommand` e instalador NSIS (R3).
   - Linux: AppImage e `.deb`, com `SHA256SUMS` assinado em OpenPGP (R4).
   - Pipeline `release.yml` gerando release em rascunho atrás de um environment protegido (R6).
3. **Atualização verificada**: `tauri-plugin-updater` acionado só pelo núcleo, atrás da porta `AppUpdater`
   e do caso de uso `UpdateService`, que aplica a política: aviso de primeira execução, verificação
   desligável, confirmação e bloqueio durante sessões (R5).
4. **Validação e medição**:
   - Matriz da 001 executada no build candidato assinado por plataforma.
   - Modo de métricas sem texto e `tooling/perf-sampler` para latência, CPU e memória (R7).
   - Harnesses de avaliação da 001 com Ollama (R8).
   - Spike S1 só como registro no ADR-002 (R9).
5. **Processo de release**: checklist, rollback, documentação de assinatura e distribuição, evidências
   versionadas e registro da decisão (R10).

## Technical Context

**Language/Version**: Rust stable (edition 2024); TypeScript strict + React 19 + Vite 8 (sem mudança).
**Primary Dependencies**: Tauri 2.12 (existente). Novas:
- `tauri-plugin-updater` 2.x, usado só no núcleo (R5).
- `sysinfo`, só em `tooling/perf-sampler` e fora do app distribuído (R7).
- `eslint-plugin-react` com a regra `forbid-dom-props`, ou equivalente, se ainda não estiver presente (R1).

Ferramentas de release: Xcode `notarytool`, a ferramenta de assinatura do provedor Windows (R3), GnuPG
(local) e a CLI do Tauri para assinatura minisign.
**Storage**: SQLCipher da 001, sem migração nova. Três chaves novas na tabela `settings` (chave/valor; ver
[data-model.md](./data-model.md)). Evidências de release como Markdown em `docs/release/`.
**Testing**:
- `cargo test`: `UpdateService` com um `AppUpdater` falso, teste de configuração `csp_policy`, extensão do
  teste de redação T157 para o CSV de métricas, e checagem de versão.
- Vitest + Testing Library + `vitest-axe`: seção de atualização nas Configurações, aviso de primeira
  execução, diálogo de confirmação, listener de CSP.
- Teste do `dist/`.
- Matriz manual em hardware real como evidência de plataforma (FR-014: CI não conta como evidência).

**Target Platform**:
- macOS 13+ em Apple Silicon. Intel só se for validado.
- Windows 10 22H2 e 11, x64.
- Linux x86_64: Ubuntu 22.04+ com Wayland e X11. Fedora 39+ só via AppImage e só se for validado.

**Project Type**: aplicativo desktop (monólito modular Tauri + React).
**Performance Goals**: os da 001 revalidados no app instalado: SC-005 (parcial ≤ 5 s, final ≤ 15 s, p95, no
hardware de referência), SC-006 (≤ 500 ms) e SC-015 (RSS ≤ +10 % em 2 h). Metas novas de CPU (≤ 2 % ocioso,
≤ 50 % capturando no hardware de referência) ficam congeladas no plano de medição.
**Constraints**:
- Nenhum backend nem serviço em tempo de execução. Hospedagem estática do GitHub para artefatos e manifesto.
- Os serviços de assinatura são usados só no build.
- Nenhuma permissão nova na webview.
- Nenhum `unsafe` novo.
- Nenhum código de áudio do sistema no release.

**Scale/Scope**:
- 1 release (0.1.0) e 6 plataformas de evidência: `macos-arm64`, `windows-10`, `windows-11`, `linux-wayland`,
  `linux-x11` e `macos-x64` opcional.
- Cerca de 20 telas e estados para percorrer sob CSP.
- 6 comandos IPC novos e 1 canal de eventos novo.
- 1 ADR novo (ADR-005) e 3 ADRs atualizados (002, 003, 004).

Não há itens NEEDS CLARIFICATION em aberto. As premissas que dependem do ambiente real estão marcadas como
**verificar** no research (R1 injeção de estilo pelo webview, R3 elegibilidade do provedor Windows, R5 pilha
TLS do plugin), cada uma com uma alternativa já definida.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

Source: `.specify/memory/constitution.md` (v1.1.0).

- [x] **I. Idiomatic Rust** ✅: `UpdateService` e o adaptador usam erros tipados (`ApplicationError`, com os
      novos `updates_disabled`, `notice_pending` e `version_mismatch`). Nenhum `unwrap`/`expect` em caminho
      recuperável. Nenhum `unsafe` novo; o S1 com FFI fica num branch de spike não mesclado (R9).
      `perf-sampler` segue os mesmos gates de fmt e clippy.
- [x] **II. Architecture** ✅:
  - Continua monólito modular, sem processo novo (o updater roda in-process).
  - Porta `AppUpdater` e caso de uso `UpdateService` em `crates/application`.
  - O adaptador `TauriUpdater` fica em `apps/desktop/src-tauri`, porque depende do `AppHandle`. É o mesmo
    precedente de `NativeSaveDialog` em `commands/export.rs`, e o adaptador não tem regra de negócio.
  - `domain` não muda.
  - `perf-sampler` em `tooling/` não é dependência de nenhum crate do app, então não cria ciclo.
- [x] **III. Domain modeling** ✅: o ciclo de vida da atualização é uma máquina de estados explícita
      (data-model §1.3), com transições e cancelamento testados. O ciclo de vida da sessão da 001 é consultado
      (bloqueio durante captura), não alterado.
- [x] **IV. Security & privacy** ✅: threat model abaixo.
  - Capability da webview inalterada (`core:default`); o updater não é exposto à webview.
  - Entradas IPC novas validadas (`deny_unknown_fields`, enums fechados, limites).
  - Segredos de assinatura em environment protegido; chave OpenPGP fora do CI.
  - CSP mais restrita.
  - Verificação de atualização: sem conteúdo de sessão, informada na primeira execução, desligável.
  - Ver o mapeamento de Art. IV.15–16 abaixo.
- [x] **V. Cross-platform audio** ✅: o áudio não muda. A matriz da 001 roda em hardware real em cada SO
      (Art. V.5). O S1 segue o protocolo do ADR-002 e não leva código ao release.
- [x] **VI. AI & human control** ✅: a análise não muda. A avaliação com provedor real usa os harnesses da 001
      e fixtures não confidenciais. A atualização exige confirmação humana: nenhuma instalação autônoma.
- [x] **VII. Monorepo** ✅:
  - Os comandos novos em `src-tauri/src/commands/updates.rs` e `diagnostics.rs` são finos.
  - `tooling/perf-sampler` é membro do Cargo workspace com `publish = false`.
  - As tarefas de release não são cacheadas pelo Turbo.
  - Cada dependência nova tem uma justificativa (Technical Context) e passa por `cargo deny`/`pnpm audit`.
- [x] **VIII. Testing/DoD** ✅: critérios testáveis na spec (SC-001…SC-015). Testes unitários do
      `UpdateService` e testes de contrato do adaptador com um manifesto/servidor falso (`wiremock`). Os casos
      do Art. IV.16 são reexecutados no build assinado. A matriz manual é obrigatória (VIII.4).
- [x] **IX. Docs** ✅: ADR-005 (distribuição, assinatura e atualização) é novo. ADR-002, ADR-003 e ADR-004 são
      atualizados com resultados reais. READMEs e notas de release declaram só o que foi validado (FR-032).

**Resultado**: nenhuma violação. Complexity Tracking não se aplica.

### Threat model (Art. IV.10)

| Fronteira | Ativo | Ameaça | Mitigação | Verificação |
|-----------|-------|--------|-----------|-------------|
| Canal de distribuição → usuário | artefatos | artefato adulterado ou trocado | assinatura da plataforma + `SHA256SUMS.asc` + atestação | US1 I1/I3; SC-002 |
| Fonte de atualização → app | binário instalado | atualização maliciosa, downgrade | minisign com chave embutida; só versões maiores; HTTPS | U2; SC-014 |
| Fonte de atualização → app | privacidade | rastreamento ou vazamento pela verificação | GET sem corpo e sem versão na URL; aviso com destinos; desligável | U3; teste com proxy |
| Pipeline → segredos | chaves de assinatura | vazamento em log, PR malicioso | environment `release` com aprovação; sem eco; gitleaks; OpenPGP fora do CI | ensaio §3 do quickstart; gitleaks |
| Webview → núcleo | IPC | entrada forjada em `update_install`/relatório de CSP | `deny_unknown_fields`; checagem de estado e versão no núcleo; relatório limitado e sem conteúdo | testes de comando |
| Webview | indicador de captura | CSS injetado ocultando ou falsificando o estado | `style-src 'self'` sem inline; nenhum HTML cru | `csp_policy`; C1 |
| Manifesto → UI | notas de atualização | conteúdo malicioso nas notas | texto puro, ≤ 4 000 caracteres, nunca renderizado como HTML | teste de componente |
| Atualização × sessão | dados de sessão | instalação interrompendo a captura e corrompendo dados | bloqueio em sessão ativa, checado duas vezes | U4; testes do `UpdateService` |

### Mapeamento de segurança (Art. IV.15–16)

| Exigência | Onde é atendida |
|-----------|-----------------|
| `cargo audit`, lint/test, auditoria JS, varredura de segredos no pipeline de release | `release.yml` reexecuta os gates da CI (R6) |
| Artefatos rastreáveis | atestações de proveniência + `candidate.md` (R4, R10) |
| Assinatura da plataforma e atualização verificada | R2, R3, R4, R5 |
| Permissão negada, início/parada, redação de logs, exclusão, prompt injection | casos 1, 2, 6, 10 e redação (T157 estendido) reexecutados no build assinado |
| Validação de IPC e rejeição de acesso não autorizado | testes dos comandos novos; capability inalterada |

## Project Structure

### Documentation (this feature)

```text
specs/002-desktop-release-readiness/
├── plan.md              # este arquivo
├── research.md          # Fase 0
├── data-model.md        # Fase 1
├── quickstart.md        # Fase 1
├── contracts/
│   ├── ipc-updates.md       # comandos/eventos de atualização e relatório de CSP
│   ├── csp-policy.md        # CSP de produção e destinos de rede novos
│   ├── release-artifacts.md # artefatos, verificação, manifesto, chaves
│   └── release-evidence.md  # formatos de evidência, plano de medição, checklist, decisão
├── checklists/requirements.md
└── tasks.md             # Fase 2 (/speckit.tasks)
```

### Source Code (repository root)

```text
.github/workflows/
├── ci.yml                         # + teste csp_policy (já coberto por cargo test) e test:dist
└── release.yml                    # NOVO: build/assinatura/notarização/rascunho (R6)

apps/desktop/
├── src/
│   ├── main.tsx                   # + listener securitypolicyviolation → diagnostics_report_csp_violation
│   ├── features/settings/         # + seção "Atualizações" (toggle, verificar agora, última verificação)
│   ├── features/updates/          # NOVO: aviso de primeira execução, banner de versão disponível, diálogo de confirmação
│   ├── ipc/                       # + cliente tipado dos comandos e do canal update_subscribe
│   └── i18n/{pt-BR,en}.json       # + textos de atualização
├── scripts/check-dist.mjs         # NOVO: dist sem <style>/style= (script `test:dist`)
├── eslint.config.*                # + forbid-dom-props(style), no-danger
└── src-tauri/
    ├── tauri.conf.json            # CSP estrita; bundle por SO; plugins.updater; createUpdaterArtifacts
    ├── Entitlements.plist         # NOVO: com.apple.security.device.audio-input
    ├── Cargo.toml                 # + tauri-plugin-updater
    ├── src/updater.rs             # NOVO: TauriUpdater (implementa AppUpdater)
    ├── src/commands/updates.rs    # NOVO: update_* (finos)
    ├── src/commands/diagnostics.rs# NOVO: diagnostics_report_csp_violation (limitado)
    ├── src/composition.rs         # wiring do UpdateService + agendamento 24 h; CSV de métricas sob DEVCOPILOT_METRICS (retenção de 7 dias)
    └── tests/
        ├── csp_policy.rs          # NOVO
        ├── version_consistency.rs # NOVO
        └── log_redaction.rs       # estendido: CSV de métricas sem texto

crates/application/src/
├── ports.rs                       # + trait AppUpdater, AvailableUpdate
├── update_service.rs              # NOVO: política e máquina de estados
├── settings_service.rs            # + updates.check_enabled / notice_acknowledged / last_checked_at
└── transcription.rs               # + gancho de métricas (instantes, sem texto) atrás de uma porta MetricsSink opcional

crates/application/tests/update_service.rs   # NOVO

tooling/perf-sampler/              # NOVO: crate binário (sysinfo), publish = false

docs/
├── adr/ADR-005.md                 # NOVO: distribuição, assinatura e atualização
├── adr/ADR-002.md, ADR-003.md, ADR-004.md   # atualizados com resultados reais
└── release/
    ├── checklist.md, rollback.md, signing-and-distribution.md, verify-downloads.md   # NOVOS (permanentes)
    └── v0.1.0/                    # NOVO: candidate, measurement-plan, measurements, evidence/<plataforma>.md, checklist, decision

specs/001-meeting-sessions/security-review.md   # disposição final de F-1, F-4, F-6 + nova aprovação
README.md, apps/desktop/README.md               # plataformas suportadas, verificação, limitações
```

**Structure Decision**: a estrutura existente do monorepo é mantida. O único diretório de código novo fora do
app é `tooling/perf-sampler`, uma ferramenta de medição que não é distribuída. Ela fica em `tooling/` por ser
ferramenta de desenvolvimento, sem consumidores no app e sem lugar natural em `crates/` ou `packages/`
(Art. VII.3); a justificativa também vai para o ADR-005. A lógica de atualização segue a
divisão porta/caso de uso/adaptador da 001. A documentação de release que vale para todos os releases fica em
`docs/release/`, e as evidências do 0.1.0 em `docs/release/v0.1.0/`.

## Fases de execução sugeridas (insumo para `/speckit.tasks`)

| Ordem | Bloco | Depende de | Fecha |
|-------|-------|------------|-------|
| 0 | Aquisição das identidades (Apple, Windows), chave minisign e chave OpenPGP | — (começar no dia 1; prazo externo) | pré-requisito de US1 e US8 |
| 1 | CSP estrita + proteções + relatório de violação, verificado nos 3 SOs com build local | — | F-1 (código) |
| 2 | `UpdateService`, adaptador, comandos e UI, com testes | 1 (CSP já estrita na UI nova) | US8 (código) |
| 3 | `release.yml`, configuração de bundle por SO, entitlements, ensaio no canal de teste | 0, 2 | F-4 (código) |
| 4 | Modo de métricas + `perf-sampler` + plano de medição aprovado | — | pré-requisito de US5 |
| 5 | Build candidato `v0.1.0-rc.1` assinado | 3 | — |
| 6 | Matriz + instalação + atualização + acessibilidade por plataforma | 5 | F-6, US1, US3, US4, US8 |
| 7 | Medições e avaliação com provedor real | 4, 5 | US5 |
| 8 | Spike S1 (em paralelo, hardware permitindo) e ADR-002 | — | US7 |
| 9 | Correção de bloqueadores → novo `rc.N` → revalidação do que mudou | 6, 7 | — |
| 10 | Docs de release, rollback ensaiado, revisão de segurança, checklist e decisão | 6–9 | US6 |

## Riscos de planejamento

Os riscos de produto estão na spec. Os de plano:

- **Prazo das identidades de assinatura** (bloco 0) é o caminho crítico. Os blocos 1, 2 e 4 não dependem dele
  e devem andar em paralelo. O bloco 3 pode ser ensaiado com identidades de teste (certificado autoassinado
  no Windows, ad hoc no macOS) até as definitivas chegarem.
- **Um `rc.N` novo invalida evidências**: a regra de `release-evidence.md` permite manter uma evidência quando
  a diferença entre commits comprovadamente não afeta o caso. Mesmo assim, correções tardias custam rodadas
  de matriz. Por isso a matriz começa pelos casos de maior risco: instalação, permissão e CSP.
- **Hardware**: sem uma máquina mínima de algum SO, a métrica correspondente fica como "não medida" e a
  plataforma não pode declarar o perfil mínimo. Isso não bloqueia o release, mas aparece nas limitações.

## Complexity Tracking

Não se aplica: o Constitution Check não tem violações.
