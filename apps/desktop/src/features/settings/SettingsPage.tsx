import { useEffect, useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";

import { LANGUAGES } from "../../i18n";
import { ipc, toIpcError, type IpcClient } from "../../ipc/client";
import type {
  AnalysisLimitsDto,
  IpcError,
  SettingsDto,
  UiLanguage,
} from "../../ipc/types";

const LIMIT_FIELDS = [
  "triggerFinals",
  "triggerSpeechSecs",
  "minIntervalSecs",
  "maxCallsPerHour",
  "maxTokensPerSession",
] as const satisfies readonly (keyof AnalysisLimitsDto)[];

type LimitField = (typeof LIMIT_FIELDS)[number];
type LimitDraft = Record<LimitField, string>;

function toDraft(limits: AnalysisLimitsDto): LimitDraft {
  return Object.fromEntries(
    LIMIT_FIELDS.map((field) => [field, String(limits[field])]),
  ) as LimitDraft;
}

/** Parses the form text; range rules are enforced by the core and reported as errors. */
function parseDraft(draft: LimitDraft): AnalysisLimitsDto | LimitField {
  const parsed: Partial<AnalysisLimitsDto> = {};
  for (const field of LIMIT_FIELDS) {
    const text = draft[field].trim();
    if (!/^\d+$/.test(text)) return field;
    parsed[field] = Number(text);
  }
  return parsed as AnalysisLimitsDto;
}

type SettingsPageProps = {
  client?: Pick<IpcClient, "settingsGet" | "settingsUpdate">;
  onLanguageChange?: (language: UiLanguage) => void;
};

export function SettingsPage({
  client = ipc,
  onLanguageChange,
}: SettingsPageProps) {
  const { t } = useTranslation();
  const [settings, setSettings] = useState<SettingsDto | null>(null);
  const [language, setLanguage] = useState<UiLanguage>("pt-BR");
  const [draft, setDraft] = useState<LimitDraft | null>(null);
  const [invalidField, setInvalidField] = useState<LimitField | null>(null);
  const [error, setError] = useState<IpcError | null>(null);
  const [saved, setSaved] = useState(false);

  useEffect(() => {
    let active = true;
    client.settingsGet().then(
      (loaded) => {
        if (!active) return;
        setSettings(loaded);
        setLanguage(loaded.uiLanguage);
        setDraft(toDraft(loaded.analysisLimits));
      },
      (failure: unknown) => active && setError(toIpcError(failure)),
    );
    return () => {
      active = false;
    };
  }, [client]);

  async function onSubmit(event: FormEvent) {
    event.preventDefault();
    if (!draft) return;
    setSaved(false);
    setError(null);
    const limits = parseDraft(draft);
    if (typeof limits === "string") {
      setInvalidField(limits);
      return;
    }
    setInvalidField(null);
    try {
      const updated = await client.settingsUpdate({
        uiLanguage: language,
        analysisLimits: limits,
      });
      setSettings(updated);
      setDraft(toDraft(updated.analysisLimits));
      setSaved(true);
      onLanguageChange?.(updated.uiLanguage);
    } catch (failure) {
      setError(toIpcError(failure));
    }
  }

  return (
    <section aria-labelledby="settings-title">
      <h1 id="settings-title">{t("settings.title")}</h1>
      {error && <p role="alert">{t(error.messageKey)}</p>}
      {!settings || !draft ? (
        !error && <p>{t("app.loading")}</p>
      ) : (
        <form onSubmit={onSubmit} noValidate>
          <fieldset>
            <legend>{t("settings.general")}</legend>
            <label>
              {t("language.label")}
              <select
                value={language}
                onChange={(event) =>
                  setLanguage(event.target.value as UiLanguage)
                }
              >
                {LANGUAGES.map((code) => (
                  <option key={code} value={code} lang={code}>
                    {t(`language.${code}`)}
                  </option>
                ))}
              </select>
            </label>
          </fieldset>
          <fieldset aria-describedby="limits-help">
            <legend>{t("settings.analysisLimits")}</legend>
            <p id="limits-help">{t("settings.analysisLimitsHelp")}</p>
            {LIMIT_FIELDS.map((field) => (
              <div key={field}>
                <label htmlFor={`limit-${field}`}>
                  {t(`settings.${field}`)}
                </label>
                <input
                  id={`limit-${field}`}
                  inputMode="numeric"
                  value={draft[field]}
                  aria-invalid={invalidField === field || undefined}
                  aria-describedby={
                    invalidField === field ? `limit-${field}-error` : undefined
                  }
                  onChange={(event) =>
                    setDraft({ ...draft, [field]: event.target.value })
                  }
                />
                {invalidField === field && (
                  <span id={`limit-${field}-error`}>
                    {t("settings.invalidNumber")}
                  </span>
                )}
              </div>
            ))}
          </fieldset>
          <button type="submit">{t("settings.save")}</button>
          <p role="status">{saved ? t("settings.saved") : ""}</p>
        </form>
      )}
    </section>
  );
}
