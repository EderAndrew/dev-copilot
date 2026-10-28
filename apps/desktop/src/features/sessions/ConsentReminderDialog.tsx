import { useEffect, useId, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

type ConsentReminderDialogProps = {
  onConfirm: (dontShowAgain: boolean) => void;
  onCancel: () => void;
};

/**
 * Reminder shown before each capture start until dismissed (FR-012). Confirming is required to
 * start; the core rejects a start without it.
 */
export function ConsentReminderDialog({
  onConfirm,
  onCancel,
}: ConsentReminderDialogProps) {
  const { t } = useTranslation();
  const ids = useId();
  const [dontShowAgain, setDontShowAgain] = useState(false);
  const confirmRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    confirmRef.current?.focus();
  }, []);

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-labelledby={`${ids}-title`}
      aria-describedby={`${ids}-body`}
      className="dialog"
      onKeyDown={(event) => {
        if (event.key === "Escape") onCancel();
      }}
    >
      <h2 id={`${ids}-title`}>{t("consent.title")}</h2>
      <p id={`${ids}-body`}>{t("consent.body")}</p>
      <label>
        <input
          type="checkbox"
          checked={dontShowAgain}
          onChange={(event) => setDontShowAgain(event.target.checked)}
        />
        {t("consent.dontShowAgain")}
      </label>
      <div className="dialog-actions">
        <button
          ref={confirmRef}
          type="button"
          onClick={() => onConfirm(dontShowAgain)}
        >
          {t("consent.confirm")}
        </button>
        <button type="button" onClick={onCancel}>
          {t("consent.cancel")}
        </button>
      </div>
    </div>
  );
}
