import { useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";

import { toIpcError, type IpcClient } from "../../ipc/client";
import type {
  EvidenceRefDto,
  IpcError,
  QuestionDto,
  RecordDto,
  RecordKind,
} from "../../ipc/types";
import { showSegment } from "../transcript/showSegment";
import { hasPending, type CitedSegment } from "./questionState";

export type QuestionsClient = Pick<
  IpcClient,
  "questionAsk" | "questionToRecord"
>;

type QuestionsPanelProps = {
  sessionId: string;
  /** In the order they were asked. */
  questions: readonly QuestionDto[];
  /** Final segments of the session, by id. */
  segments: ReadonlyMap<string, CitedSegment>;
  records: readonly RecordDto[];
  client: QuestionsClient;
  /** A question was asked (pending) — the answer arrives later as an event. */
  onAsked: (question: QuestionDto) => void;
  /** An answer became a proposed record. */
  onRecordCreated: (record: RecordDto) => void;
};

const KINDS: readonly RecordKind[] = [
  "decision",
  "requirement",
  "task",
  "risk",
  "open_question",
  "assumption",
];

const MAX_CHARS = 2000;

/**
 * Questions about the session (US5): answers come only from the session, cite the segments and
 * records they rely on — quoted as stored, in their original language (FR-053b) — and say so
 * in words when the session has no evidence (FR-036). Answers never change records; the user
 * may turn one into a proposed item (FR-037). Everything works by keyboard (FR-053).
 */
export function QuestionsPanel({
  sessionId,
  questions,
  segments,
  records,
  client,
  onAsked,
  onRecordCreated,
}: QuestionsPanelProps) {
  const { t } = useTranslation();
  const [draft, setDraft] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<IpcError | null>(null);
  const [kinds, setKinds] = useState<Record<string, RecordKind>>({});
  const [converted, setConverted] = useState<ReadonlySet<string>>(new Set());
  const pending = hasPending(questions);
  const recordsById = new Map(records.map((r) => [r.id, r]));

  async function ask(event?: FormEvent) {
    event?.preventDefault();
    const question = draft.trim();
    if (!question || busy || pending) return;
    setBusy(true);
    setError(null);
    try {
      onAsked(await client.questionAsk({ sessionId, question }));
      setDraft("");
    } catch (failure) {
      setError(toIpcError(failure));
    } finally {
      setBusy(false);
    }
  }

  async function convert(question: QuestionDto) {
    setBusy(true);
    setError(null);
    try {
      onRecordCreated(
        await client.questionToRecord({
          questionId: question.id,
          kind: kinds[question.id] ?? "decision",
        }),
      );
      setConverted((current) => new Set(current).add(question.id));
    } catch (failure) {
      setError(toIpcError(failure));
    } finally {
      setBusy(false);
    }
  }

  function citation(evidence: EvidenceRefDto) {
    if (evidence.type === "segment") {
      const segment = segments.get(evidence.id);
      return (
        <li key={`segment-${evidence.id}`}>
          <button
            type="button"
            className="link-button"
            onClick={() => showSegment(evidence.id)}
          >
            {segment
              ? t("records.segmentNumber", { number: segment.number })
              : t("questions.segmentUnavailable")}
          </button>
          {segment && <blockquote>{segment.text}</blockquote>}
        </li>
      );
    }
    const record = recordsById.get(evidence.id);
    return (
      <li key={`record-${evidence.id}`}>
        {record ? (
          <>
            {t("questions.recordCitation", {
              kind: t(`records.kind.${record.kind}`),
            })}
            <blockquote>{record.text}</blockquote>
          </>
        ) : (
          t("questions.recordUnavailable")
        )}
      </li>
    );
  }

  return (
    <section className="questions" aria-labelledby="questions-title">
      <h2 id="questions-title">{t("questions.title")}</h2>
      <form onSubmit={(e) => void ask(e)} className="questions-form">
        <label htmlFor="question-input">{t("questions.label")}</label>
        <textarea
          id="question-input"
          value={draft}
          maxLength={MAX_CHARS}
          rows={2}
          aria-describedby="question-hint"
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) void ask();
          }}
        />
        <p id="question-hint" className="hint">
          {pending ? t("questions.waitPending") : t("questions.hint")}
        </p>
        <button type="submit" disabled={busy || pending || draft.trim() === ""}>
          {t("questions.ask")}
        </button>
      </form>
      {error && <p role="alert">{t(error.messageKey)}</p>}
      {questions.length === 0 ? (
        <p>{t("questions.empty")}</p>
      ) : (
        <ol className="questions-list">
          {questions.map((question) => {
            const questionId = `question-${question.id}`;
            return (
              <li key={question.id}>
                <article
                  className={`question question-${question.status}`}
                  aria-labelledby={questionId}
                >
                  <p id={questionId} className="question-text">
                    <strong>{t("questions.you")}</strong> {question.question}
                  </p>
                  {question.status === "pending" && (
                    <p role="status">{t("questions.status.pending")}</p>
                  )}
                  {question.status === "failed" && (
                    <p role="status" className="question-failed">
                      <span aria-hidden="true">⚠ </span>
                      {t("questions.status.failed")}
                    </p>
                  )}
                  {question.status === "answered" && (
                    <>
                      <p className="badge">
                        {t(`questions.certainty.${question.certainty}`)}
                      </p>
                      <p className="question-answer">{question.answer}</p>
                      {question.evidence.length > 0 ? (
                        <>
                          <h3>{t("questions.citations")}</h3>
                          <ul className="question-citations">
                            {question.evidence.map(citation)}
                          </ul>
                        </>
                      ) : (
                        <p>{t("questions.noCitations")}</p>
                      )}
                      {converted.has(question.id) ? (
                        <p role="status">{t("questions.converted")}</p>
                      ) : (
                        <div
                          role="group"
                          aria-label={t("questions.convertLabel", {
                            question: question.question,
                          })}
                          className="question-convert"
                        >
                          <label>
                            {t("questions.kindLabel")}
                            <select
                              value={kinds[question.id] ?? "decision"}
                              onChange={(e) =>
                                setKinds((current) => ({
                                  ...current,
                                  [question.id]: e.target.value as RecordKind,
                                }))
                              }
                            >
                              {KINDS.map((kind) => (
                                <option key={kind} value={kind}>
                                  {t(`records.kind.${kind}`)}
                                </option>
                              ))}
                            </select>
                          </label>
                          <button
                            type="button"
                            disabled={busy}
                            onClick={() => void convert(question)}
                          >
                            {t("questions.convert")}
                          </button>
                        </div>
                      )}
                    </>
                  )}
                </article>
              </li>
            );
          })}
        </ol>
      )}
    </section>
  );
}
