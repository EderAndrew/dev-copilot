// Every shared fixture in specs/001-meeting-sessions/contracts/fixtures/ must match the
// TypeScript DTOs in ./types (research R11). Enum value lists below are typed as
// `Record<Union, true>`, so they fail to compile when they drift from ./types.
import { describe, expect, it } from "vitest";

import { isIpcError } from "./client";
import type {
  AudioSourceKind,
  DataCategory,
  ErrorCode,
  FailureReason,
  GapReason,
  Locality,
  ModelDownloadEvent,
  PermissionState,
  ProviderAdapterKind,
  ProviderRole,
  Nature,
  RecordKind,
  RecordStatus,
  SessionEvent,
  SessionLanguage,
  SessionState,
  UiLanguage,
  UnavailableReason,
} from "./types";

const fixtures = import.meta.glob<unknown>(
  "../../../../specs/001-meeting-sessions/contracts/fixtures/*/*.json",
  { eager: true, import: "default" },
);

type Check = (value: unknown) => boolean;
type Shape = {
  required: Record<string, Check>;
  optional?: Record<string, Check>;
};

const keys = <T extends string>(values: Record<T, true>): Check => {
  const allowed = new Set<string>(Object.keys(values));
  return (value) => typeof value === "string" && allowed.has(value);
};
const str: Check = (v) => typeof v === "string";
const bool: Check = (v) => typeof v === "boolean";
const nonNegativeInt: Check = (v) =>
  typeof v === "number" && Number.isInteger(v) && v >= 0;
const literal =
  (expected: string): Check =>
  (v) =>
    v === expected;
const arrayOf =
  (check: Check): Check =>
  (v) =>
    Array.isArray(v) && v.every(check);
const recordOf =
  (check: Check): Check =>
  (v) =>
    typeof v === "object" &&
    v !== null &&
    !Array.isArray(v) &&
    Object.values(v).every(check);

function problems(value: unknown, shape: Shape): string[] {
  if (typeof value !== "object" || value === null || Array.isArray(value))
    return ["not an object"];
  const record = value as Record<string, unknown>;
  const optional = shape.optional ?? {};
  const found: string[] = [];
  for (const [key, check] of Object.entries(shape.required)) {
    if (!(key in record)) found.push(`missing ${key}`);
    else if (!check(record[key])) found.push(`invalid ${key}`);
  }
  for (const [key, field] of Object.entries(record)) {
    const check = shape.required[key] ?? optional[key];
    if (!check) found.push(`unknown ${key}`);
    else if (key in optional && !check(field)) found.push(`invalid ${key}`);
  }
  return found;
}
const object =
  (shape: Shape): Check =>
  (v) =>
    problems(v, shape).length === 0;

const errorCode = keys<ErrorCode>({
  invalid_input: true,
  not_found: true,
  invalid_state: true,
  permission_denied: true,
  source_unavailable: true,
  no_transcriber: true,
  secret_store_unavailable: true,
  authorization_required: true,
  provider_error: true,
  storage_error: true,
  internal: true,
});
const uiLanguage = keys<UiLanguage>({ "pt-BR": true, en: true });
const role = keys<ProviderRole>({ transcription: true, analysis: true });
const locality = keys<Locality>({ local: true, remote: true });
const adapter = keys<ProviderAdapterKind>({
  whisper_local: true,
  openai_compatible_stt: true,
  anthropic: true,
  openai_compatible_chat: true,
  ollama: true,
});
const category = keys<DataCategory>({
  audio: true,
  transcript: true,
  records: true,
  questions: true,
});
const sessionState = keys<SessionState>({
  idle: true,
  starting: true,
  capturing: true,
  paused: true,
  stopping: true,
  ended: true,
  failed: true,
});
const failure = keys<FailureReason>({
  permission_denied: true,
  source_unavailable: true,
  no_transcriber: true,
  storage_error: true,
  secret_store_unavailable: true,
  interrupted: true,
  internal: true,
});
const source = keys<AudioSourceKind>({ microphone: true, system: true });
const gapReason = keys<GapReason>({
  paused: true,
  overload: true,
  provider_failure: true,
  cancelled_on_stop: true,
  source_lost: true,
  interrupted: true,
});

const sessionLanguage = keys<SessionLanguage>({ "pt-BR": true, en: true });
const unavailableReason = keys<UnavailableReason>({
  platform_unsupported: true,
  not_validated_on_platform: true,
  no_device: true,
  os_version: true,
});
const permission = keys<PermissionState>({
  not_requested: true,
  granted: true,
  denied: true,
  revoked: true,
});
const epochMs = nonNegativeInt;
const recordKind = keys<RecordKind>({
  requirement: true,
  decision: true,
  risk: true,
  open_question: true,
  task: true,
  assumption: true,
});
const nature = keys<Nature>({
  observed: true,
  inferred: true,
  suggested: true,
});
const recordStatus = keys<RecordStatus>({
  proposed: true,
  confirmed: true,
  rejected: true,
  pending: true,
});
const recordShape: Shape = {
  required: {
    id: str,
    sessionId: str,
    kind: recordKind,
    text: str,
    nature,
    status: recordStatus,
    rationale: str,
    origin: keys<"ai" | "user">({ ai: true, user: true }),
    evidence: arrayOf(str),
    acceptanceCriteria: arrayOf(str),
    alternatives: arrayOf(str),
    createdAt: epochMs,
    updatedAt: epochMs,
  },
  optional: {
    providerId: str,
    analysisRunId: str,
    conflictsWith: str,
    duplicateOf: str,
  },
};

const analysisLimits = object({
  required: {
    triggerFinals: nonNegativeInt,
    triggerSpeechSecs: nonNegativeInt,
    minIntervalSecs: nonNegativeInt,
    maxCallsPerHour: nonNegativeInt,
    maxTokensPerSession: nonNegativeInt,
  },
});

const sessionShape: Shape = {
  required: {
    id: str,
    title: str,
    language: sessionLanguage,
    state: sessionState,
    sources: arrayOf(source),
    createdAt: epochMs,
    activeDurationMs: nonNegativeInt,
  },
  optional: { failure, startedAt: epochMs, endedAt: epochMs },
};

const segmentShape: Shape = {
  required: {
    id: str,
    seq: nonNegativeInt,
    startMs: nonNegativeInt,
    endMs: nonNegativeInt,
    text: str,
  },
  optional: { source },
};

const event = (type: SessionEvent["type"], shape: Shape): Shape => ({
  required: { type: literal(type), seq: nonNegativeInt, ...shape.required },
  optional: shape.optional,
});

const eventShapes: Record<SessionEvent["type"], Shape> = {
  state_changed: event("state_changed", {
    required: { state: sessionState },
    optional: { failure },
  }),
  capture_indicator: event("capture_indicator", {
    required: { active: bool, paused: bool, sources: arrayOf(source) },
  }),
  partial_segment: event("partial_segment", {
    required: { startMs: nonNegativeInt, text: str },
    optional: { source },
  }),
  final_segment: event("final_segment", {
    required: { segment: object(segmentShape) },
  }),
  gap: event("gap", {
    required: {
      reason: gapReason,
      startMs: nonNegativeInt,
      durationMs: nonNegativeInt,
    },
  }),
  source_status: event("source_status", {
    required: {
      source,
      status: keys<"ok" | "lost" | "permission_revoked">({
        ok: true,
        lost: true,
        permission_revoked: true,
      }),
    },
  }),
  analysis_status: event("analysis_status", {
    required: {
      runId: str,
      status: keys<
        Extract<SessionEvent, { type: "analysis_status" }>["status"]
      >({
        running: true,
        ok: true,
        invalid_output: true,
        provider_error: true,
        rate_limited: true,
      }),
    },
    optional: { created: nonNegativeInt },
  }),
  record_proposed: event("record_proposed", {
    required: { record: object(recordShape) },
  }),
  provider_in_use: event("provider_in_use", {
    required: {
      role: keys<"transcription" | "analysis" | "questions">({
        transcription: true,
        analysis: true,
        questions: true,
      }),
      locality,
      providerId: str,
    },
  }),
  transcription_lag: event("transcription_lag", {
    required: { lagMs: nonNegativeInt },
  }),
  storage_error: event("storage_error", { required: { messageKey: str } }),
};

const downloadEventShapes: Record<ModelDownloadEvent["type"], Shape> = {
  progress: {
    required: {
      type: literal("progress"),
      jobId: str,
      receivedBytes: nonNegativeInt,
      totalBytes: nonNegativeInt,
    },
  },
  completed: { required: { type: literal("completed"), jobId: str } },
  failed: {
    required: { type: literal("failed"), jobId: str, messageKey: str },
  },
};

const dtoShapes: Record<string, (value: unknown) => string[]> = {
  IpcError: (v) =>
    problems(v, {
      required: { code: errorCode, messageKey: str },
      optional: { details: recordOf(str) },
    }),
  SettingsDto: (v) =>
    problems(v, {
      required: { uiLanguage, consentReminderDismissed: bool, analysisLimits },
    }),
  SettingsUpdateDto: (v) =>
    problems(v, {
      required: {},
      optional: { uiLanguage, consentReminderDismissed: bool, analysisLimits },
    }),
  ProviderDto: (v) =>
    problems(v, {
      required: {
        id: str,
        role,
        adapter,
        locality,
        model: str,
        dataCategories: arrayOf(category),
        isDefault: bool,
      },
      optional: { endpoint: str },
    }),
  ProviderUpsertDto: (v) =>
    problems(v, {
      required: {
        role,
        adapter,
        model: str,
        dataCategories: arrayOf(category),
      },
      optional: { id: str, endpoint: str },
    }),
  ProviderSetDefaultDto: (v) =>
    problems(v, { required: { role, providerId: str } }),
  SessionDto: (v) => problems(v, sessionShape),
  SessionCreateDto: (v) =>
    problems(v, {
      required: {
        title: str,
        language: sessionLanguage,
        sources: arrayOf(source),
      },
    }),
  SessionStartDto: (v) =>
    problems(v, { required: { sessionId: str, consentAcknowledged: bool } }),
  SessionIdDto: (v) => problems(v, { required: { sessionId: str } }),
  SessionListDto: (v) =>
    problems(v, {
      required: {},
      optional: { cursor: str, limit: nonNegativeInt },
    }),
  SessionPageDto: (v) =>
    problems(v, {
      required: { items: arrayOf(object(sessionShape)) },
      optional: { nextCursor: str },
    }),
  SessionRenameDto: (v) =>
    problems(v, { required: { sessionId: str, title: str } }),
  SessionDetailDto: (v) =>
    problems(v, {
      required: {
        session: object(sessionShape),
        transcript: arrayOf((item) => {
          const type = (item as { type?: unknown } | null)?.type;
          return type === "final"
            ? object({
                required: {
                  type: literal("final"),
                  segment: object(segmentShape),
                },
              })(item)
            : object({
                required: {
                  type: literal("gap"),
                  reason: gapReason,
                  startMs: nonNegativeInt,
                  durationMs: nonNegativeInt,
                },
              })(item);
        }),
        records: arrayOf(object(recordShape)),
      },
    }),
  RecordEditDto: (v) =>
    problems(v, {
      required: { recordId: str },
      optional: {
        text: str,
        rationale: str,
        acceptanceCriteria: arrayOf(str),
        alternatives: arrayOf(str),
      },
    }),
  RecordCreateDto: (v) =>
    problems(v, {
      required: {
        sessionId: str,
        kind: recordKind,
        text: str,
        status: recordStatus,
      },
      optional: { rationale: str },
    }),
  PlanDto: (v) =>
    problems(v, {
      required: {
        sessionId: str,
        sections: arrayOf(
          object({
            required: { kind: recordKind, items: arrayOf(object(recordShape)) },
          }),
        ),
      },
    }),
  RunIdDto: (v) => problems(v, { required: { runId: str } }),
  RecordDto: (v) => problems(v, recordShape),
  RecordSetStatusDto: (v) =>
    problems(v, { required: { recordId: str, status: recordStatus } }),
  AudioSourceDto: (v) =>
    problems(v, {
      required: {
        kind: source,
        availability: keys<"available" | "unavailable">({
          available: true,
          unavailable: true,
        }),
        permission,
      },
      optional: { unavailableReason },
    }),
  PermissionHelpDto: (v) => problems(v, { required: { messageKey: str } }),
  LocalModelDto: (v) =>
    problems(v, {
      required: {
        id: str,
        displayName: str,
        origin: str,
        sizeBytes: nonNegativeInt,
        downloaded: bool,
        downloading: bool,
      },
    }),
  LocalModelDownloadDto: (v) => problems(v, { required: { modelId: str } }),
  JobIdDto: (v) => problems(v, { required: { jobId: str } }),
  ModelDownloadEvent: (v) => {
    const type = (v as { type?: unknown } | null)?.type;
    const shape =
      typeof type === "string"
        ? downloadEventShapes[type as ModelDownloadEvent["type"]]
        : undefined;
    return shape ? problems(v, shape) : [`unknown event type ${String(type)}`];
  },
  SessionEvent: (v) => {
    const type = (v as { type?: unknown } | null)?.type;
    const shape =
      typeof type === "string"
        ? eventShapes[type as SessionEvent["type"]]
        : undefined;
    return shape ? problems(v, shape) : [`unknown event type ${String(type)}`];
  },
};

const entries = Object.entries(fixtures).map(([path, value]) => {
  const [dto, file] = path.split("/").slice(-2);
  return { dto, file, value };
});

describe("contract fixtures", () => {
  it("are found", () => {
    expect(entries.length).toBeGreaterThan(0);
  });

  it("only use known DTO directories", () => {
    const unknown = entries
      .filter(({ dto }) => !(dto in dtoShapes))
      .map(({ dto }) => dto);
    expect(unknown).toEqual([]);
  });

  it.each(entries)(
    "$dto/$file matches the TypeScript type",
    ({ dto, value }) => {
      expect(dtoShapes[dto]?.(value)).toEqual([]);
    },
  );

  it("IpcError fixtures pass the runtime guard", () => {
    for (const { value } of entries.filter((e) => e.dto === "IpcError")) {
      expect(isIpcError(value)).toBe(true);
    }
  });

  it("rejects values outside the contract", () => {
    expect(
      dtoShapes.SessionEvent({
        type: "gap",
        seq: 1,
        reason: "boredom",
        startMs: 0,
        durationMs: 0,
      }),
    ).not.toEqual([]);
    expect(dtoShapes.SettingsUpdateDto({ theme: "dark" })).toEqual([
      "unknown theme",
    ]);
  });
});
