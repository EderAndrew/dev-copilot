import { useId, useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";

import { toIpcError, type IpcClient } from "../../ipc/client";
import type {
  DataCategory,
  IpcError,
  ProviderAdapterKind,
  ProviderDto,
  ProviderRole,
} from "../../ipc/types";

export type ProviderFormClient = Pick<
  IpcClient,
  "providerUpsert" | "providerSetSecret"
>;

/** Adapters configured here; `whisper_local` is set up from the model list. */
export const FORM_ADAPTERS: readonly ProviderAdapterKind[] = [
  "anthropic",
  "openai_compatible_chat",
  "openai_compatible_stt",
  "ollama",
];

const ROLE: Record<ProviderAdapterKind, ProviderRole> = {
  whisper_local: "transcription",
  openai_compatible_stt: "transcription",
  anthropic: "analysis",
  openai_compatible_chat: "analysis",
  ollama: "analysis",
};

const CATEGORIES: Record<ProviderRole, DataCategory[]> = {
  transcription: ["audio"],
  analysis: ["transcript", "records", "questions"],
};

/** Suggested values; the user can change all of them. */
const DEFAULTS: Record<
  ProviderAdapterKind,
  { endpoint: string; model: string }
> = {
  whisper_local: { endpoint: "", model: "" },
  anthropic: { endpoint: "https://api.anthropic.com", model: "claude-opus-5-5" },
  openai_compatible_chat: { endpoint: "https://api.openai.com/v1", model: "" },
  openai_compatible_stt: {
    endpoint: "https://api.openai.com/v1",
    model: "whisper-1",
  },
  ollama: { endpoint: "http://127.0.0.1:11434", model: "llama3.1" },
};

const isRemote = (adapter: ProviderAdapterKind) =>
  adapter !== "ollama" && adapter !== "whisper_local";

type ProviderFormProps = {
  /** Provider being edited; a new one is created without it. */
  provider?: ProviderDto;
  client: ProviderFormClient;
  onSaved: (provider: ProviderDto) => void;
  onCancel: () => void;
};

/**
 * Configures a provider (US7). Remote endpoints must be `https`; the core checks again. The
 * API key goes straight to the OS credential store and is never shown back. Changing a remote
 * provider's endpoint or data categories requires a new authorization.
 */
export function ProviderForm({
  provider,
  client,
  onSaved,
  onCancel,
}: ProviderFormProps) {
  const { t } = useTranslation();
  const ids = useId();
  const initialAdapter = provider?.adapter ?? "anthropic";
  const [adapter, setAdapter] = useState<ProviderAdapterKind>(initialAdapter);
  const [endpoint, setEndpoint] = useState(
    provider?.endpoint ?? DEFAULTS[initialAdapter].endpoint,
  );
  const [model, setModel] = useState(
    provider?.model ?? DEFAULTS[initialAdapter].model,
  );
  const [categories, setCategories] = useState<DataCategory[]>(
    provider?.dataCategories ?? CATEGORIES[ROLE[initialAdapter]],
  );
  const [secret, setSecret] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<IpcError | null>(null);
  const role = ROLE[adapter];
  const remote = isRemote(adapter);

  function changeAdapter(next: ProviderAdapterKind) {
    setAdapter(next);
    setEndpoint(DEFAULTS[next].endpoint);
    setModel(DEFAULTS[next].model);
    setCategories(CATEGORIES[ROLE[next]]);
  }

  function toggle(category: DataCategory, checked: boolean) {
    setCategories((current) =>
      checked
        ? CATEGORIES[role].filter((c) => c === category || current.includes(c))
        : current.filter((c) => c !== category),
    );
  }

  async function submit(event: FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError(null);
    try {
      let saved = await client.providerUpsert({
        id: provider?.id,
        role,
        adapter,
        endpoint: endpoint.trim(),
        model: model.trim(),
        dataCategories: categories,
      });
      if (remote && secret.trim()) {
        await client.providerSetSecret(saved.id, secret.trim());
        saved = { ...saved, hasCredential: true };
        setSecret("");
      }
      onSaved(saved);
    } catch (failure) {
      setError(toIpcError(failure));
      setBusy(false);
    }
  }

  const field = error?.details?.field;

  return (
    <form
      className="provider-form"
      aria-labelledby={`${ids}-title`}
      onSubmit={(event) => void submit(event)}
    >
      <h3 id={`${ids}-title`}>
        {provider ? t("providers.form.editTitle") : t("providers.form.newTitle")}
      </h3>
      <label>
        {t("providers.form.adapter")}
        <select
          value={adapter}
          disabled={Boolean(provider)}
          onChange={(event) =>
            changeAdapter(event.target.value as ProviderAdapterKind)
          }
        >
          {FORM_ADAPTERS.map((kind) => (
            <option key={kind} value={kind}>
              {t(`providers.adapter.${kind}`)}
            </option>
          ))}
        </select>
      </label>
      <p>
        {remote ? t("providers.form.remoteHint") : t("providers.form.localHint")}
      </p>
      {adapter === "openai_compatible_stt" && (
        <p>{t("providers.form.finalsOnly")}</p>
      )}
      <label>
        {t("providers.form.endpoint")}
        <input
          type="url"
          required
          value={endpoint}
          aria-invalid={field === "endpoint" || undefined}
          onChange={(event) => setEndpoint(event.target.value)}
        />
      </label>
      <label>
        {t("providers.form.model")}
        <input
          type="text"
          required
          maxLength={200}
          value={model}
          aria-invalid={field === "model" || undefined}
          onChange={(event) => setModel(event.target.value)}
        />
      </label>
      <fieldset>
        <legend>{t("providers.form.categories")}</legend>
        {CATEGORIES[role].map((category) => (
          <label key={category}>
            <input
              type="checkbox"
              checked={categories.includes(category)}
              onChange={(event) => toggle(category, event.target.checked)}
            />
            {t(`providers.category.${category}`)}
          </label>
        ))}
      </fieldset>
      {remote && (
        <label>
          {provider?.hasCredential
            ? t("providers.form.secretReplace")
            : t("providers.form.secret")}
          <input
            type="password"
            autoComplete="off"
            value={secret}
            aria-invalid={field === "secret" || undefined}
            onChange={(event) => setSecret(event.target.value)}
          />
        </label>
      )}
      {remote && provider && <p>{t("providers.form.reauthorizeHint")}</p>}
      {error && <p role="alert">{t(error.messageKey)}</p>}
      <div className="dialog-actions">
        <button type="submit" disabled={busy || categories.length === 0}>
          {t("providers.form.save")}
        </button>
        <button type="button" disabled={busy} onClick={onCancel}>
          {t("providers.form.cancel")}
        </button>
      </div>
    </form>
  );
}
