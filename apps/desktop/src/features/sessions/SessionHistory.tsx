import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { toIpcError, type IpcClient } from "../../ipc/client";
import type { IpcError, SessionDto } from "../../ipc/types";
import { formatOffset } from "../transcript/transcriptState";

export type HistoryClient = Pick<IpcClient, "sessionList">;

type SessionHistoryProps = {
  client: HistoryClient;
  onOpen: (sessionId: string) => void;
};

const PAGE_SIZE = 20;

export function formatDate(epochMs: number, language: string): string {
  return new Intl.DateTimeFormat(language, {
    dateStyle: "short",
    timeStyle: "short",
  }).format(epochMs);
}

/** Saved sessions, newest first (US4.3): title, date, active duration and state. */
export function SessionHistory({ client, onOpen }: SessionHistoryProps) {
  const { t, i18n } = useTranslation();
  const [sessions, setSessions] = useState<SessionDto[]>([]);
  const [cursor, setCursor] = useState<string | undefined>();
  const [loaded, setLoaded] = useState(false);
  const [error, setError] = useState<IpcError | null>(null);

  useEffect(() => {
    let active = true;
    client.sessionList({ limit: PAGE_SIZE }).then(
      (page) => {
        if (!active) return;
        setSessions(page.items);
        setCursor(page.nextCursor);
        setLoaded(true);
      },
      (failure: unknown) => {
        if (!active) return;
        setError(toIpcError(failure));
        setLoaded(true);
      },
    );
    return () => {
      active = false;
    };
  }, [client]);

  async function loadMore(after: string) {
    try {
      const page = await client.sessionList({
        cursor: after,
        limit: PAGE_SIZE,
      });
      setSessions((current) => [...current, ...page.items]);
      setCursor(page.nextCursor);
    } catch (failure) {
      setError(toIpcError(failure));
    }
  }

  return (
    <section aria-labelledby="history-title">
      <h1 id="history-title">{t("history.title")}</h1>
      {error && <p role="alert">{t(error.messageKey)}</p>}
      {!loaded ? (
        <p>{t("app.loading")}</p>
      ) : sessions.length === 0 ? (
        !error && <p>{t("history.empty")}</p>
      ) : (
        <ul className="history-list">
          {sessions.map((session) => (
            <li key={session.id} className="history-item">
              <button
                type="button"
                className="link-button"
                onClick={() => onOpen(session.id)}
              >
                {session.title}
              </button>
              <span>{formatDate(session.createdAt, i18n.language)}</span>
              <span>
                {t("history.duration", {
                  duration: formatOffset(session.activeDurationMs),
                })}
              </span>
              <span className="badge badge-status">
                {session.state === "failed" && (
                  <span aria-hidden="true">⚠ </span>
                )}
                {t(`sessions.state.${session.state}`)}
              </span>
            </li>
          ))}
        </ul>
      )}
      {cursor && (
        <button type="button" onClick={() => void loadMore(cursor)}>
          {t("history.more")}
        </button>
      )}
    </section>
  );
}
