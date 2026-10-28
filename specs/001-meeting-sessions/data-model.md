# Data Model: Sessões de reunião e assistência técnica incremental

**Feature**: `001-meeting-sessions` | **Date**: 2026-10-27 | **Spec**: [spec.md](./spec.md)

Entidades do crate `domain` (Rust puro, sem dependências de infraestrutura — Art. II.2). Identificadores são
UUID v7 (ordenáveis por tempo) encapsulados em newtypes (`SessionId`, `RecordId`, ...). Datas em UTC.
Textos de conteúdo são considerados sensíveis: nunca aparecem em logs (FR-051) e são persistidos cifrados
(FR-039a).

## Session

| Campo | Tipo | Regras |
|-------|------|--------|
| id | SessionId | imutável |
| title | String | 1–200 caracteres, sem caracteres de controle; editável |
| language | SessionLanguage | `pt-BR` \| `en` (extensível); define idioma de transcrição e de conteúdo gerado (FR-053b) |
| state | SessionState | ver máquina de estados |
| sources | Vec<AudioSourceKind> | 1–2 itens, sem repetição; definido antes de iniciar |
| created_at | Timestamp | |
| started_at / ended_at | Option<Timestamp> | `ended_at` ≥ `started_at` |
| active_duration | Duration | soma dos intervalos em `capturing` (exclui pausas) |
| failure | Option<FailureReason> | presente sse `state = failed` |

**FailureReason**: `permission_denied`, `source_unavailable`, `no_transcriber`, `storage_error`,
`secret_store_unavailable`, `interrupted`, `internal`.

### Máquina de estados (FR-003, FR-006a, FR-007)

```text
idle ──start──▶ starting ──ready──▶ capturing ──stop──▶ stopping ──drained──▶ ended
  ▲               │  │                 │  ▲                 
  └──cancel───────┘  └─fail─▶ failed   │  │                 
                                 pause │  │ resume (fontes/permissões válidas)
                                       ▼  │                 
                                      paused ──stop──▶ stopping
capturing ─fail─▶ failed      paused ─fail─▶ failed      (startup) starting|capturing|paused|stopping ─▶ failed(interrupted)
```

- Transições fora desta tabela retornam `DomainError::InvalidTransition` (testado exaustivamente).
- `ended` e `failed` são terminais para captura; permitem consulta, perguntas, análise final (FR-033),
  exportação e exclusão.
- Invariante global (aplicação): no máximo uma sessão em `starting|capturing|paused|stopping` (FR-006).
- `start` exige: pelo menos uma fonte com permissão concedida, provedor de transcrição utilizável (FR-045a) e
  cofre de segredos disponível (FR-039a).

## AudioSource

| Campo | Tipo | Regras |
|-------|------|--------|
| kind | AudioSourceKind | `microphone` \| `system` |
| device_id | Option<String> | id opaco do dispositivo; sem troca silenciosa (FR-011) |
| availability | Availability | `available` \| `unavailable{reason: UnavailableReason}` |
| permission | PermissionState | `not_requested` \| `granted` \| `denied` \| `revoked` |

**UnavailableReason**: `platform_unsupported`, `not_validated_on_platform`, `no_device`, `os_version`.
Estado de permissão não é persistido como verdade: é consultado ao SO a cada início/retomada.

## TranscriptSegment

| Campo | Tipo | Regras |
|-------|------|--------|
| id | SegmentId | |
| session_id | SessionId | |
| seq | u64 | monotônico por sessão |
| kind | SegmentKind | `final{text}` \| `gap{reason: GapReason, duration}` |
| source | Option<AudioSourceKind> | obrigatório quando a sessão tem 2 fontes (FR-016) |
| start_offset / end_offset | Duration | relativos ao início da sessão; `end ≥ start` |

- **GapReason**: `paused`, `overload`, `provider_failure`, `cancelled_on_stop`, `source_lost`, `interrupted`
  (anexado na recuperação de inicialização após o último trecho final persistido, US2.3).
- Segmentos **parciais** existem apenas em memória e em eventos de UI; nunca são persistidos (FR-017).
- Texto final: 1–10 000 caracteres.

## TechnicalRecord

| Campo | Tipo | Regras |
|-------|------|--------|
| id | RecordId | |
| session_id | SessionId | |
| kind | RecordKind | `requirement` \| `decision` \| `risk` \| `open_question` \| `task` \| `assumption` |
| text | String | 1–2 000 caracteres |
| nature | Nature | `observed` \| `inferred` \| `suggested` |
| status | RecordStatus | `proposed` \| `confirmed` \| `rejected` \| `pending` |
| rationale | String | 0–2 000 caracteres |
| origin | Origin | `ai{provider_id, analysis_run_id}` \| `user` |
| evidence | Vec<SegmentId> | `nature = observed` ⇒ não vazio e todos da mesma sessão (FR-023) |
| acceptance_criteria | Vec<String> | apenas `task` |
| alternatives | Vec<String> | apenas `decision` |
| flags | RecordFlags | `conflicts_with: Option<RecordId>`, `duplicate_of: Option<RecordId>` |
| created_at / updated_at | Timestamp | |

`assumption` atende Art. VI.4 (premissas no plano).

### Transições de status (FR-024, FR-025)

```text
proposed ─▶ confirmed | rejected | pending
pending  ─▶ confirmed | rejected
confirmed ─▶ pending | rejected       (somente ação do usuário — revisão explícita)
rejected ─▶ pending | confirmed       (somente ação do usuário)
```

- Registros com `origin = ai` nascem `proposed`; registros `user` podem nascer `confirmed`.
- Nenhuma operação originada de IA pode alterar `text` ou `status` de registro existente: a camada de
  aplicação só expõe `propose_from_analysis` (cria) e `apply_user_change` (altera), e o domínio rejeita
  `RecordChange` com `actor = ai` sobre registros existentes.

## RecordChange (histórico, FR-026)

| Campo | Tipo |
|-------|------|
| id | ChangeId |
| record_id | RecordId |
| actor | `user` \| `ai{analysis_run_id}` (este último só para criação) |
| field | `created` \| `text` \| `status` \| `rationale` \| `acceptance_criteria` \| `alternatives` |
| old_value / new_value | Option<String> |
| at | Timestamp |

Append-only.

## AnalysisRun

| Campo | Tipo | Regras |
|-------|------|--------|
| id | AnalysisRunId | |
| session_id | SessionId | |
| provider_id | ProviderId | |
| trigger | `incremental` \| `final` | `final` só com sessão `ended`/`failed` (FR-033) |
| segment_range | (u64, u64) | seq dos trechos analisados |
| outcome | `ok{created: u32}` \| `invalid_output` \| `provider_error` \| `rate_limited` | |
| started_at / finished_at | Timestamp | |

Permite limitar custo/frequência (FR-028) e rastrear proveniência.

## DevelopmentPlan

Visão derivada (não armazenada separadamente): registros da sessão agrupados por `kind`, ordenados por status
(`confirmed`, `pending`, `proposed`, `rejected`) e data. Itens de usuário marcados (FR-032).

## ContextQuestion (FR-034–FR-038)

| Campo | Tipo | Regras |
|-------|------|--------|
| id | QuestionId | |
| session_id | SessionId | |
| question | String | 1–2 000 caracteres |
| answer | Option<String> | 1–4 000 caracteres; ausente enquanto pendente ou em falha |
| evidence | Vec<EvidenceRef> | `segment(SegmentId)` \| `record(RecordId)` da mesma sessão |
| certainty | `supported` \| `partial` \| `no_evidence` | `no_evidence` ⇒ `evidence` vazio |
| status | `pending` \| `answered` \| `failed` | |
| provider_id | Option<ProviderId> | |
| asked_at / answered_at | Timestamp | |

Referências de evidência devolvidas pelo modelo que não existam na sessão são descartadas; se nenhuma
restar, `certainty` é rebaixada para `no_evidence` (SC-011).

## Provider e ProviderAuthorization

| Campo (Provider) | Tipo | Regras |
|------------------|------|--------|
| id | ProviderId | |
| role | `transcription` \| `analysis` | analysis cobre também perguntas |
| locality | `local` \| `remote` | |
| adapter | ProviderAdapterKind | ex.: `whisper_local`, `openai_compatible_stt`, `anthropic`, `openai_compatible_chat`, `ollama` |
| endpoint | Option<Url> | obrigatório para remotos; `https` obrigatório para remotos |
| model | String | |
| credential_ref | Option<CredentialRef> | chave no cofre do SO; nunca o segredo |
| data_categories | Set<DataCategory> | `audio`, `transcript`, `records`, `questions` |

| Campo (ProviderAuthorization) | Tipo | Regras |
|-------------------------------|------|--------|
| provider_id | ProviderId | |
| state | `granted` \| `revoked` | |
| destination | String | host exibido ao usuário |
| data_categories | Set<DataCategory> | snapshot do que foi autorizado |
| retention_notice | String | texto exibido |
| granted_at / revoked_at | Timestamp | |

Regra: requisição remota permitida sse existe autorização `granted` cujo `destination` e `data_categories`
cobrem a requisição. Mudança de endpoint ou categorias invalida a autorização (nova autorização exigida).

## ExportRecord

| Campo | Tipo |
|-------|------|
| id | ExportId |
| session_id | SessionId |
| formats | Set<`markdown` \| `json`> |
| content | Set<`plan` \| `transcript`> |
| at | Timestamp |

O caminho do arquivo não é armazenado (pode revelar informação e fica obsoleto).

## AppSettings

`ui_language` (`pt-BR` \| `en`), `consent_reminder_dismissed` (bool), `analysis_limits`
(`min_interval_secs`, `max_calls_per_hour`, `max_tokens_per_session`), provedores padrão por papel.

## Persistência (resumo para `persistence`)

Banco SQLite cifrado (SQLCipher, R8). Tabelas: `sessions`, `session_sources`, `segments`, `records`,
`record_evidence`, `record_changes`, `analysis_runs`, `questions`, `question_evidence`, `providers`,
`provider_authorizations`, `exports`, `settings`, `schema_migrations` (via `user_version`).
Exclusão de sessão: `ON DELETE CASCADE` a partir de `sessions`, em transação, seguida de checkpoint do WAL.
