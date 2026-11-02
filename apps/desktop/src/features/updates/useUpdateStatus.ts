import { useCallback, useEffect, useState } from "react";

import { toIpcError, type IpcClient } from "../../ipc/client";
import type { IpcError, UpdateStatusDto } from "../../ipc/types";

export type UpdatesClient = Pick<
  IpcClient,
  | "subscribeUpdates"
  | "updateCheck"
  | "updateInstall"
  | "updateCancel"
  | "updateNoticeAcknowledge"
>;

export type UpdateControls = {
  /** `null` until the core answered, or when updates are unavailable (no storage). */
  status: UpdateStatusDto | null;
  /** Last command error (e.g. `version_mismatch`), cleared by the next command. */
  error: IpcError | null;
  check: () => void;
  install: (version: string) => void;
  cancel: () => void;
  acknowledge: () => void;
};

/**
 * Mirrors the core's update state (specs/002 contracts/ipc-updates.md). Presentation only:
 * every rule (gates, confirmation of the checked version, session blocking) lives in the core.
 */
export function useUpdateStatus(client: UpdatesClient): UpdateControls {
  const [status, setStatus] = useState<UpdateStatusDto | null>(null);
  const [error, setError] = useState<IpcError | null>(null);

  useEffect(() => {
    let active = true;
    client
      .subscribeUpdates((next) => {
        if (active) setStatus(next);
      })
      .then(
        (current) => {
          if (active) setStatus((previous) => previous ?? current);
        },
        // Without storage there is no update service; the banner simply stays hidden.
        () => undefined,
      );
    return () => {
      active = false;
    };
  }, [client]);

  const run = useCallback(
    (command: () => Promise<UpdateStatusDto | unknown>, applies: boolean) => {
      setError(null);
      command().then(
        (result) => {
          if (applies) setStatus(result as UpdateStatusDto);
        },
        (failure: unknown) => setError(toIpcError(failure)),
      );
    },
    [],
  );

  return {
    status,
    error,
    check: () => run(() => client.updateCheck(), true),
    install: (version) => run(() => client.updateInstall(version), true),
    cancel: () => run(() => client.updateCancel(), true),
    // The new state arrives on the channel; the command returns settings.
    acknowledge: () => run(() => client.updateNoticeAcknowledge(), false),
  };
}
