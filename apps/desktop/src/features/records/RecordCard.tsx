import { useTranslation } from "react-i18next";

import type { RecordDto, RecordStatus } from "../../ipc/types";
import { nextStatuses } from "./recordState";

type RecordCardProps = {
  record: RecordDto;
  /** Position of each cited segment in the transcript, for readable evidence labels. */
  evidenceLabel: (segmentId: string, index: number) => string;
  busy: boolean;
  onSetStatus: (status: RecordStatus) => void;
  onShowEvidence: (segmentId: string) => void;
};

/**
 * One technical record (FR-022): kind, nature, status and markers are shown as text, never by
 * color alone; evidence links jump to the cited transcript segments; review actions are plain
 * buttons, so the whole card works by keyboard (FR-053).
 */
export function RecordCard({
  record,
  evidenceLabel,
  busy,
  onSetStatus,
  onShowEvidence,
}: RecordCardProps) {
  const { t } = useTranslation();
  const textId = `record-${record.id}-text`;

  return (
    <article
      className={`record-card record-${record.status}`}
      aria-labelledby={textId}
    >
      <ul className="record-badges" aria-label={t("records.badgesLabel")}>
        <li className="badge">{t(`records.kind.${record.kind}`)}</li>
        <li className="badge">{t(`records.nature.${record.nature}`)}</li>
        <li className="badge badge-status">
          {t(`records.status.${record.status}`)}
        </li>
        {record.origin === "user" && (
          <li className="badge">{t("records.userCreated")}</li>
        )}
        {record.conflictsWith && (
          <li className="badge badge-warning">
            <span aria-hidden="true">⚠ </span>
            {t("records.conflict")}
          </li>
        )}
        {record.duplicateOf && (
          <li className="badge badge-warning">
            <span aria-hidden="true">⧉ </span>
            {t("records.duplicate")}
          </li>
        )}
      </ul>
      <p id={textId} className="record-text">
        {record.text}
      </p>
      {record.rationale && (
        <p className="record-rationale">
          <strong>{t("records.rationale")}</strong> {record.rationale}
        </p>
      )}
      {record.acceptanceCriteria.length > 0 && (
        <>
          <h4>{t("records.acceptanceCriteria")}</h4>
          <ul>
            {record.acceptanceCriteria.map((item, i) => (
              <li key={i}>{item}</li>
            ))}
          </ul>
        </>
      )}
      {record.alternatives.length > 0 && (
        <>
          <h4>{t("records.alternatives")}</h4>
          <ul>
            {record.alternatives.map((item, i) => (
              <li key={i}>{item}</li>
            ))}
          </ul>
        </>
      )}
      {record.evidence.length > 0 ? (
        <p className="record-evidence">
          {t("records.evidence")}{" "}
          {record.evidence.map((segmentId, i) => (
            <button
              key={segmentId}
              type="button"
              className="link-button"
              onClick={() => onShowEvidence(segmentId)}
            >
              {evidenceLabel(segmentId, i)}
            </button>
          ))}
        </p>
      ) : (
        <p className="record-evidence">{t("records.noEvidence")}</p>
      )}
      <div
        role="group"
        aria-label={t("records.actionsLabel", { text: record.text })}
        className="record-actions"
      >
        {nextStatuses(record.status).map((status) => (
          <button
            key={status}
            type="button"
            disabled={busy}
            onClick={() => onSetStatus(status)}
          >
            {t(`records.action.${status}`)}
          </button>
        ))}
      </div>
    </article>
  );
}
