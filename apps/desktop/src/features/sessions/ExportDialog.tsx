import { useEffect, useId, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { toIpcError, type IpcClient } from "../../ipc/client";
import type { ExportContent, ExportFormat, IpcError } from "../../ipc/types";

export type ExportClient = Pick<IpcClient, "exportSession">;

type ExportDialogProps = {
  sessionId: string;
  client: ExportClient;
  onClose: () => void;
};

const FORMATS: readonly ExportFormat[] = ["markdown", "json"];
const CONTENT: readonly ExportContent[] = ["plan", "transcript"];

type Outcome = { written: string[] } | null;

function toggle<T>(values: readonly T[], value: T, on: boolean): T[] {
  return on
    ? [...values.filter((v) => v !== value), value]
    : values.filter((v) => v !== value);
}

/**
 * Export of a saved session (US6.1, FR-041): the user picks formats and content, is told the
 * files are not encrypted, and the core asks where to save each file with the system dialog.
 */
export function ExportDialog({
  sessionId,
  client,
  onClose,
}: ExportDialogProps) {
  const { t } = useTranslation();
  const ids = useId();
  const [formats, setFormats] = useState<ExportFormat[]>([...FORMATS]);
  const [content, setContent] = useState<ExportContent[]>([...CONTENT]);
  const [busy, setBusy] = useState(false);
  const [outcome, setOutcome] = useState<Outcome>(null);
  const [error, setError] = useState<IpcError | null>(null);
  const firstRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    firstRef.current?.focus();
  }, []);

  async function exportSession() {
    setBusy(true);
    setError(null);
    setOutcome(null);
    try {
      // Keep the contract order regardless of the order boxes were ticked in.
      const result = await client.exportSession({
        sessionId,
        formats: FORMATS.filter((f) => formats.includes(f)),
        content: CONTENT.filter((c) => content.includes(c)),
      });
      setOutcome(result);
    } catch (failure) {
      setError(toIpcError(failure));
    } finally {
      setBusy(false);
    }
  }

  const ready = formats.length > 0 && content.length > 0 && !busy;

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-labelledby={`${ids}-title`}
      aria-describedby={`${ids}-warning`}
      className="dialog"
      onKeyDown={(event) => {
        if (event.key === "Escape" && !busy) onClose();
      }}
    >
      <h2 id={`${ids}-title`}>{t("export.title")}</h2>
      <p id={`${ids}-warning`} className="export-warning">
        <span aria-hidden="true">⚠ </span>
        {t("export.unencryptedWarning")}
      </p>
      <fieldset>
        <legend>{t("export.formats")}</legend>
        {FORMATS.map((format, index) => (
          <label key={format}>
            <input
              ref={index === 0 ? firstRef : undefined}
              type="checkbox"
              checked={formats.includes(format)}
              onChange={(event) =>
                setFormats((current) =>
                  toggle(current, format, event.target.checked),
                )
              }
            />
            {t(`export.format.${format}`)}
          </label>
        ))}
      </fieldset>
      <fieldset>
        <legend>{t("export.content")}</legend>
        {CONTENT.map((part) => (
          <label key={part}>
            <input
              type="checkbox"
              checked={content.includes(part)}
              onChange={(event) =>
                setContent((current) =>
                  toggle(current, part, event.target.checked),
                )
              }
            />
            {t(`export.part.${part}`)}
          </label>
        ))}
      </fieldset>
      {!ready && !busy && <p>{t("export.chooseSomething")}</p>}
      <p role="status">
        {busy && t("export.choosingDestination")}
        {outcome &&
          (outcome.written.length > 0
            ? t("export.written", { files: outcome.written.join(", ") })
            : t("export.cancelled"))}
      </p>
      {error && <p role="alert">{t(error.messageKey)}</p>}
      <div className="dialog-actions">
        <button
          type="button"
          disabled={!ready}
          onClick={() => void exportSession()}
        >
          {t("export.submit")}
        </button>
        <button type="button" disabled={busy} onClick={onClose}>
          {t("export.close")}
        </button>
      </div>
    </div>
  );
}
