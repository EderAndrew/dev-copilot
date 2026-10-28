// Client-side view of the current session, driven by core events (contracts/ipc-events.md).
// The core is the source of truth: the reducer only mirrors what events and command results
// report, applying events in `seq` order and dropping stale ones. The capture indicator is
// derived from `capture_indicator` events only (SC-002, SC-003).
import { useCallback, useEffect, useReducer, useRef } from "react";

import type { IpcClient } from "../../ipc/client";
import type {
  AudioSourceKind,
  SessionDto,
  SessionEvent,
} from "../../ipc/types";

export type SourceHealth = "ok" | "lost" | "permission_revoked";

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
};

export type SessionAction =
  | { type: "loaded"; session: SessionDto }
  | { type: "event"; event: SessionEvent }
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
};

function applyEvent(
  state: SessionViewState,
  event: SessionEvent,
): SessionViewState {
  if (event.seq <= state.lastSeq) return state;
  const next = { ...state, lastSeq: event.seq };
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
      };
    case "source_status":
      return {
        ...next,
        sourceHealth: { ...next.sourceHealth, [event.source]: event.status },
      };
    case "storage_error":
      return { ...next, storageErrorKey: event.messageKey };
    default:
      // Transcript, analysis and provider events are handled by later stories.
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
    case "cleared":
      return initialSessionState;
  }
}

type StoreClient = Pick<IpcClient, "sessionSubscribe">;

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

  return { state, track, refresh, clear };
}

export type SessionStore = ReturnType<typeof useSessionStore>;
