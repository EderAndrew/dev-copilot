import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { Layout, type Page } from "./components/Layout";
import { SettingsPage } from "./features/settings/SettingsPage";
import { applyLanguage, DEFAULT_LANGUAGE } from "./i18n";
import { ipc, toIpcError } from "./ipc/client";
import type { IpcError, UiLanguage } from "./ipc/types";

function App() {
  const { t } = useTranslation();
  const [page, setPage] = useState<Page>("sessions");
  const [language, setLanguage] = useState<UiLanguage>(DEFAULT_LANGUAGE);
  const [startupError, setStartupError] = useState<IpcError | null>(null);

  useEffect(() => {
    ipc.settingsGet().then(
      (settings) => {
        setLanguage(settings.uiLanguage);
        void applyLanguage(settings.uiLanguage);
      },
      (error: unknown) => setStartupError(toIpcError(error)),
    );
  }, []);

  function changeLanguage(next: UiLanguage) {
    setLanguage(next);
    void applyLanguage(next);
    // Persisting is best effort: without storage the choice still applies to this run.
    ipc.settingsUpdate({ uiLanguage: next }).catch(() => undefined);
  }

  return (
    <Layout
      page={page}
      onNavigate={setPage}
      language={language}
      onLanguageChange={changeLanguage}
    >
      {startupError && <p role="alert">{t(startupError.messageKey)}</p>}
      {page === "sessions" && (
        <section aria-labelledby="sessions-title">
          <h1 id="sessions-title">{t("sessions.title")}</h1>
          <p>{t("sessions.empty")}</p>
        </section>
      )}
      {page === "providers" && (
        <section aria-labelledby="providers-title">
          <h1 id="providers-title">{t("providers.title")}</h1>
          <p>{t("providers.empty")}</p>
        </section>
      )}
      {page === "settings" && (
        <SettingsPage onLanguageChange={changeLanguage} />
      )}
    </Layout>
  );
}

export default App;
