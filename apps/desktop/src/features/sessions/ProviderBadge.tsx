import { useTranslation } from "react-i18next";

import type { ProviderInUse, ProviderStage } from "./sessionStore";

type ProviderBadgeProps = {
  providers: Partial<Record<ProviderStage, ProviderInUse>>;
};

const STAGES: ProviderStage[] = ["transcription", "analysis", "questions"];

/**
 * Whether each stage runs locally or on a remote provider (FR-050), as reported by the core's
 * `provider_in_use` events. Remote stages are marked by text and icon, not by color alone.
 */
export function ProviderBadge({ providers }: ProviderBadgeProps) {
  const { t } = useTranslation();
  const stages = STAGES.filter((stage) => providers[stage]);
  if (stages.length === 0) return null;
  return (
    <ul className="provider-badge" aria-label={t("providerInUse.label")}>
      {stages.map((stage) => {
        const locality = providers[stage]?.locality ?? "local";
        return (
          <li
            key={stage}
            className={`provider-badge-item provider-badge-item--${locality}`}
          >
            <span aria-hidden="true">{locality === "local" ? "⌂ " : "⇅ "}</span>
            {t("providerInUse.item", {
              stage: t(`providerInUse.stage.${stage}`),
              locality: t(`providerInUse.locality.${locality}`),
            })}
          </li>
        );
      })}
    </ul>
  );
}
