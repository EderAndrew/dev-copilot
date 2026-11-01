import { useId, useState } from "react";
import { useTranslation } from "react-i18next";

import type { IpcError, UpdateStatusDto } from "../../ipc/types";
import { UpdateConfirmDialog } from "./UpdateConfirmDialog";

type UpdateBannerProps = {
  status: UpdateStatusDto | null;
  error: IpcError | null;
  onInstall: (version: string) => void;
  onCancel: () => void;
};

/** What the polite live region announces for `status`, if anything. */
function announcement(
  status: UpdateStatusDto | null,
  t: (key: string, options?: Record<string, unknown>) => string,
): string {
  if (!status) return "";
  switch (status.state) {
    case "blocked_by_session":
      return t("updates.banner.blocked", { version: status.version });
    case "failed":
      return t(`updates.failed.${status.reason}`);
    case "installing":
      return t("updates.banner.installing", { version: status.version });
    default:
      return "";
  }
}

/**
 * Update banner (US8): offers an available version with its notes as plain text, asks for
 * confirmation, and shows download progress, blocking by an active session and failures.
 */
export function UpdateBanner({
  status,
  error,
  onInstall,
  onCancel,
}: UpdateBannerProps) {
  const { t } = useTranslation();
  const id = useId();
  const [confirming, setConfirming] = useState<{
    version: string;
    notes: string;
  } | null>(null);
  const message = error ? t(error.messageKey) : announcement(status, t);

  return (
    <>
      <div role="status" aria-live="polite" className="update-live">
        {message}
      </div>
      {status?.state === "available" && (
        <section aria-labelledby={`${id}-title`} className="update-banner">
          <h2 id={`${id}-title`}>
            {t("updates.banner.available", { version: status.version })}
          </h2>
          {status.notes && (
            // Untrusted manifest text: rendered as text, never as HTML or Markdown.
            <p className="update-notes">{status.notes}</p>
          )}
          <button
            type="button"
            onClick={() =>
              setConfirming({ version: status.version, notes: status.notes })
            }
          >
            {t("updates.banner.install")}
          </button>
        </section>
      )}
      {status?.state === "downloading" && (
        <section aria-labelledby={`${id}-title`} className="update-banner">
          <h2 id={`${id}-title`}>
            {t("updates.banner.downloading", { version: status.version })}
          </h2>
          <progress
            aria-label={t("updates.banner.progressLabel")}
            value={status.receivedBytes}
            max={status.totalBytes}
          />
          <button type="button" onClick={onCancel}>
            {t("updates.banner.cancel")}
          </button>
        </section>
      )}
      {status?.state === "blocked_by_session" && (
        // The core re-checks sessions on every attempt; nothing is installed meanwhile.
        <button type="button" onClick={() => onInstall(status.version)}>
          {t("updates.banner.retry")}
        </button>
      )}
      {confirming && (
        <UpdateConfirmDialog
          version={confirming.version}
          notes={confirming.notes}
          onConfirm={(version) => {
            setConfirming(null);
            onInstall(version);
          }}
          onCancel={() => setConfirming(null)}
        />
      )}
    </>
  );
}
