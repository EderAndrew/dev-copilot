import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { ipc, toIpcError, type IpcClient } from "../../ipc/client";
import type { IpcError, LocalModelDto, ProviderDto } from "../../ipc/types";

export type LocalModelsClient = Pick<
  IpcClient,
  | "localModelList"
  | "localModelDownload"
  | "providerList"
  | "providerUpsert"
  | "providerSetDefault"
>;

type Progress = { received: number; total: number };

const MB = 1024 * 1024;

function without<T>(record: Record<string, T>, key: string): Record<string, T> {
  const copy = { ...record };
  delete copy[key];
  return copy;
}

/**
 * Local transcription models (research R4): the pinned catalog with origin and size. A model
 * is downloaded only when the user asks, and then chosen as the local transcription provider.
 */
export function LocalModels({ client = ipc }: { client?: LocalModelsClient }) {
  const { t, i18n } = useTranslation();
  const [models, setModels] = useState<LocalModelDto[] | null>(null);
  const [providers, setProviders] = useState<ProviderDto[]>([]);
  const [progress, setProgress] = useState<Record<string, Progress>>({});
  const [failures, setFailures] = useState<Record<string, string>>({});
  const [error, setError] = useState<IpcError | null>(null);

  const load = useCallback(
    () => Promise.all([client.localModelList(), client.providerList()]),
    [client],
  );

  const reload = useCallback(async () => {
    try {
      const [list, configured] = await load();
      setModels(list);
      setProviders(configured);
    } catch (failure) {
      setError(toIpcError(failure));
    }
  }, [load]);

  useEffect(() => {
    let active = true;
    load().then(
      ([list, configured]) => {
        if (!active) return;
        setModels(list);
        setProviders(configured);
      },
      (failure: unknown) => active && setError(toIpcError(failure)),
    );
    return () => {
      active = false;
    };
  }, [load]);

  const whisper =
    providers.find((p) => p.adapter === "whisper_local" && p.isDefault) ??
    providers.find((p) => p.adapter === "whisper_local");

  async function download(model: LocalModelDto) {
    setError(null);
    setFailures((all) => without(all, model.id));
    setProgress((all) => ({
      ...all,
      [model.id]: { received: 0, total: model.sizeBytes },
    }));
    try {
      await client.localModelDownload(model.id, (event) => {
        if (event.type === "progress") {
          setProgress((all) => ({
            ...all,
            [model.id]: {
              received: event.receivedBytes,
              total: event.totalBytes,
            },
          }));
          return;
        }
        setProgress((all) => without(all, model.id));
        if (event.type === "failed")
          setFailures((all) => ({ ...all, [model.id]: event.messageKey }));
        void reload();
      });
    } catch (failure) {
      setProgress((all) => without(all, model.id));
      setError(toIpcError(failure));
    }
  }

  async function use(model: LocalModelDto) {
    setError(null);
    try {
      const provider = await client.providerUpsert({
        id: whisper?.id,
        role: "transcription",
        adapter: "whisper_local",
        model: model.id,
        dataCategories: ["audio"],
      });
      await client.providerSetDefault({
        role: "transcription",
        providerId: provider.id,
      });
      await reload();
    } catch (failure) {
      setError(toIpcError(failure));
    }
  }

  const size = (bytes: number) =>
    t("models.size", {
      size: new Intl.NumberFormat(i18n.language, {
        maximumFractionDigits: 0,
      }).format(bytes / MB),
    });

  return (
    <section aria-labelledby="models-title">
      <h2 id="models-title">{t("models.title")}</h2>
      <p>{t("models.help")}</p>
      {error && <p role="alert">{t(error.messageKey)}</p>}
      {!models ? (
        !error && <p>{t("app.loading")}</p>
      ) : (
        <ul className="model-list">
          {models.map((model) => {
            const running = progress[model.id];
            const inUse = whisper?.model === model.id;
            const nameId = `model-${model.id}`;
            return (
              <li
                key={model.id}
                className="model-item"
                aria-labelledby={nameId}
              >
                <strong id={nameId}>{model.displayName}</strong>{" "}
                <span>{size(model.sizeBytes)}</span>{" "}
                <span>{t("models.origin", { origin: model.origin })}</span>{" "}
                {inUse && <span>{t("models.inUse")}</span>}
                {running || model.downloading ? (
                  <label>
                    {t("models.downloading")}{" "}
                    <progress
                      max={running?.total ?? model.sizeBytes}
                      value={running?.received ?? 0}
                    />
                  </label>
                ) : model.downloaded ? (
                  !inUse && (
                    <button type="button" onClick={() => void use(model)}>
                      {t("models.use", { name: model.displayName })}
                    </button>
                  )
                ) : (
                  <button type="button" onClick={() => void download(model)}>
                    {t("models.download", { name: model.displayName })}
                  </button>
                )}
                {failures[model.id] && (
                  <p role="alert">{t(failures[model.id])}</p>
                )}
              </li>
            );
          })}
        </ul>
      )}
    </section>
  );
}
