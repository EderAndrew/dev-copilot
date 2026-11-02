import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { ipc, toIpcError, type IpcClient } from "../../ipc/client";
import type { IpcError, ProviderDto } from "../../ipc/types";
import { AuthorizationDialog } from "./AuthorizationDialog";
import { LocalModels, type LocalModelsClient } from "./LocalModels";
import { ProviderForm } from "./ProviderForm";

export type ProvidersClient = LocalModelsClient &
  Pick<
    IpcClient,
    | "providerSetSecret"
    | "providerAuthorizationPreview"
    | "providerAuthorize"
    | "providerRevoke"
  >;

type Editing = { provider?: ProviderDto } | null;

/**
 * Providers (US7, FR-045–FR-050): local and remote providers per role, the default of each
 * role, and the authorization state of remote providers, which can be authorized after the
 * informed dialog and revoked at any time. Local transcription models are managed below.
 */
export function ProvidersPage({ client = ipc }: { client?: ProvidersClient }) {
  const { t } = useTranslation();
  const [providers, setProviders] = useState<ProviderDto[] | null>(null);
  const [editing, setEditing] = useState<Editing>(null);
  const [authorizing, setAuthorizing] = useState<string | null>(null);
  const [error, setError] = useState<IpcError | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const reload = useCallback(async () => {
    try {
      setProviders(await client.providerList());
    } catch (failure) {
      setError(toIpcError(failure));
    }
  }, [client]);

  useEffect(() => {
    let active = true;
    client.providerList().then(
      (list) => active && setProviders(list),
      (failure: unknown) => active && setError(toIpcError(failure)),
    );
    return () => {
      active = false;
    };
  }, [client]);

  async function act(action: () => Promise<unknown>, done?: string) {
    setError(null);
    setNotice(null);
    try {
      await action();
      if (done) setNotice(done);
      await reload();
    } catch (failure) {
      setError(toIpcError(failure));
    }
  }

  const name = (provider: ProviderDto) =>
    t("providers.name", {
      adapter: t(`providers.adapter.${provider.adapter}`),
      model: provider.model,
    });

  return (
    <section aria-labelledby="providers-title">
      <h1 id="providers-title">{t("providers.title")}</h1>
      <p>{t("providers.help")}</p>
      {error && <p role="alert">{t(error.messageKey)}</p>}
      {notice && <p role="status">{notice}</p>}
      {!providers ? (
        !error && <p>{t("app.loading")}</p>
      ) : providers.length === 0 ? (
        <p>{t("providers.empty")}</p>
      ) : (
        <ul className="provider-list">
          {providers.map((provider) => {
            const nameId = `provider-${provider.id}`;
            const remote = provider.locality === "remote";
            return (
              <li
                key={provider.id}
                className="provider-item"
                aria-labelledby={nameId}
              >
                <strong id={nameId}>{name(provider)}</strong>{" "}
                <span>{t(`providers.role.${provider.role}`)}</span>{" "}
                <span>
                  <span aria-hidden="true">{remote ? "⇅ " : "⌂ "}</span>
                  {t(`providerInUse.locality.${provider.locality}`)}
                </span>{" "}
                {provider.endpoint && <code>{provider.endpoint}</code>}{" "}
                <span>{t(`providers.status.${provider.authorization}`)}</span>{" "}
                {remote && provider.hasCredential && (
                  <span>{t("providers.credentialStored")}</span>
                )}{" "}
                {provider.isDefault && <span>{t("providers.isDefault")}</span>}
                <div className="provider-actions">
                  {!provider.isDefault && (
                    <button
                      type="button"
                      onClick={() =>
                        void act(() =>
                          client.providerSetDefault({
                            role: provider.role,
                            providerId: provider.id,
                          }),
                        )
                      }
                    >
                      {t("providers.makeDefault", { name: name(provider) })}
                    </button>
                  )}
                  {provider.adapter !== "whisper_local" && (
                    <button
                      type="button"
                      onClick={() => setEditing({ provider })}
                    >
                      {t("providers.edit", { name: name(provider) })}
                    </button>
                  )}
                  {remote && provider.authorization !== "authorized" && (
                    <button
                      type="button"
                      onClick={() => setAuthorizing(provider.id)}
                    >
                      {t("providers.authorize", { name: name(provider) })}
                    </button>
                  )}
                  {remote && provider.authorization === "authorized" && (
                    <button
                      type="button"
                      onClick={() =>
                        void act(
                          () => client.providerRevoke(provider.id),
                          t("providers.revoked", { name: name(provider) }),
                        )
                      }
                    >
                      {t("providers.revoke", { name: name(provider) })}
                    </button>
                  )}
                </div>
              </li>
            );
          })}
        </ul>
      )}
      {editing ? (
        <ProviderForm
          provider={editing.provider}
          client={client}
          onSaved={() => {
            setEditing(null);
            void reload();
          }}
          onCancel={() => setEditing(null)}
        />
      ) : (
        <button type="button" onClick={() => setEditing({})}>
          {t("providers.add")}
        </button>
      )}
      {authorizing && (
        <AuthorizationDialog
          providerId={authorizing}
          client={client}
          onAuthorized={() => {
            setAuthorizing(null);
            setNotice(t("providers.authorized"));
            void reload();
          }}
          onCancel={() => setAuthorizing(null)}
        />
      )}
      <LocalModels client={client} />
    </section>
  );
}
