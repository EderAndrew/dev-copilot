import { useId, useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";

import { LANGUAGES } from "../../i18n";
import type {
  AudioSourceDto,
  AudioSourceKind,
  SessionCreateDto,
  SessionLanguage,
} from "../../ipc/types";

type NewSessionFormProps = {
  sources: AudioSourceDto[];
  defaultLanguage: SessionLanguage;
  busy?: boolean;
  onCreate: (request: SessionCreateDto) => void;
};

/** Title, language and source selection; unavailable sources show why (FR-008, FR-014). */
export function NewSessionForm({
  sources,
  defaultLanguage,
  busy = false,
  onCreate,
}: NewSessionFormProps) {
  const { t } = useTranslation();
  const ids = useId();
  const [title, setTitle] = useState("");
  const [language, setLanguage] = useState<SessionLanguage>(defaultLanguage);
  const [selected, setSelected] = useState<AudioSourceKind[]>(() =>
    sources.some(
      (s) => s.kind === "microphone" && s.availability === "available",
    )
      ? ["microphone"]
      : [],
  );
  const [showErrors, setShowErrors] = useState(false);
  const titleMissing = title.trim() === "";
  const noSource = selected.length === 0;

  function toggle(kind: AudioSourceKind, on: boolean) {
    setSelected((current) =>
      on ? [...current, kind] : current.filter((k) => k !== kind),
    );
  }

  function onSubmit(event: FormEvent) {
    event.preventDefault();
    if (titleMissing || noSource) {
      setShowErrors(true);
      return;
    }
    onCreate({ title: title.trim(), language, sources: selected });
  }

  return (
    <form onSubmit={onSubmit} noValidate aria-labelledby={`${ids}-legend`}>
      <h2 id={`${ids}-legend`}>{t("sessions.new")}</h2>
      <div>
        <label htmlFor={`${ids}-title`}>{t("sessions.titleLabel")}</label>
        <input
          id={`${ids}-title`}
          value={title}
          maxLength={200}
          required
          aria-invalid={(showErrors && titleMissing) || undefined}
          aria-describedby={
            showErrors && titleMissing ? `${ids}-title-error` : undefined
          }
          onChange={(event) => setTitle(event.target.value)}
        />
        {showErrors && titleMissing && (
          <span id={`${ids}-title-error`}>{t("sessions.titleRequired")}</span>
        )}
      </div>
      <div>
        <label htmlFor={`${ids}-language`}>{t("sessions.languageLabel")}</label>
        <select
          id={`${ids}-language`}
          value={language}
          onChange={(event) =>
            setLanguage(event.target.value as SessionLanguage)
          }
        >
          {LANGUAGES.map((code) => (
            <option key={code} value={code} lang={code}>
              {t(`language.${code}`)}
            </option>
          ))}
        </select>
      </div>
      <fieldset
        aria-describedby={
          showErrors && noSource ? `${ids}-sources-error` : undefined
        }
      >
        <legend>{t("sessions.sourcesLabel")}</legend>
        {sources.map((source) => {
          const unavailable = source.availability === "unavailable";
          const reasonId = `${ids}-${source.kind}-reason`;
          return (
            <div key={source.kind}>
              <label>
                <input
                  type="checkbox"
                  checked={selected.includes(source.kind)}
                  disabled={unavailable}
                  aria-describedby={unavailable ? reasonId : undefined}
                  onChange={(event) =>
                    toggle(source.kind, event.target.checked)
                  }
                />
                {t(`sources.${source.kind}`)}
              </label>
              {unavailable && source.unavailableReason && (
                <span id={reasonId} className="source-reason">
                  {" "}
                  {t(`sources.unavailable.${source.unavailableReason}`)}
                </span>
              )}
            </div>
          );
        })}
        {showErrors && noSource && (
          <span id={`${ids}-sources-error`}>
            {t("sessions.sourceRequired")}
          </span>
        )}
      </fieldset>
      <button type="submit" disabled={busy}>
        {t("sessions.create")}
      </button>
    </form>
  );
}
