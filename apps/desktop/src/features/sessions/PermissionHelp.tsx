import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { ipc, type IpcClient } from "../../ipc/client";
import type { AudioSourceKind } from "../../ipc/types";

type PermissionHelpProps = {
  kind: AudioSourceKind;
  client?: Pick<IpcClient, "audioPermissionHelp">;
};

/** How to grant the capture permission in this OS's settings (FR-010). */
export function PermissionHelp({ kind, client = ipc }: PermissionHelpProps) {
  const { t } = useTranslation();
  const [messageKey, setMessageKey] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    client.audioPermissionHelp(kind).then(
      (help) => active && setMessageKey(help.messageKey),
      () => active && setMessageKey("permissionHelp.generic"),
    );
    return () => {
      active = false;
    };
  }, [client, kind]);

  if (!messageKey) return null;
  return (
    <aside
      className="permission-help"
      aria-labelledby={`permission-help-${kind}`}
    >
      <h3 id={`permission-help-${kind}`}>{t("permissionHelp.title")}</h3>
      <p>{t(messageKey)}</p>
    </aside>
  );
}
