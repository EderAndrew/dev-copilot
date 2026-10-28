import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { ipc, toIpcError, type IpcClient } from "../../ipc/client";
import type {
  AudioSourceDto,
  IpcError,
  SessionCreateDto,
  SessionDto,
  SessionLanguage,
} from "../../ipc/types";
import { RecordsPanel } from "../records/RecordsPanel";
import { ConsentReminderDialog } from "./ConsentReminderDialog";
import { NewSessionForm } from "./NewSessionForm";
import { TranscriptView } from "../transcript/TranscriptView";
import { PermissionHelp } from "./PermissionHelp";
import { ProviderBadge } from "./ProviderBadge";
import { SessionControls } from "./SessionControls";
import type { SessionStore } from "./sessionStore";

export type SessionsClient = Pick<
  IpcClient,
  | "audioSourcesList"
  | "audioPermissionHelp"
  | "settingsGet"
  | "settingsUpdate"
  | "sessionCreate"
  | "sessionStart"
  | "sessionCancelStart"
  | "sessionStop"
  | "recordSetStatus"
>;

type SessionsPageProps = {
  store: SessionStore;
  defaultLanguage: SessionLanguage;
  client?: SessionsClient;
};

/**
 * Create a session, control its capture (US1), follow its transcript (US2) and review the
 * records proposed by analysis (US3).
 */
export function SessionsPage({
  store,
  defaultLanguage,
  client = ipc,
}: SessionsPageProps) {
  const { t } = useTranslation();
  const [sources, setSources] = useState<AudioSourceDto[] | null>(null);
  const [consentDismissed, setConsentDismissed] = useState(false);
  const [askingConsent, setAskingConsent] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<IpcError | null>(null);
  const session = store.state.session;
  const entries = store.state.transcript.entries;
  const segmentNumbers = useMemo(() => {
    const numbers = new Map<string, number>();
    for (const entry of entries)
      if (entry.kind === "final")
        numbers.set(entry.segment.id, numbers.size + 1);
    return numbers;
  }, [entries]);

  useEffect(() => {
    let active = true;
    client.audioSourcesList().then(
      (list) => active && setSources(list),
      (failure: unknown) => active && setError(toIpcError(failure)),
    );
    client.settingsGet().then(
      (settings) =>
        active && setConsentDismissed(settings.consentReminderDismissed),
      () => undefined,
    );
    return () => {
      active = false;
    };
  }, [client]);

  async function run(command: () => Promise<SessionDto>) {
    setBusy(true);
    setError(null);
    try {
      store.track(await command());
    } catch (failure) {
      setError(toIpcError(failure));
    } finally {
      setBusy(false);
    }
  }

  function create(request: SessionCreateDto) {
    void run(() => client.sessionCreate(request));
  }

  function start(target: SessionDto, consentAcknowledged: boolean) {
    void run(() =>
      client.sessionStart({ sessionId: target.id, consentAcknowledged }),
    );
  }

  async function confirmConsent(target: SessionDto, dontShowAgain: boolean) {
    setAskingConsent(false);
    if (dontShowAgain) {
      try {
        await client.settingsUpdate({ consentReminderDismissed: true });
        setConsentDismissed(true);
      } catch {
        // The reminder simply shows again next time.
      }
    }
    start(target, true);
  }

  const permissionProblem =
    error?.code === "permission_denied" ||
    (session?.state === "failed" && session.failure === "permission_denied");

  return (
    <section aria-labelledby="sessions-title">
      <h1 id="sessions-title">{t("sessions.title")}</h1>
      {store.state.storageErrorKey && (
        <p role="alert">{t(store.state.storageErrorKey)}</p>
      )}
      {!session ? (
        <>
          {error && <p role="alert">{t(error.messageKey)}</p>}
          {sources ? (
            <NewSessionForm
              sources={sources}
              defaultLanguage={defaultLanguage}
              busy={busy}
              onCreate={create}
            />
          ) : (
            !error && <p>{t("app.loading")}</p>
          )}
        </>
      ) : (
        <>
          <SessionControls
            session={session}
            busy={busy}
            error={error}
            onStart={() =>
              consentDismissed ? start(session, false) : setAskingConsent(true)
            }
            onCancelStart={() =>
              void run(() => client.sessionCancelStart(session.id))
            }
            onStop={() => void run(() => client.sessionStop(session.id))}
          />
          <ProviderBadge providers={store.state.providers} />
          {session.state !== "idle" && (
            <div className="session-workspace">
              <TranscriptView
                transcript={store.state.transcript}
                showSources={session.sources.length > 1}
              />
              <RecordsPanel
                records={store.state.records}
                segmentNumbers={segmentNumbers}
                client={client}
                onUpdated={store.updateRecord}
              />
            </div>
          )}
          {permissionProblem &&
            session.sources.map((kind) => (
              <PermissionHelp key={kind} kind={kind} client={client} />
            ))}
          {(session.state === "ended" || session.state === "failed") && (
            <button
              type="button"
              onClick={() => {
                setError(null);
                store.clear();
              }}
            >
              {t("sessions.newAnother")}
            </button>
          )}
          {askingConsent && (
            <ConsentReminderDialog
              onConfirm={(dontShowAgain) =>
                void confirmConsent(session, dontShowAgain)
              }
              onCancel={() => setAskingConsent(false)}
            />
          )}
        </>
      )}
    </section>
  );
}
