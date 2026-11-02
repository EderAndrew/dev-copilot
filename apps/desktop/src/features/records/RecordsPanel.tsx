import { useState } from "react";
import { useTranslation } from "react-i18next";

import { toIpcError, type IpcClient } from "../../ipc/client";
import type { IpcError, RecordDto, RecordStatus } from "../../ipc/types";
import { showSegment } from "../transcript/showSegment";
import { RecordCard } from "./RecordCard";
import type { RecordsState } from "./recordState";

export type RecordsClient = Pick<IpcClient, "recordSetStatus">;

type RecordsPanelProps = {
  records: RecordsState;
  /** Transcript position (1-based) of each final segment, by id. */
  segmentNumbers: ReadonlyMap<string, number>;
  client: RecordsClient;
  onUpdated: (record: RecordDto) => void;
};

/**
 * Proposals from incremental analysis and their review (US3). Analysis failures are reported
 * here without touching the records already shown (FR-030).
 */
export function RecordsPanel({
  records,
  segmentNumbers,
  client,
  onUpdated,
}: RecordsPanelProps) {
  const { t } = useTranslation();
  const [busyId, setBusyId] = useState<string | null>(null);
  const [error, setError] = useState<IpcError | null>(null);

  async function setStatus(record: RecordDto, status: RecordStatus) {
    setBusyId(record.id);
    setError(null);
    try {
      onUpdated(await client.recordSetStatus({ recordId: record.id, status }));
    } catch (failure) {
      setError(toIpcError(failure));
    } finally {
      setBusyId(null);
    }
  }

  const analysis = records.analysis;
  const evidenceLabel = (segmentId: string, index: number) => {
    const number = segmentNumbers.get(segmentId);
    return number
      ? t("records.segmentNumber", { number })
      : t("records.segmentIndex", { index: index + 1 });
  };

  return (
    <section className="records" aria-labelledby="records-title">
      <h2 id="records-title">{t("records.title")}</h2>
      <p role="status" className="records-analysis">
        {analysis &&
          t(`records.analysis.${analysis.status}`, {
            count: analysis.created ?? 0,
          })}
      </p>
      {error && <p role="alert">{t(error.messageKey)}</p>}
      {records.records.length === 0 ? (
        <p>{t("records.empty")}</p>
      ) : (
        <ol className="records-list">
          {records.records.map((record) => (
            <li key={record.id}>
              <RecordCard
                record={record}
                evidenceLabel={evidenceLabel}
                busy={busyId === record.id}
                onSetStatus={(status) => void setStatus(record, status)}
                onShowEvidence={showSegment}
              />
            </li>
          ))}
        </ol>
      )}
    </section>
  );
}
