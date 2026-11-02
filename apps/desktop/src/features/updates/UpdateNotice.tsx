import { useId } from "react";
import { useTranslation } from "react-i18next";

/** Network destinations of update checks (specs/002 contracts/csp-policy.md). */
export const UPDATE_DESTINATIONS = ["manifest", "assets"] as const;

type UpdateNoticeProps = {
  onAcknowledge: () => void;
};

/**
 * First-run notice (FR-008a/b): explains when the app checks for updates, which hosts it
 * contacts and that checks can be switched off. No check happens until it is dismissed.
 */
export function UpdateNotice({ onAcknowledge }: UpdateNoticeProps) {
  const { t } = useTranslation();
  const id = useId();
  return (
    <section aria-labelledby={`${id}-title`} className="update-notice">
      <h2 id={`${id}-title`}>{t("updates.notice.title")}</h2>
      <p>{t("updates.notice.body")}</p>
      <p id={`${id}-destinations`}>{t("updates.notice.destinationsLabel")}</p>
      <ul aria-labelledby={`${id}-destinations`}>
        {UPDATE_DESTINATIONS.map((destination) => (
          <li key={destination}>
            {t(`updates.notice.destination.${destination}`)}
          </li>
        ))}
      </ul>
      <p>{t("updates.notice.settingsHint")}</p>
      <button type="button" onClick={onAcknowledge}>
        {t("updates.notice.acknowledge")}
      </button>
    </section>
  );
}
