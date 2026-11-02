# Data Model — 002 Desktop Release Readiness

A feature tem dois tipos de dado:

1. **Dados do app**: o estado da atualização e três chaves novas de configuração. Não há tabela nova nem
   migração: as configurações da 001 já são chave/valor (`settings`, `crates/persistence/src/migrations/v1.sql`).
2. **Dados de processo de release**: documentos versionados em `docs/release/` (formatos em
   `contracts/release-evidence.md`). Não são lidos pelo app.

## 1. Dados do app

### 1.1 Configurações novas (`AppSettings`, chave/valor, não sensíveis)

| Chave | Tipo | Padrão | Validação / regra |
|-------|------|--------|-------------------|
| `updates.check_enabled` | bool | `true` | Desligada: o `UpdateService` não faz nenhuma requisição (FR-008b) |
| `updates.notice_acknowledged` | bool | `false` | Fica `true` quando o usuário dispensa o aviso de primeira execução. Enquanto for `false`, nenhuma verificação acontece |
| `updates.last_checked_at` | timestamp UTC (RFC 3339) | ausente | Só informativo (exibido nas Configurações). Valor inválido é tratado como ausente, como as outras chaves (`settings_service.rs`) |

Valores inválidos voltam ao padrão, como na 001: uma linha danificada nunca bloqueia o app. Nenhuma dessas
chaves guarda dado pessoal ou de sessão.

### 1.2 `AvailableUpdate` (application, valor em memória, nunca persistido)

| Campo | Tipo | Regra |
|-------|------|-------|
| `version` | texto semver | Maior que a versão instalada (garantido pelo adaptador/plugin e checado de novo no caso de uso) |
| `notes` | texto, ≤ 4 000 caracteres | Vem do manifesto. É dado não confiável: exibido como texto puro, nunca como HTML ou Markdown renderizado |
| `published_at` | timestamp opcional | Informativo |

Tipos do plugin não passam da porta `AppUpdater`. O adaptador converte para `AvailableUpdate`.

### 1.3 Máquina de estados de atualização (`UpdateStatus`, application)

```text
                 ┌──────────── check desligado / aviso pendente ───────────┐
                 ▼                                                          │
  Disabled ◀── (setting off)            Idle ──check()──▶ Checking ─────────┘
                                         ▲                 │
                         (24 h / manual) │                 ├─▶ UpToDate ──▶ Idle
                                         │                 ├─▶ Failed{reason} ──▶ Idle
                                         │                 └─▶ Available{update}
                                         │                          │ install(version) com confirmação
                                         │                          ▼
                                         │          BlockedBySession ◀── sessão ativa
                                         │                          │ (sessão parou → volta a Available)
                                         │                          ▼
                                         │                    Downloading{progress} ──cancel/falha──▶ Failed
                                         │                          ▼
                                         └────────────────── Installing ──▶ (reinício do app)
```

Regras:

- `install(version)` só é aceito em `Available` e se `version` for igual à da última verificação. Isso impede
  que uma confirmação valha para outro pacote.
- `BlockedBySession` é avaliado na hora da confirmação e de novo imediatamente antes de `Installing`. Sessões
  em `starting`, `capturing`, `paused` ou `stopping` bloqueiam.
- Só uma operação por vez (`check` ou `install`). Uma chamada concorrente recebe `invalid_state`.
- `Failed.reason` ∈ {`network`, `signature_invalid`, `manifest_invalid`, `download_interrupted`,
  `install_failed`}. Nenhuma falha altera a versão instalada (FR-008c).
- Desligar `updates.check_enabled` cancela um `Checking` ou `Downloading` em curso e leva a `Disabled`.

### 1.4 Relatório de violação de CSP (`CspViolationReport`, entrada de IPC, não persistido)

| Campo | Tipo | Validação |
|-------|------|-----------|
| `effectiveDirective` | enum fechado (`style-src`, `style-src-elem`, `style-src-attr`, `script-src`, `script-src-elem`, `img-src`, `font-src`, `connect-src`, `default-src`, `other`) | Valor desconhecido → `other` |
| `blockedScheme` | texto ≤ 16, `[a-z][a-z0-9+.-]*` ou `inline`/`eval` | Recusado se não casar |
| `disposition` | `enforce` \| `report` | — |

Sem `sample`, sem URL completa, sem número de linha. O núcleo registra no máximo 20 relatórios por minuto e
descarta o resto contando quantos foram descartados.

## 2. Dados de processo de release (documentos)

Entidades da spec e onde cada uma vive. Os formatos detalhados estão em `contracts/release-evidence.md`.

| Entidade (spec) | Documento | Campos principais | Relações |
|-----------------|-----------|-------------------|----------|
| Build candidato | `docs/release/v0.1.0/candidate.md` | versão, tag, commit, URL do workflow, lista de artefatos com SHA-256, estado de assinatura/notarização por artefato, link das atestações | Referenciado por toda evidência |
| Plano de medição | `docs/release/v0.1.0/measurement-plan.md` | metas, hardware (ref./mín. por SO), método, condições, amostra, data de aprovação, histórico de alterações | Precede os resultados (data menor) |
| Registro de evidência | `docs/release/v0.1.0/evidence/<plataforma>.md` + anexos | caso, resultado (✅/❌/➖), ambiente, executor, data, link do anexo ou problema | Ligado ao build candidato pelo commit |
| Resultados de medição | `docs/release/v0.1.0/measurements.md` + CSVs | valor medido vs. meta por SO/hardware, classificação (ok/limitação/bloqueador) | Ligado ao plano e ao build |
| Problema/bloqueador | Issue no GitHub com rótulos `release-0.1.0`, `severity:*`, `platform:*` (e `release-blocker` se crítico) | severidade, plataforma, estado, decisão | Citado nas evidências |
| Declaração de suporte | seção "Plataformas suportadas e limitações" das notas de release e do README | SO, versão, arquitetura, recursos, limitações | Derivada só das evidências aprovadas |
| Checklist de release | `docs/release/checklist.md` (modelo) → cópia preenchida em `docs/release/v0.1.0/checklist.md` | item, estado, link da evidência | Bloqueia a decisão |
| Decisão de publicação | `docs/release/v0.1.0/decision.md` | decisão (publicar / não publicar), responsável, data, versão, commit, checksums, disposição F-1…F-6, limitações | Referencia o checklist preenchido |
| Plano de rollback | `docs/release/rollback.md` | gatilhos, passos, responsáveis, comunicação, chaves | Ensaiado no canal de teste (SC-013) |

### Estados da decisão de publicação

```text
Rascunho ──(checklist completo, zero release-blocker aberto)──▶ Aprovada ──(rascunho do GitHub publicado)──▶ Publicada
   ▲                                                              │
   └────────────(novo release-blocker ou artefato alterado)───────┘   Publicada ──(gatilho)──▶ Retirada (rollback)
```

Uma aprovação vale para um conjunto exato de checksums. Qualquer artefato reconstruído invalida a aprovação
(FR-031).
