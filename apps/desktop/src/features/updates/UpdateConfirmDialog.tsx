import { useEffect, useId, useRef, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";

type UpdateConfirmDialogProps = {
  version: string;
  notes: string;
  onConfirm: (version: string) => void;
  onCancel: () => void;
};

/**
 * Explicit confirmation of one version (FR-008c). Keyboard only: focus starts on "Agora não",
 * Tab cycles inside the dialog, Escape closes it. The core re-checks the version and sessions.
 */
export function UpdateConfirmDialog({
  version,
  notes,
  onConfirm,
  onCancel,
}: UpdateConfirmDialogProps) {
  const { t } = useTranslation();
  const ids = useId();
  const dialogRef = useRef<HTMLDivElement>(null);
  const cancelRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    const previous = document.activeElement;
    cancelRef.current?.focus();
    return () => {
      if (previous instanceof HTMLElement) previous.focus();
    };
  }, []);

  function onKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    if (event.key === "Escape") {
      event.preventDefault();
      onCancel();
      return;
    }
    if (event.key !== "Tab") return;
    const focusable = Array.from(
      dialogRef.current?.querySelectorAll<HTMLElement>(
        "button:not([disabled])",
      ) ?? [],
    );
    if (focusable.length === 0) return;
    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  }

  return (
    <div
      ref={dialogRef}
      role="dialog"
      aria-modal="true"
      aria-labelledby={`${ids}-title`}
      aria-describedby={`${ids}-body`}
      className="dialog"
      onKeyDown={onKeyDown}
    >
      <h2 id={`${ids}-title`}>{t("updates.confirm.title", { version })}</h2>
      <p id={`${ids}-body`}>{t("updates.confirm.body")}</p>
      {notes && (
        <>
          <h3>{t("updates.banner.notesLabel")}</h3>
          {/* Untrusted manifest text: rendered as text, never as HTML or Markdown. */}
          <p className="update-notes">{notes}</p>
        </>
      )}
      <div className="dialog-actions">
        <button type="button" onClick={() => onConfirm(version)}>
          {t("updates.confirm.confirm")}
        </button>
        <button ref={cancelRef} type="button" onClick={onCancel}>
          {t("updates.confirm.cancel")}
        </button>
      </div>
    </div>
  );
}
