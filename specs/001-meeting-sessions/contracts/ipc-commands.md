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

## Sessões

| Comando | Entrada | Saída | Erros específicos |
|---------|---------|-------|-------------------|
| `session_create` | `{ title, language, sources: ("microphone"\|"system")[] }` | `SessionDto` | `invalid_input`, `source_unavailable` |
| `session_start` | `{ sessionId, consentAcknowledged: boolean }` | `SessionDto` (estado `starting`) | `invalid_state`, `permission_denied`, `no_transcriber`, `secret_store_unavailable`, `invalid_input` se lembrete exigido e não confirmado (FR-012) |
| `session_cancel_start` | `{ sessionId }` | `SessionDto` (estado `idle`) — fecha qualquer fonte já aberta (FR-003) | `invalid_state` se não estiver em `starting` |
| `session_pause` | `{ sessionId }` | `SessionDto` | `invalid_state` |
| `session_resume` | `{ sessionId }` | `SessionDto` | `invalid_state`, `permission_denied`, `source_unavailable`, `no_transcriber` |
| `session_stop` | `{ sessionId }` | `SessionDto` (estado `stopping`) — retorna sem aguardar drenagem | `invalid_state` |
| `session_list` | `{ cursor?: string, limit?: 1..100 }` | `{ items: SessionSummaryDto[], nextCursor? }` | — |
| `session_get` | `{ sessionId }` | `SessionDetailDto` (sessão + segmentos finais + registros + perguntas) | `not_found` |
| `session_rename` | `{ sessionId, title }` | `SessionDto` | `invalid_input` |
| `session_delete` | `{ sessionId, confirmed: true }` | `{}` | `invalid_state` (sessão ativa, FR-044) |
| `session_subscribe` | `{ sessionId }` + argumento de topo `channel: Channel<SessionEvent>` (Tauri só aceita `Channel` como argumento de comando, não dentro de `request`) | `SessionDto` atual (para ressincronizar ao reconectar) | `not_found` |

## Fontes e permissões

| Comando | Entrada | Saída |
|---------|---------|-------|
| `audio_sources_list` | `{}` | `AudioSourceDto[]` com `availability` e `permission` (FR-008) |
| `audio_permission_request` | `{ kind }` | `PermissionState` |
| `audio_permission_help` | `{ kind }` | `{ messageKey }` — orientação por SO (FR-010) |

## Registros e plano

| Comando | Entrada | Saída | Regras |
|---------|---------|-------|--------|
| `record_set_status` | `{ recordId, status }` | `RecordDto` | ator sempre `user`; transições do data-model |
| `record_edit` | `{ recordId, text?, rationale?, acceptanceCriteria?, alternatives? }` | `RecordDto` | gera `RecordChange` |
| `record_create` | `{ sessionId, kind, text, rationale?, status }` | `RecordDto` | `origin = user` |
| `plan_get` | `{ sessionId }` | `PlanDto` | visão derivada |
| `analysis_run_final` | `{ sessionId }` | `{ runId }` | só `ended`/`failed` (FR-033); `authorization_required` se provedor remoto sem autorização |

## Perguntas

| Comando | Entrada | Saída |
|---------|---------|-------|
| `question_ask` | `{ sessionId, question }` | `{ questionId }` — resposta chega por evento |
| `question_to_record` | `{ questionId, kind }` | `RecordDto` com `status = proposed`, `nature = suggested` (FR-037) |

## Exportação

| Comando | Entrada | Saída | Regras |
|---------|---------|-------|--------|
| `export_session` | `{ sessionId, formats: ("markdown"\|"json")[], content: ("plan"\|"transcript")[] }` | `{ written: string[] }` (nomes de arquivo, sem diretório) | o núcleo abre o diálogo de salvar e escreve somente no caminho escolhido; cancelamento → `{ written: [] }` |

## Provedores e autorização

| Comando | Entrada | Saída | Regras |
|---------|---------|-------|--------|
| `provider_list` | `{}` | `ProviderDto[]` (sem segredos) | |
| `provider_upsert` | `{ id?, role, adapter, endpoint?, model, dataCategories }` | `ProviderDto` | remoto exige `https`; alterar endpoint/categorias revoga autorização vigente |
| `provider_set_secret` | `{ providerId, secret }` | `{}` | grava no cofre do SO; segredo nunca retorna |
| `provider_authorization_preview` | `{ providerId }` | `{ destination, dataCategories, retentionNotice }` | texto exibido antes do aceite |
| `provider_authorize` | `{ providerId, accepted: true }` | `ProviderAuthorizationDto` | |
| `provider_revoke` | `{ providerId }` | `ProviderAuthorizationDto` | envios novos cessam imediatamente (FR-047) |
| `provider_set_default` | `{ role, providerId }` | `{}` | |
| `local_model_list` | `{}` | `LocalModelDto[]` — `{ id, displayName, origin, sizeBytes, downloaded, downloading }` | catálogo fixo (ADR-003); não faz requisição de rede |
| `local_model_download` | `{ modelId }` + argumento de topo `channel: Channel<ModelDownloadEvent>` | `{ jobId }` | ação explícita; URL (revisão fixa) e SHA-256 fixos no app; `GET` sem dados de sessão; `invalid_input` fora do catálogo, `invalid_state` se já baixando; progresso pelo canal: `{ type: "progress", jobId, receivedBytes, totalBytes }` \| `{ type: "completed", jobId }` \| `{ type: "failed", jobId, messageKey }` |

## Configurações

| Comando | Entrada | Saída |
|---------|---------|-------|
| `settings_get` | `{}` | `SettingsDto` |
| `settings_update` | `Partial<SettingsDto>` | `SettingsDto` |
