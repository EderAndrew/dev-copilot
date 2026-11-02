// i18next setup (FR-053a). pt-BR is the default; both catalogs must have identical keys
// (checked by i18n.test.ts, SC-019).
import i18n from "i18next";
import { initReactI18next } from "react-i18next";

import type { UiLanguage } from "../ipc/types";
import en from "./en.json";
import ptBR from "./pt-BR.json";

export const DEFAULT_LANGUAGE: UiLanguage = "pt-BR";
export const LANGUAGES: readonly UiLanguage[] = ["pt-BR", "en"];

export const resources = {
  "pt-BR": { translation: ptBR },
  en: { translation: en },
} as const;

export function initI18n(language: UiLanguage = DEFAULT_LANGUAGE) {
  if (!i18n.isInitialized) {
    void i18n.use(initReactI18next).init({
      resources,
      lng: language,
      fallbackLng: DEFAULT_LANGUAGE,
      interpolation: { escapeValue: false }, // React already escapes
      returnNull: false,
    });
  }
  return i18n;
}

export function applyLanguage(language: UiLanguage) {
  document.documentElement.lang = language;
  return i18n.changeLanguage(language);
}

export default i18n;
