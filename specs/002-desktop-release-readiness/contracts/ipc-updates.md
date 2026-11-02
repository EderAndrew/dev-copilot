# Contract: comandos e eventos de atualização e diagnóstico de CSP

Este contrato estende `specs/001-meeting-sessions/contracts/ipc-commands.md` e `ipc-events.md`. As regras
gerais da 001 continuam valendo: DTOs com `deny_unknown_fields`, `camelCase`, erros como `ErrorDto`
(`{ code, messageKey }`) e autorização decidida no núcleo, independentemente do estado do frontend
(Art. IV.11).

A capability da webview **não muda** (`core:default`). Nenhuma permissão do `tauri-plugin-updater` é dada à
webview, e o plugin só é usado pelo núcleo.

## Comandos

| Comando | Entrada | Saída | Erros |
|---------|---------|-------|-------|
| `update_status_get` | `{}` | `UpdateStatusDto` | — |
| `update_check` | `{}` | `UpdateStatusDto` (estado depois de iniciar: `checking`) | `invalid_state` (operação em curso), `updates_disabled`, `notice_pending` |
| `update_install` | `{ version: string }` | `UpdateStatusDto` (`downloading` ou `blocked_by_session`) | `invalid_state` (não está em `available`, ou outra operação em curso), `version_mismatch` (diferente da última verificação), `updates_disabled` |
| `update_cancel` | `{}` | `UpdateStatusDto` | `invalid_state` (nada a cancelar) |
| `update_notice_acknowledge` | `{}` | `SettingsDto` | — |
| `diagnostics_report_csp_violation` | `CspViolationDto` | `{}` | `invalid_input` (formato inválido; nunca ecoa a entrada) |

`update_install` exige confirmação explícita: o frontend só o chama a partir de um diálogo de confirmação
que mostra a versão e as notas. O núcleo não confia nisso. Ele volta a checar o estado e as sessões, e
`version` precisa ser igual à da última verificação.

### `SettingsDto` (campos acrescentados)

```ts
type SettingsDto = {
  // …campos da 001…
  updateCheckEnabled: boolean;        // updates.check_enabled
  updateNoticeAcknowledged: boolean;  // updates.notice_acknowledged (só leitura via settings_update; muda por update_notice_acknowledge)
  updateLastCheckedAt?: string;       // RFC 3339, só leitura
};
```

`settings_update` aceita `updateCheckEnabled`. Tentar alterar `updateNoticeAcknowledged` ou
`updateLastCheckedAt` por ele retorna `invalid_input`.

### `UpdateStatusDto`

```ts
type UpdateStatusDto =
  | { state: "disabled" }
  | { state: "notice_pending" }                       // aviso de primeira execução ainda não dispensado
  | { state: "idle"; lastCheckedAt?: string }
  | { state: "checking" }
  | { state: "up_to_date"; lastCheckedAt: string }
  | { state: "available"; version: string; notes: string; publishedAt?: string }
  | { state: "blocked_by_session"; version: string }
  | { state: "downloading"; version: string; receivedBytes: number; totalBytes?: number }
  | { state: "installing"; version: string }
  | { state: "failed"; reason: "network" | "signature_invalid" | "manifest_invalid" | "download_interrupted" | "install_failed" };
```

`notes` é texto puro. A UI renderiza como texto, nunca como HTML ou Markdown interpretado.

### `CspViolationDto`

```ts
type CspViolationDto = {
  effectiveDirective: "style-src" | "style-src-elem" | "style-src-attr" | "script-src" | "script-src-elem"
                    | "img-src" | "font-src" | "connect-src" | "default-src" | "other";
  blockedScheme: string;   // ≤ 16 chars: esquema (ex.: "https", "data") ou "inline" | "eval"
  disposition: "enforce" | "report";
};
```

O núcleo registra `csp_violation directive=… scheme=… disposition=…` em nível `warn`, com no máximo 20
registros por minuto. Os descartes são contados num registro de resumo.

## Eventos

O canal de sessão da 001 é por sessão. A atualização é global, então usa um canal próprio:

| Assinatura | Evento |
|------------|--------|
| `update_subscribe` → `Channel<UpdateEvent>` | `{ type: "update_status"; status: UpdateStatusDto }`, emitido a cada transição e a cada ≥ 5 % de progresso do download |

## Garantias

- Com `updateCheckEnabled = false` ou `notice_pending`, zero requisições de rede de atualização (SC-014).
- Nenhuma requisição de atualização leva conteúdo de sessão, credenciais ou identificadores além do que o
  HTTP revela por natureza (IP, user-agent).
- Uma falha em qualquer comando de atualização nunca afeta comandos de sessão. Um `update_check` lento
  nunca atrasa os controles de captura (001/SC-006).
- `installing` só é alcançado sem sessão em `starting`, `capturing`, `paused` ou `stopping`.
- Pacote com assinatura inválida → `failed { reason: "signature_invalid" }`, e a versão instalada não muda.
