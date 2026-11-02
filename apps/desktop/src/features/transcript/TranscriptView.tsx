import { useLayoutEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import {
  formatOffset,
  type PartialEntry,
  type TranscriptEntry,
  type TranscriptState,
} from "./transcriptState";

type TranscriptViewProps = {
  transcript: TranscriptState;
  /** Show source tags (sessions capturing two sources, FR-016). */
  showSources: boolean;
};

/** Distance from the bottom, in px, still treated as "at the bottom". */
const BOTTOM_SLACK = 24;

/**
 * Progressive transcript (FR-015–FR-018): finals with timestamps, partials marked as such,
 * gaps with their reason, and a lag notice. Follows new content until the user scrolls up;
 * a button brings auto-scroll back.
 */
export function TranscriptView({
  transcript,
  showSources,
}: TranscriptViewProps) {
  const { t } = useTranslation();
  const container = useRef<HTMLDivElement>(null);
  const [following, setFollowing] = useState(true);
  const partials = Object.values(transcript.partials).sort(
    (a, b) => a.startMs - b.startMs,
  );

  useLayoutEffect(() => {
    const element = container.current;
    if (following && element) element.scrollTop = element.scrollHeight;
  }, [transcript.entries, transcript.partials, following]);

  function onScroll() {
    const element = container.current;
    if (!element) return;
    const atBottom =
      element.scrollHeight - element.scrollTop - element.clientHeight <=
      BOTTOM_SLACK;
    setFollowing(atBottom);
  }

  const source = (kind?: string) =>
    showSources && kind ? (
      <span className="transcript-source">{t(`sources.${kind}`)}</span>
    ) : null;

  const time = (ms: number) => (
    <time
      className="transcript-time"
      dateTime={`PT${(ms / 1000).toFixed(1)}S`}
      aria-label={t("transcript.at", { time: formatOffset(ms) })}
    >
      {formatOffset(ms)}
    </time>
  );

  function renderEntry(entry: TranscriptEntry) {
    if (entry.kind === "final") {
      const { segment } = entry;
      return (
        <li
          key={segment.id}
          id={`segment-${segment.id}`}
          tabIndex={-1}
          className="transcript-final"
        >
          {time(segment.startMs)} {source(segment.source)}{" "}
          <span>{segment.text}</span>
        </li>
      );
    }
    return (
      <li
        key={`gap-${entry.startMs}-${entry.reason}`}
        className="transcript-gap"
      >
        {time(entry.startMs)}{" "}
        <span>
          {t("transcript.gap", {
            reason: t(`transcript.gapReason.${entry.reason}`),
            duration: formatOffset(entry.durationMs),
          })}
        </span>
      </li>
    );
  }

  function renderPartial(partial: PartialEntry) {
    return (
      <li
        key={`partial-${partial.source ?? "single"}`}
        className="transcript-partial"
      >
        {time(partial.startMs)} {source(partial.source)}{" "}
        <span className="visually-hidden">{t("transcript.partialLabel")} </span>
        <span>{partial.text}</span>
      </li>
    );
  }

  const empty = transcript.entries.length === 0 && partials.length === 0;

  return (
    <section className="transcript" aria-labelledby="transcript-title">
      <h2 id="transcript-title">{t("transcript.title")}</h2>
      {transcript.lagMs > 0 && (
        <p className="transcript-lag" role="status">
          <span aria-hidden="true">⏱ </span>
          {t("transcript.lag", {
            seconds: Math.round(transcript.lagMs / 1000),
          })}
        </p>
      )}
      <div
        ref={container}
        className="transcript-scroll"
        onScroll={onScroll}
        tabIndex={0}
        role="region"
        aria-label={t("transcript.regionLabel")}
      >
        {empty ? (
          <p>{t("transcript.empty")}</p>
        ) : (
          <ol className="transcript-list">
            {transcript.entries.map(renderEntry)}
            {partials.map(renderPartial)}
          </ol>
        )}
      </div>
      {!following && (
        <button type="button" onClick={() => setFollowing(true)}>
          {t("transcript.followLatest")}
        </button>
      )}
    </section>
  );
}
