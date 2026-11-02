import { useEffect, useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";

import { LANGUAGES } from "../../i18n";
import { ipc, toIpcError, type IpcClient } from "../../ipc/client";
import type {
  AnalysisLimitsDto,
  IpcError,
  SettingsDto,
  UiLanguage,
  UpdateStatusDto,
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
  /** Global update state, for the "Atualizações" section. */
  updateStatus?: UpdateStatusDto | null;
  /** "Verificar agora"; the core refuses it while disabled or before the notice. */
  onCheckUpdates?: () => void;
};

/** Text of the update state shown next to "Verificar agora", if any. */
function updateStatusText(
  status: UpdateStatusDto | null | undefined,
  t: (key: string) => string,
): string {
  switch (status?.state) {
    case "checking":
      return t("settings.updates.checking");
    case "up_to_date":
      return t("settings.updates.upToDate");
    case "failed":
      return t(`updates.failed.${status.reason}`);
    case "notice_pending":
      return t("settings.updates.noticeFirst");
    default:
      return "";
  }
}

export function SettingsPage({
  client = ipc,
  onLanguageChange,
  updateStatus,
  onCheckUpdates,
}: SettingsPageProps) {
  const { t, i18n } = useTranslation();
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

  async function toggleUpdateChecks(enabled: boolean) {
    setError(null);
    try {
      setSettings(await client.settingsUpdate({ updateCheckEnabled: enabled }));
    } catch (failure) {
      setError(toIpcError(failure));
    }
  }

  function formatInstant(rfc3339: string): string {
    return new Intl.DateTimeFormat(i18n.language, {
      dateStyle: "medium",
      timeStyle: "short",
    }).format(new Date(rfc3339));
  }

  const lastCheckedAt =
    (updateStatus?.state === "idle" || updateStatus?.state === "up_to_date"
      ? updateStatus.lastCheckedAt
      : undefined) ?? settings?.updateLastCheckedAt;
  const canCheckNow =
    settings?.updateCheckEnabled === true &&
    settings.updateNoticeAcknowledged &&
    updateStatus?.state !== "checking" &&
    updateStatus?.state !== "downloading" &&
    updateStatus?.state !== "installing";

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
          <fieldset aria-describedby="updates-destinations">
            <legend>{t("settings.updates.title")}</legend>
            <label>
              <input
                type="checkbox"
                checked={settings.updateCheckEnabled}
                onChange={(event) =>
                  void toggleUpdateChecks(event.target.checked)
                }
              />
              {t("settings.updates.toggle")}
            </label>
            <p id="updates-destinations">
              {t("settings.updates.destinations")}
            </p>
            <p>
              {lastCheckedAt
                ? t("settings.updates.lastChecked", {
                    when: formatInstant(lastCheckedAt),
                  })
                : t("settings.updates.neverChecked")}
            </p>
            <button
              type="button"
              disabled={!canCheckNow}
              onClick={() => onCheckUpdates?.()}
            >
              {t("settings.updates.checkNow")}
            </button>
            <p role="status" aria-live="polite">
              {updateStatusText(updateStatus, t)}
            </p>
          </fieldset>
          <button type="submit">{t("settings.save")}</button>
          <p role="status">{saved ? t("settings.saved") : ""}</p>
        </form>
      )}
    </section>
  );
}
