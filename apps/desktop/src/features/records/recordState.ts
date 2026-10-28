// Records of the current session as the UI mirrors them: proposals arrive by `record_proposed`
// events and user changes by command results (contracts/ipc-events.md). The core decides every
// transition; `nextStatuses` only chooses which actions to offer, following data-model.md.
import type {
  AnalysisStatus,
  RecordDto,
  RecordStatus,
  SessionEvent,
} from "../../ipc/types";

export type AnalysisView = { status: AnalysisStatus; created?: number };

export type RecordsState = {
  /** In creation order. */
  records: RecordDto[];
  /** Latest `analysis_status`, if any run happened. */
  analysis: AnalysisView | null;
};

export const emptyRecords: RecordsState = { records: [], analysis: null };

/** Inserts or replaces `record`, keeping creation order. */
export function upsertRecord(
  state: RecordsState,
  record: RecordDto,
): RecordsState {
  const index = state.records.findIndex((r) => r.id === record.id);
  if (index === -1) {
    const records = [...state.records, record].sort(
      (a, b) => a.createdAt - b.createdAt,
    );
    return { ...state, records };
  }
  const records = [...state.records];
  records[index] = record;
  return { ...state, records };
}

export function applyRecordEvent(
  state: RecordsState,
  event: SessionEvent,
): RecordsState {
  switch (event.type) {
    case "record_proposed":
      return upsertRecord(state, event.record);
    case "analysis_status":
      return {
        ...state,
        analysis: { status: event.status, created: event.created },
      };
    default:
      return state;
  }
}

const TRANSITIONS: Record<RecordStatus, RecordStatus[]> = {
  proposed: ["confirmed", "rejected", "pending"],
  pending: ["confirmed", "rejected"],
  confirmed: ["pending", "rejected"],
  rejected: ["pending", "confirmed"],
};

/** Statuses the user may move a record to from `status`. */
export function nextStatuses(status: RecordStatus): RecordStatus[] {
  return TRANSITIONS[status];
}
