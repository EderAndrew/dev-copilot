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
/** Authorization status of a provider (spec "Provedor"); local providers need none. */
export type AuthorizationStatus =
  | "not_required"
  | "configured"
  | "authorized"
  | "revoked";
export type AuthorizationState = "granted" | "revoked";

export type ProviderDto = {
  id: string;
  role: ProviderRole;
  adapter: ProviderAdapterKind;
  locality: Locality;
  endpoint?: string;
  model: string;
  dataCategories: DataCategory[];
  isDefault: boolean;
  authorization: AuthorizationStatus;
  /** An API key is stored in the OS credential store; the key itself is never returned. */
  hasCredential: boolean;
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

export type ProviderSetSecretDto = { providerId: string; secret: string };
export type ProviderIdDto = { providerId: string };
export type ProviderAuthorizeDto = { providerId: string; accepted: true };

/** What the user sees before authorizing a remote provider (FR-046). */
export type AuthorizationPreviewDto = {
  providerId: string;
  destination: string;
  dataCategories: DataCategory[];
  retentionNotice: string;
};

export type ProviderAuthorizationDto = {
  providerId: string;
  state: AuthorizationState;
  destination: string;
  dataCategories: DataCategory[];
  retentionNotice: string;
  grantedAt: number;
  revokedAt?: number;
};

// Local transcription models (contracts/ipc-commands.md "Provedores e autorização") ---------

export type LocalModelDto = {
  /** Catalog id; the `model` of a `whisper_local` provider. */
  id: string;
  displayName: string;
  /** Host the file is downloaded from. */
  origin: string;
  sizeBytes: number;
  downloaded: boolean;
  downloading: boolean;
};

export type LocalModelDownloadDto = { modelId: string };
export type JobIdDto = { jobId: string };

export type ModelDownloadEvent =
  | {
      type: "progress";
      jobId: string;
      receivedBytes: number;
      totalBytes: number;
    }
  | { type: "completed"; jobId: string }
  | { type: "failed"; jobId: string; messageKey: string };

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

export type UnavailableReason =
  | "platform_unsupported"
  | "not_validated_on_platform"
  | "no_device"
  | "os_version";

export type PermissionState =
  "not_requested" | "granted" | "denied" | "revoked";

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

// Technical records (contracts/ipc-commands.md "Registros e plano") ----------------------------

export type RecordKind =
  "requirement" | "decision" | "risk" | "open_question" | "task" | "assumption";
export type Nature = "observed" | "inferred" | "suggested";
export type RecordStatus = "proposed" | "confirmed" | "rejected" | "pending";

export type RecordDto = {
  id: string;
  sessionId: string;
  kind: RecordKind;
  text: string;
  nature: Nature;
  status: RecordStatus;
  rationale: string;
  origin: "ai" | "user";
  providerId?: string;
  analysisRunId?: string;
  /** Ids of the transcript segments cited as evidence. */
  evidence: string[];
  acceptanceCriteria: string[];
  alternatives: string[];
  conflictsWith?: string;
  duplicateOf?: string;
  /** Unix epoch milliseconds. */
  createdAt: number;
  updatedAt: number;
};

export type RecordSetStatusDto = { recordId: string; status: RecordStatus };

export type RecordEditDto = {
  recordId: string;
  text?: string;
  rationale?: string;
  acceptanceCriteria?: string[];
  alternatives?: string[];
};

export type RecordCreateDto = {
  sessionId: string;
  kind: RecordKind;
  text: string;
  rationale?: string;
  status: RecordStatus;
};

/** Derived development plan: one section per kind, in plan order (FR-031). */
export type PlanDto = {
  sessionId: string;
  sections: { kind: RecordKind; items: RecordDto[] }[];
};

export type RunIdDto = { runId: string };

export type AnalysisStatus =
  "running" | "ok" | "invalid_output" | "provider_error" | "rate_limited";

// Contextual questions (contracts/ipc-commands.md "Perguntas") --------------------------------

export type Certainty = "supported" | "partial" | "no_evidence";
export type QuestionStatus = "pending" | "answered" | "failed";

/** Session content cited by an answer; quoted from storage, never translated (FR-053b). */
export type EvidenceRefDto =
  { type: "segment"; id: string } | { type: "record"; id: string };

export type QuestionDto = {
  id: string;
  sessionId: string;
  question: string;
  /** Present iff `status` is `answered`. */
  answer?: string;
  certainty?: Certainty;
  /** Empty iff `certainty` is `no_evidence` (or not answered). */
  evidence: EvidenceRefDto[];
  status: QuestionStatus;
  providerId?: string;
  /** Unix epoch milliseconds. */
  askedAt: number;
  answeredAt?: number;
};

export type QuestionAskDto = { sessionId: string; question: string };

export type QuestionToRecordDto = { questionId: string; kind: RecordKind };

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
      status: AnalysisStatus;
      created?: number;
    }
  | { type: "record_proposed"; seq: number; record: RecordDto }
  | { type: "question_answered"; seq: number; question: QuestionDto }
  | {
      type: "provider_in_use";
      seq: number;
      role: "transcription" | "analysis" | "questions";
      locality: Locality;
      providerId: string;
    }
  | {
      type: "provider_fallback";
      seq: number;
      role: "transcription" | "analysis" | "questions";
      fromProviderId: string;
      toProviderId: string;
    }
  | { type: "transcription_lag"; seq: number; lagMs: number }
  | { type: "storage_error"; seq: number; messageKey: string };

// Sessions (contracts/ipc-commands.md "Sessões") ----------------------------------------------

export type SessionDto = {
  id: string;
  title: string;
  language: SessionLanguage;
  state: SessionState;
  failure?: FailureReason;
  sources: AudioSourceKind[];
  /** Unix epoch milliseconds. */
  createdAt: number;
  startedAt?: number;
  endedAt?: number;
  activeDurationMs: number;
};

export type SessionCreateDto = {
  title: string;
  language: SessionLanguage;
  sources: AudioSourceKind[];
};

export type SessionStartDto = {
  sessionId: string;
  consentAcknowledged: boolean;
};

export type SessionIdDto = { sessionId: string };

export type SessionListDto = { cursor?: string; limit?: number };

export type SessionPageDto = { items: SessionDto[]; nextCursor?: string };

export type SessionRenameDto = { sessionId: string; title: string };

export type TranscriptItemDto =
  | { type: "final"; segment: SegmentDto }
  | { type: "gap"; reason: GapReason; startMs: number; durationMs: number };

/** Everything persisted for a session. */
export type SessionDetailDto = {
  session: SessionDto;
  transcript: TranscriptItemDto[];
  records: RecordDto[];
  /** In the order they were asked (FR-038). */
  questions: QuestionDto[];
};

/** `session_delete` input; `confirmed` must be `true` (FR-042). */
export type SessionDeleteDto = { sessionId: string; confirmed: boolean };

// Export -----------------------------------------------------------------------------------------

export type ExportFormat = "markdown" | "json";
export type ExportContent = "plan" | "transcript";

export type ExportSessionDto = {
  sessionId: string;
  formats: ExportFormat[];
  content: ExportContent[];
};

/** File names written, without directories; empty when the user cancelled the dialog. */
export type ExportResultDto = { written: string[] };

// Audio sources and permissions ----------------------------------------------------------------

export type AudioSourceDto = {
  kind: AudioSourceKind;
  availability: "available" | "unavailable";
  unavailableReason?: UnavailableReason;
  permission: PermissionState;
};

export type PermissionHelpDto = { messageKey: string };
