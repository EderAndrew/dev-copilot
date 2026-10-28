import { useTranslation } from "react-i18next";

import type { CaptureIndicatorState } from "./sessionStore";

type CaptureIndicatorProps = { indicator: CaptureIndicatorState };

/**
 * Persistent capture indicator for the header (FR-004, SC-003). State is conveyed by icon
 * shape and text, never by color alone (FR-053), and announced politely to screen readers.
 */
export function CaptureIndicator({ indicator }: CaptureIndicatorProps) {
  const { t } = useTranslation();
  const mode = !indicator.active ? "off" : indicator.paused ? "paused" : "on";
  const sources = indicator.sources
    .map((kind) => t(`sources.${kind}`))
    .join(", ");
  const icon = { on: "●", paused: "❚❚", off: "○" }[mode];
  return (
    <p
      className={`capture-indicator capture-indicator--${mode}`}
      role="status"
      aria-live="polite"
      aria-atomic="true"
    >
      <span className="capture-indicator-icon" aria-hidden="true">
        {icon}
      </span>{" "}
      <span>
        {mode === "off"
          ? t("capture.off")
          : t(mode === "on" ? "capture.on" : "capture.paused", { sources })}
      </span>
    </p>
  );
}
