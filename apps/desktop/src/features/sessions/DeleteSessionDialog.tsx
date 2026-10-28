import { useEffect, useId, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { toIpcError, type IpcClient } from "../../ipc/client";
import type { IpcError, SessionDto, SessionState } from "../../ipc/types";

export type DeleteClient = Pick<IpcClient, "sessionDelete">;

type DeleteSessionDialogProps = {
  session: SessionDto;
  client: DeleteClient;
  onDeleted: () => void;
  onCancel: () => void;
};

const ACTIVE: readonly SessionState[] = [
  "starting",
  "capturing",
  "paused",
  "stopping",
];

const WARNINGS = [
  "secureDeletion",
  "backups",
  "exportedFiles",
  "irreversible",
] as const;

/**
 * Deletion of a saved session (US6.2–US6.4, FR-042, FR-044): states the limits of secure
 * deletion, system backups and already exported files, and deletes only on explicit
 * confirmation. An active session must be stopped first. Focus starts on Cancel.
 */
export function DeleteSessionDialog({
  session,
  client,
  onDeleted,
  onCancel,
}: DeleteSessionDialogProps) {
  const { t } = useTranslation();
  const ids = useId();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<IpcError | null>(null);
  const cancelRef = useRef<HTMLButtonElement>(null);
  const active = ACTIVE.includes(session.state);

  useEffect(() => {
    cancelRef.current?.focus();
  }, []);

  async function confirm() {
    setBusy(true);
    setError(null);
    try {
      await client.sessionDelete(session.id);
      onDeleted();
    } catch (failure) {
      setError(toIpcError(failure));
      setBusy(false);
    }
  }

  return (
    <div
      role="alertdialog"
      aria-modal="true"
      aria-labelledby={`${ids}-title`}
      aria-describedby={`${ids}-body`}
      className="dialog"
      onKeyDown={(event) => {
        if (event.key === "Escape" && !busy) onCancel();
      }}
    >
      <h2 id={`${ids}-title`}>
        {t("deleteSession.title", { title: session.title })}
      </h2>
      {active ? (
        <p id={`${ids}-body`}>{t("deleteSession.stopFirst")}</p>
      ) : (
        <ul id={`${ids}-body`} className="delete-warnings">
          {WARNINGS.map((warning) => (
            <li key={warning}>{t(`deleteSession.warning.${warning}`)}</li>
          ))}
        </ul>
      )}
      {error && <p role="alert">{t(error.messageKey)}</p>}
      <div className="dialog-actions">
        {!active && (
          <button
            type="button"
            className="danger"
            disabled={busy}
            onClick={() => void confirm()}
          >
            {busy ? t("deleteSession.deleting") : t("deleteSession.confirm")}
          </button>
        )}
        <button
          ref={cancelRef}
          type="button"
          disabled={busy}
          onClick={onCancel}
        >
          {t("deleteSession.cancel")}
        </button>
      </div>
    </div>
  );
}
