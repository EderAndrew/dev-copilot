// Typed wrappers over Tauri `invoke`. Every rejection is normalized to an `IpcError`, so UI code
// only ever handles contract error codes.
import { Channel, invoke } from "@tauri-apps/api/core";

import type {
  AudioSourceDto,
  AudioSourceKind,
  ErrorCode,
  IpcError,
  PermissionHelpDto,
  PermissionState,
  ProviderDto,
  ProviderSetDefaultDto,
  ProviderUpsertDto,
  SessionCreateDto,
  SessionDto,
  SessionEvent,
  SessionStartDto,
  SettingsDto,
  SettingsUpdateDto,
} from "./types";

const ERROR_CODES: ReadonlySet<string> = new Set<ErrorCode>([
  "invalid_input",
  "not_found",
  "invalid_state",
  "permission_denied",
  "source_unavailable",
  "no_transcriber",
  "secret_store_unavailable",
  "authorization_required",
  "provider_error",
  "storage_error",
  "internal",
]);

export function isIpcError(value: unknown): value is IpcError {
  if (typeof value !== "object" || value === null) return false;
  const candidate = value as Record<string, unknown>;
  return (
    typeof candidate.code === "string" &&
    ERROR_CODES.has(candidate.code) &&
    typeof candidate.messageKey === "string"
  );
}

export function toIpcError(value: unknown): IpcError {
  if (isIpcError(value)) return value;
  // Deserialization failures and runtime errors arrive as strings; their text is not shown.
  return { code: "internal", messageKey: "errors.internal" };
}

async function call<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (error) {
    throw toIpcError(error);
  }
}

export const ipc = {
  settingsGet: () => call<SettingsDto>("settings_get"),
  settingsUpdate: (request: SettingsUpdateDto) =>
    call<SettingsDto>("settings_update", { request }),
  providerList: () => call<ProviderDto[]>("provider_list"),
  providerUpsert: (request: ProviderUpsertDto) =>
    call<ProviderDto>("provider_upsert", { request }),
  providerSetDefault: (request: ProviderSetDefaultDto) =>
    call<void>("provider_set_default", { request }),
  sessionCreate: (request: SessionCreateDto) =>
    call<SessionDto>("session_create", { request }),
  sessionStart: (request: SessionStartDto) =>
    call<SessionDto>("session_start", { request }),
  sessionCancelStart: (sessionId: string) =>
    call<SessionDto>("session_cancel_start", { request: { sessionId } }),
  sessionStop: (sessionId: string) =>
    call<SessionDto>("session_stop", { request: { sessionId } }),
  /** Routes the session's events to `onEvent`; resolves with the current session. */
  sessionSubscribe: (
    sessionId: string,
    onEvent: (event: SessionEvent) => void,
  ) => {
    const channel = new Channel<SessionEvent>();
    channel.onmessage = onEvent;
    return call<SessionDto>("session_subscribe", {
      request: { sessionId },
      channel,
    });
  },
  audioSourcesList: () => call<AudioSourceDto[]>("audio_sources_list"),
  audioPermissionRequest: (kind: AudioSourceKind) =>
    call<PermissionState>("audio_permission_request", { request: { kind } }),
  audioPermissionHelp: (kind: AudioSourceKind) =>
    call<PermissionHelpDto>("audio_permission_help", { request: { kind } }),
};

export type IpcClient = typeof ipc;
