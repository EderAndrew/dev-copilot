import { useEffect, useId, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { toIpcError, type IpcClient } from "../../ipc/client";
import type {
  AuthorizationPreviewDto,
  IpcError,
  ProviderAuthorizationDto,
} from "../../ipc/types";

export type AuthorizationClient = Pick<
  IpcClient,
  "providerAuthorizationPreview" | "providerAuthorize"
>;

type AuthorizationDialogProps = {
  providerId: string;
  client: AuthorizationClient;
  onAuthorized: (authorization: ProviderAuthorizationDto) => void;
  onCancel: () => void;
};

/**
 * Informed authorization of a remote provider (US7, FR-046): shows the destination, the data
 * categories and the retention notice exactly as the core will record them, and authorizes
 * only on explicit acceptance. Nothing is sent before that; declining sends nothing. Focus
 * starts on Decline so a stray Enter never authorizes.
 */
export function AuthorizationDialog({
  providerId,
  client,
  onAuthorized,
  onCancel,
}: AuthorizationDialogProps) {
  const { t } = useTranslation();
  const ids = useId();
  const [preview, setPreview] = useState<AuthorizationPreviewDto | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<IpcError | null>(null);
  const declineRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    declineRef.current?.focus();
  }, []);

  useEffect(() => {
    let active = true;
    client.providerAuthorizationPreview(providerId).then(
      (loaded) => active && setPreview(loaded),
      (failure: unknown) => active && setError(toIpcError(failure)),
    );
    return () => {
      active = false;
    };
  }, [client, providerId]);

  async function accept() {
    setBusy(true);
    setError(null);
    try {
      onAuthorized(await client.providerAuthorize(providerId));
    } catch (failure) {
      setError(toIpcError(failure));
      setBusy(false);
    }
  }

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-labelledby={`${ids}-title`}
      aria-describedby={`${ids}-body`}
      className="dialog"
      onKeyDown={(event) => {
        if (event.key === "Escape" && !busy) onCancel();
      }}
    >
      <h2 id={`${ids}-title`}>
        {preview
          ? t("authorization.title", { destination: preview.destination })
          : t("authorization.titleLoading")}
      </h2>
      <div id={`${ids}-body`}>
        <p>{t("authorization.nothingSent")}</p>
        {preview ? (
          <dl className="authorization-details">
            <dt>{t("authorization.destination")}</dt>
            <dd>{preview.destination}</dd>
            <dt>{t("authorization.categories")}</dt>
            <dd>
              <ul>
                {preview.dataCategories.map((category) => (
                  <li key={category}>{t(`providers.category.${category}`)}</li>
                ))}
              </ul>
            </dd>
            <dt>{t("authorization.retention")}</dt>
            <dd>{preview.retentionNotice}</dd>
          </dl>
        ) : (
          !error && <p>{t("app.loading")}</p>
        )}
        <p>{t("authorization.revocable")}</p>
      </div>
      {error && <p role="alert">{t(error.messageKey)}</p>}
      <div className="dialog-actions">
        <button
          type="button"
          disabled={!preview || busy}
          onClick={() => void accept()}
        >
          {t("authorization.accept")}
        </button>
        <button
          ref={declineRef}
          type="button"
          disabled={busy}
          onClick={onCancel}
        >
          {t("authorization.decline")}
        </button>
      </div>
    </div>
  );
}
