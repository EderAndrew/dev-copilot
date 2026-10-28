import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";

import { LANGUAGES } from "../i18n";
import type { UiLanguage } from "../ipc/types";

export type Page = "sessions" | "history" | "providers" | "settings";

const PAGES: readonly Page[] = ["sessions", "history", "providers", "settings"];

type LayoutProps = {
  page: Page;
  onNavigate: (page: Page) => void;
  language: UiLanguage;
  onLanguageChange: (language: UiLanguage) => void;
  /** Persistent slot for the capture indicator (filled by US1). */
  captureIndicator?: ReactNode;
  children: ReactNode;
};

export function Layout({
  page,
  onNavigate,
  language,
  onLanguageChange,
  captureIndicator,
  children,
}: LayoutProps) {
  const { t } = useTranslation();
  return (
    <div className="layout">
      <header className="layout-header">
        <span className="layout-title">{t("app.title")}</span>
        <div
          className="layout-capture"
          role="region"
          aria-label={t("capture.regionLabel")}
        >
          {captureIndicator}
        </div>
        <label className="layout-language">
          <span>{t("language.label")}</span>
          <select
            value={language}
            onChange={(event) =>
              onLanguageChange(event.target.value as UiLanguage)
            }
          >
            {LANGUAGES.map((code) => (
              <option key={code} value={code} lang={code}>
                {t(`language.${code}`)}
              </option>
            ))}
          </select>
        </label>
      </header>
      <nav className="layout-nav" aria-label={t("nav.label")}>
        <ul>
          {PAGES.map((target) => (
            <li key={target}>
              <button
                type="button"
                aria-current={target === page ? "page" : undefined}
                onClick={() => onNavigate(target)}
              >
                {t(`nav.${target}`)}
              </button>
            </li>
          ))}
        </ul>
      </nav>
      <main className="layout-main">{children}</main>
    </div>
  );
}
