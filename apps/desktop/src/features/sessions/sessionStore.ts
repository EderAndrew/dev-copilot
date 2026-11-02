// Client-side view of the current session, driven by core events (contracts/ipc-events.md).
// The core is the source of truth: the reducer only mirrors what events and command results
// report, applying events in `seq` order and dropping stale ones. The capture indicator is
// derived from `capture_indicator` events only (SC-002, SC-003).
import { useCallback, useEffect, useReducer, useRef } from "react";

import type { IpcClient } from "../../ipc/client";
import type {
  AudioSourceKind,
  Locality,
  QuestionDto,
  RecordDto,
  SessionDto,
  SessionEvent,
} from "../../ipc/types";
import {
  applyRecordEvent,
  emptyRecords,
  upsertRecord,
  type RecordsState,
} from "../records/recordState";
import { applyQuestionEvent, upsertQuestion } from "../questions/questionState";
import {
  applyTranscriptEvent,
  emptyTranscript,
  type TranscriptState,
} from "../transcript/transcriptState";

export type SourceHealth = "ok" | "lost" | "permission_revoked";

export type ProviderStage = "transcription" | "analysis" | "questions";

export type ProviderInUse = { locality: Locality; providerId: string };

export type CaptureIndicatorState = {
  active: boolean;
  paused: boolean;
  sources: AudioSourceKind[];
};

export type SessionViewState = {
  session: SessionDto | null;
  /** Highest event `seq` applied; kept across resubscriptions. */
  lastSeq: number;
  indicator: CaptureIndicatorState;
  sourceHealth: Partial<Record<AudioSourceKind, SourceHealth>>;
  storageErrorKey: string | null;
  transcript: TranscriptState;
  /** Provider used per stage, from `provider_in_use` (FR-050). */
  providers: Partial<Record<ProviderStage, ProviderInUse>>;
  /** Stages a local provider took over after a remote one failed (FR-045b). */
  fallbacks: ProviderStage[];
  /** Technical records and the latest analysis status (US3). */
  records: RecordsState;
  /** Questions in the order they were asked (US5). */
  questions: QuestionDto[];
};

export type SessionAction =
  | { type: "loaded"; session: SessionDto }
  | { type: "event"; event: SessionEvent }
  | { type: "recordUpdated"; record: RecordDto }
  | {
      type: "detailLoaded";
      sessionId: string;
      records: RecordDto[];
      questions: QuestionDto[];
    }
  | { type: "questionUpdated"; question: QuestionDto }
  | { type: "cleared" };

export const INDICATOR_OFF: CaptureIndicatorState = {
  active: false,
  paused: false,
  sources: [],
};

export const initialSessionState: SessionViewState = {
  session: null,
  lastSeq: 0,
  indicator: INDICATOR_OFF,
  sourceHealth: {},
  storageErrorKey: null,
  transcript: emptyTranscript,
  providers: {},
  fallbacks: [],
  records: emptyRecords,
  questions: [],
};

function applyEvent(
  state: SessionViewState,
  event: SessionEvent,
): SessionViewState {
  if (event.seq <= state.lastSeq) return state;
  const next = {
    ...state,
    lastSeq: event.seq,
    transcript: applyTranscriptEvent(state.transcript, event),
    records: applyRecordEvent(state.records, event),
    questions: applyQuestionEvent(state.questions, event),
  };
  switch (event.type) {
    case "state_changed":
      return next.session
        ? {
            ...next,
            session: {
              ...next.session,
              state: event.state,
              failure: event.failure,
            },
          }
        : next;
    case "capture_indicator":
      return {
        ...next,
        indicator: {
          active: event.active,
          paused: event.paused,
          sources: event.sources,
        },
        // A source listed as capturing again (e.g. after resuming) is healthy.
        sourceHealth:
          event.active && !event.paused
            ? {
                ...next.sourceHealth,
                ...Object.fromEntries(
                  event.sources.map((kind) => [kind, "ok" as const]),
                ),
              }
            : next.sourceHealth,
      };
    case "source_status":
      return {
        ...next,
        sourceHealth: { ...next.sourceHealth, [event.source]: event.status },
      };
    case "storage_error":
      return { ...next, storageErrorKey: event.messageKey };
    case "provider_in_use":
      return {
        ...next,
        providers: {
          ...next.providers,
          [event.role]: {
            locality: event.locality,
            providerId: event.providerId,
          },
        },
      };
    case "provider_fallback":
      return next.fallbacks.includes(event.role)
        ? next
        : { ...next, fallbacks: [...next.fallbacks, event.role] };
    default:
      // Transcript, record and question events are folded in above.
      return next;
  }
}

export function sessionReducer(
  state: SessionViewState,
  action: SessionAction,
): SessionViewState {
  switch (action.type) {
    case "loaded": {
      const sameSession = state.session?.id === action.session.id;
      return {
        ...(sameSession ? state : initialSessionState),
        session: action.session,
      };
    }
    case "event":
      return applyEvent(state, action.event);
    case "detailLoaded":
      return action.sessionId === state.session?.id
        ? {
            ...state,
            records: action.records.reduce(upsertRecord, state.records),
            questions: action.questions.reduce(upsertQuestion, state.questions),
          }
        : state;
    case "questionUpdated":
      return action.question.sessionId === state.session?.id
        ? {
            ...state,
            questions: upsertQuestion(state.questions, action.question),
          }
        : state;
    case "recordUpdated":
      return action.record.sessionId === state.session?.id
        ? { ...state, records: upsertRecord(state.records, action.record) }
        : state;
    case "cleared":
      return initialSessionState;
  }
}

type StoreClient = Pick<IpcClient, "sessionSubscribe" | "sessionGet">;

/**
 * Holds the current session and keeps it subscribed to core events. `track` switches to a
 * session (after create or a control command) and resubscribes; `refresh` resubscribes after
 * a reconnect, resynchronizing from the session the core returns.
 */
export function useSessionStore(client: StoreClient) {
  const [state, dispatch] = useReducer(sessionReducer, initialSessionState);
  const trackedId = useRef<string | null>(null);

  const subscribe = useCallback(
    async (sessionId: string) => {
      trackedId.current = sessionId;
      const current = await client.sessionSubscribe(sessionId, (event) => {
        if (trackedId.current === sessionId) dispatch({ type: "event", event });
      });
      if (trackedId.current === sessionId)
        dispatch({ type: "loaded", session: current });
      // Records and answers from before this subscription (e.g. before a reconnect) are not
      // replayed as events; load them from storage. Upserts make overlap with events harmless.
      try {
        const detail = await client.sessionGet(sessionId);
        if (trackedId.current === sessionId)
          dispatch({
            type: "detailLoaded",
            sessionId,
            records: detail.records,
            questions: detail.questions,
          });
      } catch {
        // Events still deliver new proposals and answers.
      }
    },
    [client],
  );

  // Command results can be older than events already applied (the core may publish
  // `capturing` before `session_start` returns `starting`), so for the tracked session only
  // events change state.
  const track = useCallback(
    (session: SessionDto) => {
      if (trackedId.current === session.id) return;
      dispatch({ type: "loaded", session });
      void subscribe(session.id);
    },
    [subscribe],
  );

  const refresh = useCallback(() => {
    if (trackedId.current) void subscribe(trackedId.current);
  }, [subscribe]);

  /** Applies a record returned by a command (e.g. `record_set_status`). */
  const updateRecord = useCallback((record: RecordDto) => {
    dispatch({ type: "recordUpdated", record });
  }, []);

  /** Applies a question returned by a command (`question_ask`). */
  const updateQuestion = useCallback((question: QuestionDto) => {
    dispatch({ type: "questionUpdated", question });
  }, []);

  const clear = useCallback(() => {
    trackedId.current = null;
    dispatch({ type: "cleared" });
  }, []);

  // Resynchronize when the window comes back (e.g. after sleep or a webview hiccup).
  useEffect(() => {
    const onVisible = () => {
      if (document.visibilityState === "visible") refresh();
    };
    document.addEventListener("visibilitychange", onVisible);
    return () => {
      document.removeEventListener("visibilitychange", onVisible);
      trackedId.current = null;
    };
  }, [refresh]);

  return { state, track, refresh, clear, updateRecord, updateQuestion };
}

export type SessionStore = ReturnType<typeof useSessionStore>;
