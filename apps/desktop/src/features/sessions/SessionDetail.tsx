import { useEffect, useMemo, useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";

import { toIpcError, type IpcClient } from "../../ipc/client";
import type { IpcError, SessionDetailDto, SessionEvent } from "../../ipc/types";
import { PlanView, type PlanClient } from "../records/PlanView";
import type { AnalysisView } from "../records/recordState";
import { TranscriptView } from "../transcript/TranscriptView";
import {
  formatOffset,
  transcriptFromItems,
} from "../transcript/transcriptState";
import { formatDate } from "./SessionHistory";

export type DetailClient = PlanClient &
  Pick<
    IpcClient,
    "sessionGet" | "sessionRename" | "analysisRunFinal" | "sessionSubscribe"
  >;

type SessionDetailProps = {
  sessionId: string;
  client: DetailClient;
  onBack: () => void;
};

/**
 * A saved session (US4.3, US4.4): its data, failure reason, transcript and plan, with rename
 * and an explicit final analysis for ended or failed sessions (FR-033).
 */
export function SessionDetail({
  sessionId,
  client,
  onBack,
}: SessionDetailProps) {
  const { t, i18n } = useTranslation();
  const [detail, setDetail] = useState<SessionDetailDto | null>(null);
  const [error, setError] = useState<IpcError | null>(null);
  const [renaming, setRenaming] = useState(false);
  const [title, setTitle] = useState("");
  const [analysis, setAnalysis] = useState<AnalysisView | null>(null);
  const [planVersion, setPlanVersion] = useState(0);

  useEffect(() => {
    let active = true;
    client.sessionGet(sessionId).then(
      (loaded) => {
        if (!active) return;
        setDetail(loaded);
        setTitle(loaded.session.title);
      },
      (failure: unknown) => active && setError(toIpcError(failure)),
    );
    return () => {
      active = false;
    };
  }, [client, sessionId]);

  const state = detail?.session.state;
  const finished = state === "ended" || state === "failed";

  // Final analysis results arrive as session events.
  useEffect(() => {
    if (!finished) return;
    const onEvent = (event: SessionEvent) => {
      if (event.type === "analysis_status")
        setAnalysis({ status: event.status, created: event.created });
      if (event.type === "record_proposed") setPlanVersion((v) => v + 1);
    };
    client.sessionSubscribe(sessionId, onEvent).catch(() => undefined);
  }, [client, sessionId, finished]);

  const transcript = useMemo(
    () => transcriptFromItems(detail?.transcript ?? []),
    [detail],
  );

  async function rename(event: FormEvent) {
    event.preventDefault();
    try {
      const session = await client.sessionRename(sessionId, title);
      setDetail((current) => current && { ...current, session });
      setRenaming(false);
    } catch (failure) {
      setError(toIpcError(failure));
    }
  }

  async function runFinal() {
    setError(null);
    try {
      await client.analysisRunFinal(sessionId);
      setAnalysis({ status: "running" });
    } catch (failure) {
      setError(toIpcError(failure));
    }
  }

  if (!detail) {
    return (
      <section>
        <button type="button" onClick={onBack}>
          {t("detail.back")}
        </button>
        {error ? (
          <p role="alert">{t(error.messageKey)}</p>
        ) : (
          <p>{t("app.loading")}</p>
        )}
      </section>
    );
  }

  const { session } = detail;
  return (
    <section aria-labelledby="detail-title">
      <button type="button" onClick={onBack}>
        {t("detail.back")}
      </button>
      {renaming ? (
        <form onSubmit={rename} className="detail-rename">
          <label>
            {t("sessions.titleLabel")}
            <input
              value={title}
              required
              maxLength={200}
              autoFocus
              onChange={(e) => setTitle(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Escape") setRenaming(false);
              }}
            />
          </label>
          <button type="submit" disabled={title.trim() === ""}>
            {t("plan.save")}
          </button>
          <button type="button" onClick={() => setRenaming(false)}>
            {t("plan.cancel")}
          </button>
        </form>
      ) : (
        <h1 id="detail-title">
          {session.title}{" "}
          <button type="button" onClick={() => setRenaming(true)}>
            {t("detail.rename")}
          </button>
        </h1>
      )}
      <dl className="detail-meta">
        <dt>{t("detail.createdAt")}</dt>
        <dd>{formatDate(session.createdAt, i18n.language)}</dd>
        <dt>{t("detail.activeDuration")}</dt>
        <dd>{formatOffset(session.activeDurationMs)}</dd>
        <dt>{t("sessions.stateLabel")}</dt>
        <dd>{t(`sessions.state.${session.state}`)}</dd>
      </dl>
      {session.failure && (
        <p className="session-failure">
          <span aria-hidden="true">⚠ </span>
          {t(`sessions.failure.${session.failure}`)}
        </p>
      )}
      {!finished && session.state !== "idle" && <p>{t("detail.active")}</p>}
      {error && <p role="alert">{t(error.messageKey)}</p>}
      {finished && (
        <div className="detail-analysis">
          <button
            type="button"
            disabled={analysis?.status === "running"}
            onClick={() => void runFinal()}
          >
            {t("detail.runFinal")}
          </button>
          <p role="status">
            {analysis &&
              t(`records.analysis.${analysis.status}`, {
                count: analysis.created ?? 0,
              })}
          </p>
        </div>
      )}
      <div className="session-workspace">
        <TranscriptView
          transcript={transcript}
          showSources={session.sources.length > 1}
        />
        <PlanView
          sessionId={sessionId}
          client={client}
          refreshKey={planVersion}
        />
      </div>
    </section>
  );
}
