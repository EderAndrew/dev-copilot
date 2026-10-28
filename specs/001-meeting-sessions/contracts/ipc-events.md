# Contract: Eventos de sessão (núcleo → frontend)

Entregues por `tauri::ipc::Channel<SessionEvent>` aberto com `session_subscribe`. Eventos são ordenados por
`seq` dentro da sessão; o frontend descarta eventos com `seq` menor que o último aplicado. Ao reconectar, o
frontend chama `session_get` e volta a assinar.

```ts
type SessionEvent =
  | { type: "state_changed"; seq: number; state: SessionState; failure?: FailureReason }
  | { type: "capture_indicator"; seq: number; active: boolean; paused: boolean; sources: AudioSourceKind[] }
  | { type: "partial_segment"; seq: number; source?: AudioSourceKind; startMs: number; text: string }
  | { type: "final_segment"; seq: number; segment: SegmentDto }
  | { type: "gap"; seq: number; reason: GapReason; startMs: number; durationMs: number }
  | { type: "source_status"; seq: number; source: AudioSourceKind; status: "ok" | "lost" | "permission_revoked" }
  | { type: "analysis_status"; seq: number; runId: string; status: "running" | "ok" | "invalid_output" | "provider_error" | "rate_limited"; created?: number }
  | { type: "record_proposed"; seq: number; record: RecordDto }
  | { type: "question_answered"; seq: number; question: QuestionDto }
  | { type: "provider_in_use"; seq: number; role: "transcription" | "analysis" | "questions"; locality: "local" | "remote"; providerId: string }
  | { type: "transcription_lag"; seq: number; lagMs: number }
  | { type: "storage_error"; seq: number; messageKey: string };
```

Garantias:
- `capture_indicator.active = false` é emitido em até 1 s após `session_stop` (SC-002).
- `partial_segment` nunca é persistido; é substituído pelo `final_segment` de mesmo `startMs` ou por um `gap`.
- `provider_in_use` é emitido no início e em qualquer troca de provedor (FR-050, FR-045b).
- `transcription_lag` é emitido quando o atraso entre o áudio captado e o último trecho final passa de 15 s
  (SC-005), e de novo com `lagMs = 0` quando volta ao normal (FR-018 "atraso", US2.4).
- Nenhum evento carrega segredos; conteúdo de sessão só é enviado à janela `main`.
