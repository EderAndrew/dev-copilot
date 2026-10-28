# Contract: Comandos IPC (frontend → núcleo)

Comandos Tauri expostos à janela `main`. Todos são **adaptadores finos** (Art. VII.4): desserializam o DTO,
validam, chamam um caso de uso de `application` e mapeiam o resultado. A autorização é verificada no núcleo,
independente do estado do frontend (Art. IV.11).

`SessionDto`: `{ id, title, language, state, failure?, sources, createdAt, startedAt?, endedAt?,
activeDurationMs }` (instantes em ms desde a época Unix). `AudioSourceDto`: `{ kind, availability:
"available" | "unavailable", unavailableReason?, permission }`. Exemplos em `fixtures/`.

Convenções:
- Nomes em `snake_case`; payloads JSON em `camelCase`.
- A entrada de cada comando é passada como um único argumento `request` (`invoke(cmd, { request })`);
  comandos sem entrada não recebem argumentos. Campos desconhecidos → `invalid_input`.
- Ids são strings UUID; ids desconhecidos → `not_found`.
- Strings são limitadas conforme [data-model.md](../data-model.md); excesso → `invalid_input`.
- Erros seguem `IpcError` e nunca contêm conteúdo de sessão, caminhos internos ou segredos.

```ts
type IpcError = {
  code:
    | "invalid_input" | "not_found" | "invalid_state" | "permission_denied"
    | "source_unavailable" | "no_transcriber" | "secret_store_unavailable"
    | "authorization_required" | "provider_error" | "storage_error" | "internal";
  messageKey: string;          // chave i18n, ex.: "errors.permissionDenied"
  details?: Record<string, string>; // somente dados não sensíveis (ex.: { source: "microphone" })
};
```

`authorization_required` traz `details.providerId` quando o núcleo sabe qual provedor remoto precisa de
autorização (ex.: `session_start` com transcritor remoto padrão ainda não autorizado); a UI abre então o
diálogo de autorização (US7.2).

## Sessões

| Comando | Entrada | Saída | Erros específicos |
|---------|---------|-------|-------------------|
| `session_create` | `{ title, language, sources: ("microphone"\|"system")[] }` | `SessionDto` | `invalid_input`, `source_unavailable` |
| `session_start` | `{ sessionId, consentAcknowledged: boolean }` | `SessionDto` (estado `starting`) | `invalid_state`, `permission_denied`, `no_transcriber`, `authorization_required` (com `providerId`), `secret_store_unavailable`, `invalid_input` se lembrete exigido e não confirmado (FR-012) |
| `session_cancel_start` | `{ sessionId }` | `SessionDto` (estado `idle`) — fecha qualquer fonte já aberta (FR-003) | `invalid_state` se não estiver em `starting` |
| `session_pause` | `{ sessionId }` | `SessionDto` (estado `paused`) — todas as fontes fechadas antes de retornar; indicador `paused`; o áudio captado antes da pausa termina de ser transcrito em segundo plano | `invalid_state` (fora de `capturing`, ou retomada ainda abrindo as fontes) |
| `session_resume` | `{ sessionId }` | `SessionDto` (estado `capturing`) — fontes reabrem em segundo plano depois de gravada a lacuna `paused`; fonte que ficou indisponível ou sem permissão durante a pausa é informada por `source_status` e não é retomada (US9.4) | `invalid_state`, `permission_denied` / `source_unavailable` (nenhuma fonte utilizável; a sessão continua `paused`), `no_transcriber`, `authorization_required`, `secret_store_unavailable` |
| `session_stop` | `{ sessionId }` | `SessionDto` (estado `stopping`) — retorna sem aguardar drenagem | `invalid_state` |
| `session_list` | `{ cursor?: string, limit?: 1..100 }` (padrão 50; `cursor` = último id da página anterior) | `{ items: SessionDto[], nextCursor? }`, mais recentes primeiro | `invalid_input` (limite fora da faixa, cursor desconhecido) |
| `session_get` | `{ sessionId }` | `SessionDetailDto` = `{ session: SessionDto, transcript: TranscriptItemDto[], records: RecordDto[], questions: QuestionDto[] }`; `TranscriptItemDto` = `{ type: "final", segment: SegmentDto }` \| `{ type: "gap", reason, startMs, durationMs }` em ordem de `seq`; perguntas em ordem de envio | `not_found` |
| `session_rename` | `{ sessionId, title }` | `SessionDto` | `invalid_input` |
| `session_delete` | `{ sessionId, confirmed: true }` | `{}`; análises e pergunta em andamento da sessão são canceladas e descartadas antes da remoção | `invalid_input` (`confirmed` ≠ `true`), `not_found`, `invalid_state` (sessão ativa — inclusive `starting`/`stopping` —, FR-044) |
| `session_subscribe` | `{ sessionId }` + argumento de topo `channel: Channel<SessionEvent>` (Tauri só aceita `Channel` como argumento de comando, não dentro de `request`) | `SessionDto` atual (para ressincronizar ao reconectar) | `not_found` |

## Fontes e permissões

| Comando | Entrada | Saída |
|---------|---------|-------|
| `audio_sources_list` | `{}` | `AudioSourceDto[]` com `availability` e `permission` (FR-008) |
| `audio_permission_request` | `{ kind }` | `PermissionState` |
| `audio_permission_help` | `{ kind }` | `{ messageKey }` — orientação por SO (FR-010) |

## Registros e plano

`RecordDto`: `{ id, sessionId, kind, text, nature, status, rationale, origin: "ai" | "user", providerId?,
analysisRunId?, evidence: string[] (ids de trechos), acceptanceCriteria: string[], alternatives: string[],
conflictsWith?, duplicateOf?, createdAt, updatedAt }`. Evidências são ids de trechos persistidos; o texto citado
vem sempre do trecho original (FR-053b). Exemplos em `fixtures/RecordDto/`.

`PlanDto`: `{ sessionId, sections: { kind, items: RecordDto[] }[] }` — uma seção por tipo, sempre na ordem
`task`, `requirement`, `decision`, `assumption`, `risk`, `open_question` (vazias incluídas); itens por status
(`confirmed`, `pending`, `proposed`, `rejected`) e data de criação. Registros do usuário são criados com
`nature = suggested` e identificados por `origin = "user"`.

| Comando | Entrada | Saída | Regras |
|---------|---------|-------|--------|
| `record_set_status` | `{ recordId, status }` | `RecordDto` | ator sempre `user`; transições do data-model |
| `record_edit` | `{ recordId, text?, rationale?, acceptanceCriteria?, alternatives? }` | `RecordDto` | gera um `RecordChange` por campo alterado; status não muda; edição vazia → `invalid_input`; critérios só em `task`, alternativas só em `decision` |
| `record_create` | `{ sessionId, kind, text, rationale?, status }` | `RecordDto` | `origin = user` |
| `plan_get` | `{ sessionId }` | `PlanDto` | visão derivada |
| `analysis_run_final` | `{ sessionId }` | `{ runId }` — resultado por `analysis_status`/`record_proposed` | só `ended`/`failed` e uma por vez (`invalid_state`, FR-033); `provider_error` sem provedor de análise utilizável; `authorization_required` se provedor remoto sem autorização |

## Perguntas

`QuestionDto`: `{ id, sessionId, question, answer?, certainty?: "supported" | "partial" | "no_evidence",
evidence: ({ type: "segment", id } | { type: "record", id })[], status: "pending" | "answered" | "failed",
providerId?, askedAt, answeredAt? }`. `answer` e `certainty` só existem com `status = answered`;
`no_evidence` ⇔ `evidence` vazio. Evidências são ids da própria sessão (as demais são descartadas pelo
núcleo, SC-011); o texto citado vem do trecho ou registro armazenado, nunca traduzido (FR-053b). Exemplos em
`fixtures/QuestionDto/`. A resposta do provedor segue
[answer-output.schema.json](./answer-output.schema.json).

| Comando | Entrada | Saída | Erros específicos |
|---------|---------|-------|-------------------|
| `question_ask` | `{ sessionId, question }` (1–2 000 caracteres) | `QuestionDto` com `status = pending` — a resposta (ou a falha) chega pelo evento `question_answered`; captura e transcrição seguem sem esperar (US5.4) | `invalid_state` (sessão nunca iniciada, ou já há uma pergunta pendente dela), `provider_error` (sem provedor de análise utilizável) |
| `question_to_record` | `{ questionId, kind }` | `RecordDto` novo com `status = proposed`, `nature = suggested`, `origin = user`, a pergunta como justificativa e os trechos citados como evidência; nenhum registro existente muda (FR-037) | `invalid_state` (pergunta não respondida) |

## Exportação

| Comando | Entrada | Saída | Regras |
|---------|---------|-------|--------|
| `export_session` | `{ sessionId, formats: ("markdown"\|"json")[], content: ("plan"\|"transcript")[] }` | `{ written: string[] }` (nomes de arquivo, sem diretório) | o núcleo abre um diálogo de salvar por formato e escreve somente nos caminhos escolhidos; cancelar qualquer diálogo → `{ written: [] }` e nada gravado; listas vazias → `invalid_input`; mesmo caminho para dois formatos → `invalid_input` (`path`); falha de escrita → `storage_error` sem o caminho |

## Provedores e autorização

`ProviderDto`: `{ id, role, adapter, locality, endpoint?, model, dataCategories, isDefault, authorization:
"not_required" | "configured" | "authorized" | "revoked", hasCredential }` — nunca contém o segredo;
`hasCredential` só indica que há chave no cofre do SO. `ProviderAuthorizationDto`: `{ providerId, state:
"granted" | "revoked", destination, dataCategories, retentionNotice, grantedAt, revokedAt? }`. Exemplos em
`fixtures/ProviderDto/`, `fixtures/AuthorizationPreviewDto/`, `fixtures/ProviderAuthorizationDto/`.

Resolução do provedor de cada etapa: o padrão, se utilizável (local, ou remoto com autorização vigente que cubra
destino e categorias); senão apenas um provedor local; um remoto diferente do padrão só é escolhido quando não há
padrão definido. Nunca há troca automática para outro provedor remoto (FR-048).

| Comando | Entrada | Saída | Regras |
|---------|---------|-------|--------|
| `provider_list` | `{}` | `ProviderDto[]` (sem segredos) | |
| `provider_upsert` | `{ id?, role, adapter, endpoint?, model, dataCategories }` | `ProviderDto` | remoto exige `https`; local exige loopback; alterar adaptador, endpoint ou categorias revoga a autorização vigente; a chave guardada é mantida |
| `provider_set_secret` | `{ providerId, secret }` (≤ 8 KiB, sem caracteres de controle) | `{}` | só provedores remotos; grava no cofre do SO; segredo nunca retorna; `secret_store_unavailable` sem gravar em outro lugar |
| `provider_authorization_preview` | `{ providerId }` | `{ providerId, destination, dataCategories, retentionNotice }` | texto exibido antes do aceite, no idioma da UI; `invalid_input` para provedor local |
| `provider_authorize` | `{ providerId, accepted: true }` | `ProviderAuthorizationDto` | grava exatamente o que a prévia mostra; `accepted ≠ true` → `invalid_input` |
| `provider_revoke` | `{ providerId }` | `ProviderAuthorizationDto` | envios novos cessam imediatamente (FR-047); `invalid_state` se não autorizado |
| `provider_set_default` | `{ role, providerId }` | `{}` | |
| `local_model_list` | `{}` | `LocalModelDto[]` — `{ id, displayName, origin, sizeBytes, downloaded, downloading }` | catálogo fixo (ADR-003); não faz requisição de rede |
| `local_model_download` | `{ modelId }` + argumento de topo `channel: Channel<ModelDownloadEvent>` | `{ jobId }` | ação explícita; URL (revisão fixa) e SHA-256 fixos no app; `GET` sem dados de sessão; `invalid_input` fora do catálogo, `invalid_state` se já baixando; progresso pelo canal: `{ type: "progress", jobId, receivedBytes, totalBytes }` \| `{ type: "completed", jobId }` \| `{ type: "failed", jobId, messageKey }` |

## Configurações

| Comando | Entrada | Saída |
|---------|---------|-------|
| `settings_get` | `{}` | `SettingsDto` |
| `settings_update` | `Partial<SettingsDto>` | `SettingsDto` |
