// IPC DTOs mirrored by hand from the Rust side (research R11). Shapes follow
// specs/001-meeting-sessions/contracts/ and are checked against the shared fixtures in
// contract.test.ts.

export type UiLanguage = "pt-BR" | "en";
export type SessionLanguage = "pt-BR" | "en";

export type ErrorCode =
  | "invalid_input"
  | "not_found"
  | "invalid_state"
  | "permission_denied"
  | "source_unavailable"
  | "no_transcriber"
  | "secret_store_unavailable"
  | "authorization_required"
  | "provider_error"
  | "storage_error"
  | "internal";

export type IpcError = {
  code: ErrorCode;
  messageKey: string;
  details?: Record<string, string>;
};

// Settings ------------------------------------------------------------------------------------

export type AnalysisLimitsDto = {
  triggerFinals: number;
  triggerSpeechSecs: number;
  minIntervalSecs: number;
  maxCallsPerHour: number;
  maxTokensPerSession: number;
};

export type SettingsDto = {
  uiLanguage: UiLanguage;
  consentReminderDismissed: boolean;
  analysisLimits: AnalysisLimitsDto;
};

export type SettingsUpdateDto = Partial<SettingsDto>;

// Providers -----------------------------------------------------------------------------------

export type ProviderRole = "transcription" | "analysis";
export type Locality = "local" | "remote";
export type ProviderAdapterKind =
  | "whisper_local"
  | "openai_compatible_stt"
  | "anthropic"
  | "openai_compatible_chat"
  | "ollama";
export type DataCategory = "audio" | "transcript" | "records" | "questions";

export type ProviderDto = {
  id: string;
  role: ProviderRole;
  adapter: ProviderAdapterKind;
  locality: Locality;
  endpoint?: string;
  model: string;
  dataCategories: DataCategory[];
  isDefault: boolean;
};

export type ProviderUpsertDto = {
  id?: string;
  role: ProviderRole;
  adapter: ProviderAdapterKind;
  endpoint?: string;
  model: string;
  dataCategories: DataCategory[];
};

export type ProviderSetDefaultDto = {
  role: ProviderRole;
  providerId: string;
};

// Session events (contracts/ipc-events.md) ------------------------------------------------------

export type SessionState =
  | "idle"
  | "starting"
  | "capturing"
  | "paused"
  | "stopping"
  | "ended"
  | "failed";

export type FailureReason =
  | "permission_denied"
  | "source_unavailable"
  | "no_transcriber"
  | "storage_error"
  | "secret_store_unavailable"
  | "interrupted"
  | "internal";

export type AudioSourceKind = "microphone" | "system";

export type GapReason =
  | "paused"
  | "overload"
  | "provider_failure"
  | "cancelled_on_stop"
  | "source_lost"
  | "interrupted";

export type SegmentDto = {
  id: string;
  seq: number;
  source?: AudioSourceKind;
  startMs: number;
  endMs: number;
  text: string;
};

// `record_proposed` and `question_answered` are added with US3 and US5.
export type SessionEvent =
  | {
      type: "state_changed";
      seq: number;
      state: SessionState;
      failure?: FailureReason;
    }
  | {
      type: "capture_indicator";
      seq: number;
      active: boolean;
      paused: boolean;
      sources: AudioSourceKind[];
    }
  | {
      type: "partial_segment";
      seq: number;
      source?: AudioSourceKind;
      startMs: number;
      text: string;
    }
  | { type: "final_segment"; seq: number; segment: SegmentDto }
  | {
      type: "gap";
      seq: number;
      reason: GapReason;
      startMs: number;
      durationMs: number;
    }
  | {
      type: "source_status";
      seq: number;
      source: AudioSourceKind;
      status: "ok" | "lost" | "permission_revoked";
    }
  | {
      type: "analysis_status";
      seq: number;
      runId: string;
      status:
        "running" | "ok" | "invalid_output" | "provider_error" | "rate_limited";
      created?: number;
    }
  | {
      type: "provider_in_use";
      seq: number;
      role: "transcription" | "analysis" | "questions";
      locality: Locality;
      providerId: string;
    }
  | { type: "transcription_lag"; seq: number; lagMs: number }
  | { type: "storage_error"; seq: number; messageKey: string };
