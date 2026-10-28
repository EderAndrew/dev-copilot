import { useTranslation } from "react-i18next";

import type { IpcError, SessionDto } from "../../ipc/types";

type SessionControlsProps = {
  session: SessionDto;
  /** A control command is in flight. */
  busy?: boolean;
  error?: IpcError | null;
  onStart: () => void;
  onCancelStart: () => void;
  onStop: () => void;
};

/**
 * Start/stop controls for one session. Which actions exist in which state mirrors the core
 * state machine; the core still validates every command (Art. IV.11).
 */
export function SessionControls({
  session,
  busy = false,
  error,
  onStart,
  onCancelStart,
  onStop,
}: SessionControlsProps) {
  const { t } = useTranslation();
  const { state, failure } = session;
  return (
    <section className="session-controls" aria-labelledby="session-title">
      <h2 id="session-title">{session.title}</h2>
      <p>
        {t("sessions.stateLabel")}{" "}
        <strong>{t(`sessions.state.${state}`)}</strong>
      </p>
      {state === "failed" && failure && (
        <p role="alert">{t(`sessions.failure.${failure}`)}</p>
      )}
      {error && <p role="alert">{t(error.messageKey)}</p>}
      <div className="session-controls-actions">
        {state === "idle" && (
          <button type="button" onClick={onStart} disabled={busy}>
            {t("sessions.start")}
          </button>
        )}
        {state === "starting" && (
          <button type="button" onClick={onCancelStart} disabled={busy}>
            {t("sessions.cancelStart")}
          </button>
        )}
        {(state === "capturing" || state === "paused") && (
          <button type="button" onClick={onStop} disabled={busy}>
            {t("sessions.stop")}
          </button>
        )}
        {state === "stopping" && (
          <button type="button" disabled>
            {t("sessions.stopping")}
          </button>
        )}
      </div>
    </section>
  );
}
