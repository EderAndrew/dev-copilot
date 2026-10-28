import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { Layout, type Page } from "./components/Layout";
import { CaptureIndicator } from "./features/sessions/CaptureIndicator";
import { SessionsPage } from "./features/sessions/SessionsPage";
import { useSessionStore } from "./features/sessions/sessionStore";
import { SettingsPage } from "./features/settings/SettingsPage";
import { applyLanguage, DEFAULT_LANGUAGE } from "./i18n";
import { ipc, toIpcError } from "./ipc/client";
import type { IpcError, UiLanguage } from "./ipc/types";

function App() {
  const { t } = useTranslation();
  const [page, setPage] = useState<Page>("sessions");
  const [language, setLanguage] = useState<UiLanguage>(DEFAULT_LANGUAGE);
  const [startupError, setStartupError] = useState<IpcError | null>(null);
  // Lives above the pages so the indicator stays visible on every screen (SC-003).
  const sessionStore = useSessionStore(ipc);

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
      captureIndicator={
        <CaptureIndicator indicator={sessionStore.state.indicator} />
      }
    >
      {startupError && <p role="alert">{t(startupError.messageKey)}</p>}
      {page === "sessions" && (
        <SessionsPage store={sessionStore} defaultLanguage={language} />
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
