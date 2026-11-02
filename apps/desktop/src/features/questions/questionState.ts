// Questions of a session as the UI mirrors them: `question_ask` returns the pending question and
// `question_answered` replaces it with the answer or the failure (contracts/ipc-events.md). The
// core grounds every answer; the UI only shows what it reports.
import type { QuestionDto, SessionEvent } from "../../ipc/types";
import type { TranscriptEntry } from "../transcript/transcriptState";

/** A final segment as a citation shows it: its transcript position and original words. */
export type CitedSegment = { number: number; text: string };

/** Inserts or replaces `question`, keeping the order questions were asked in. */
export function upsertQuestion(
  questions: QuestionDto[],
  question: QuestionDto,
): QuestionDto[] {
  const index = questions.findIndex((q) => q.id === question.id);
  if (index === -1)
    return [...questions, question].sort((a, b) => a.askedAt - b.askedAt);
  // An event can arrive before the command result; never go back to pending.
  if (question.status === "pending" && questions[index].status !== "pending")
    return questions;
  const next = [...questions];
  next[index] = question;
  return next;
}

export function applyQuestionEvent(
  questions: QuestionDto[],
  event: SessionEvent,
): QuestionDto[] {
  return event.type === "question_answered"
    ? upsertQuestion(questions, event.question)
    : questions;
}

/** Whether a question of the session still waits for its answer (one at a time). */
export function hasPending(questions: readonly QuestionDto[]): boolean {
  return questions.some((q) => q.status === "pending");
}

/** Final segments by id with their transcript position (1-based) and original text. */
export function citedSegments(
  entries: readonly TranscriptEntry[],
): Map<string, CitedSegment> {
  const segments = new Map<string, CitedSegment>();
  for (const entry of entries)
    if (entry.kind === "final")
      segments.set(entry.segment.id, {
        number: segments.size + 1,
        text: entry.segment.text,
      });
  return segments;
}
