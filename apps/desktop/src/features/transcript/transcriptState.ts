// Transcript view of the current session, built from core events (contracts/ipc-events.md).
// Finals and gaps are kept in time order; partials live only here, one per source, and are
// dropped once a final or a gap covers them (FR-015, FR-017, FR-018).
import type {
  AudioSourceKind,
  GapReason,
  SegmentDto,
  SessionEvent,
  TranscriptItemDto,
} from "../../ipc/types";

export type TranscriptEntry =
  | { kind: "final"; segment: SegmentDto }
  | { kind: "gap"; reason: GapReason; startMs: number; durationMs: number };

export type PartialEntry = {
  source?: AudioSourceKind;
  startMs: number;
  text: string;
};

export type TranscriptState = {
  entries: TranscriptEntry[];
  /** Keyed by source, or "single" for one-source sessions. */
  partials: Record<string, PartialEntry>;
  /** Current transcription lag; 0 when transcription keeps up. */
  lagMs: number;
};

export const emptyTranscript: TranscriptState = {
  entries: [],
  partials: {},
  lagMs: 0,
};

const partialKey = (source?: AudioSourceKind) => source ?? "single";

function startOf(entry: TranscriptEntry): number {
  return entry.kind === "final" ? entry.segment.startMs : entry.startMs;
}

/** Inserts keeping start order; equal starts keep arrival order. */
function insert(
  entries: TranscriptEntry[],
  entry: TranscriptEntry,
): TranscriptEntry[] {
  const start = startOf(entry);
  let index = entries.length;
  while (index > 0 && startOf(entries[index - 1]) > start) index -= 1;
  return [...entries.slice(0, index), entry, ...entries.slice(index)];
}

function dropPartials(
  partials: Record<string, PartialEntry>,
  covered: (partial: PartialEntry) => boolean,
): Record<string, PartialEntry> {
  return Object.fromEntries(
    Object.entries(partials).filter(([, partial]) => !covered(partial)),
  );
}

/** Applies a transcript event; other events leave the state unchanged. */
export function applyTranscriptEvent(
  state: TranscriptState,
  event: SessionEvent,
): TranscriptState {
  switch (event.type) {
    case "partial_segment":
      return {
        ...state,
        partials: {
          ...state.partials,
          [partialKey(event.source)]: {
            source: event.source,
            startMs: event.startMs,
            text: event.text,
          },
        },
      };
    case "final_segment": {
      const { segment } = event;
      if (
        state.entries.some(
          (e) => e.kind === "final" && e.segment.id === segment.id,
        )
      )
        return state;
      return {
        ...state,
        entries: insert(state.entries, { kind: "final", segment }),
        partials: dropPartials(
          state.partials,
          (p) =>
            partialKey(p.source) === partialKey(segment.source) &&
            p.startMs < segment.endMs,
        ),
      };
    }
    case "gap": {
      const end = event.startMs + event.durationMs;
      return {
        ...state,
        entries: insert(state.entries, {
          kind: "gap",
          reason: event.reason,
          startMs: event.startMs,
          durationMs: event.durationMs,
        }),
        // Partials of the lost audio are shown as the gap, not as content (US2.3).
        partials: dropPartials(state.partials, (p) => p.startMs < end),
      };
    }
    case "transcription_lag":
      return { ...state, lagMs: event.lagMs };
    case "state_changed":
      // Partials never outlive capture.
      return event.state === "ended" || event.state === "failed"
        ? { ...state, partials: {}, lagMs: 0 }
        : state;
    default:
      return state;
  }
}

/** Transcript of a saved session, from `session_get` (no partials, no lag). */
export function transcriptFromItems(
  items: TranscriptItemDto[],
): TranscriptState {
  const entries = items.reduce<TranscriptEntry[]>(
    (acc, item) =>
      insert(
        acc,
        item.type === "final"
          ? { kind: "final", segment: item.segment }
          : {
              kind: "gap",
              reason: item.reason,
              startMs: item.startMs,
              durationMs: item.durationMs,
            },
      ),
    [],
  );
  return { ...emptyTranscript, entries };
}

/** `m:ss`, or `h:mm:ss` from one hour on. */
export function formatOffset(ms: number): string {
  const total = Math.floor(ms / 1000);
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const seconds = String(total % 60).padStart(2, "0");
  return hours > 0
    ? `${hours}:${String(minutes).padStart(2, "0")}:${seconds}`
    : `${minutes}:${seconds}`;
}
