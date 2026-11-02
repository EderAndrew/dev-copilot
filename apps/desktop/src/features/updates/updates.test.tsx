import {
  act,
  render,
  renderHook,
  screen,
  waitFor,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { axe } from "vitest-axe";

import type { IpcError, UpdateStatusDto } from "../../ipc/types";
import { SettingsPage } from "../settings/SettingsPage";
import { UpdateBanner } from "./UpdateBanner";
import { UpdateConfirmDialog } from "./UpdateConfirmDialog";
import { UpdateNotice } from "./UpdateNotice";
import { useUpdateStatus, type UpdatesClient } from "./useUpdateStatus";

const available: UpdateStatusDto = {
  state: "available",
  version: "0.2.0",
  notes: "Correções <b>x</b>",
};

function banner(status: UpdateStatusDto | null, error: IpcError | null = null) {
  const onInstall = vi.fn();
  const onCancel = vi.fn();
  const view = render(
    <UpdateBanner
      status={status}
      error={error}
      onInstall={onInstall}
      onCancel={onCancel}
    />,
  );
  return { ...view, onInstall, onCancel };
}

describe("UpdateNotice", () => {
  it("lists the contacted hosts and acknowledges", async () => {
    const user = userEvent.setup();
    const onAcknowledge = vi.fn();
    const { container } = render(
      <UpdateNotice onAcknowledge={onAcknowledge} />,
    );
    const items = screen.getAllByRole("listitem").map((li) => li.textContent);
    expect(items).toEqual([
      expect.stringContaining("github.com"),
      expect.stringContaining("objects.githubusercontent.com"),
    ]);
    await user.click(screen.getByRole("button", { name: "Entendi" }));
    expect(onAcknowledge).toHaveBeenCalledTimes(1);
    expect((await axe(container)).violations).toEqual([]);
  });
});

describe("UpdateBanner", () => {
  it("shows the version and renders notes as literal text", async () => {
    const { container } = banner(available);
    expect(
      screen.getByRole("heading", { name: "Versão 0.2.0 disponível" }),
    ).toBeTruthy();
    expect(screen.getByText("Correções <b>x</b>")).toBeTruthy();
    expect(container.querySelector("b")).toBeNull();
    expect((await axe(container)).violations).toEqual([]);
  });

  it("installs only after confirming the displayed version", async () => {
    const user = userEvent.setup();
    const { onInstall } = banner(available);
    await user.click(screen.getByRole("button", { name: "Instalar…" }));
    expect(onInstall).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "Baixar e instalar" }));
    expect(onInstall).toHaveBeenCalledWith("0.2.0");
  });

  it("announces blocking by a session politely and allows a retry", async () => {
    const user = userEvent.setup();
    const { container, onInstall } = banner({
      state: "blocked_by_session",
      version: "0.2.0",
    });
    const live = screen.getByRole("status");
    expect(live.getAttribute("aria-live")).toBe("polite");
    expect(live.textContent).toContain("espera o fim da sessão");
    await user.click(
      screen.getByRole("button", { name: "Tentar instalar de novo" }),
    );
    expect(onInstall).toHaveBeenCalledWith("0.2.0");
    expect((await axe(container)).violations).toEqual([]);
  });

  it.each([
    ["signature_invalid", "assinatura válida"],
    ["network", "Verifique a conexão"],
    ["download_interrupted", "interrompido"],
  ] as const)("announces the %s failure politely", async (reason, text) => {
    const { container } = banner({ state: "failed", reason });
    const live = screen.getByRole("status");
    expect(live.getAttribute("aria-live")).toBe("polite");
    expect(live.textContent).toContain(text);
    expect((await axe(container)).violations).toEqual([]);
  });

  it("announces command errors", () => {
    banner(available, {
      code: "version_mismatch",
      messageKey: "errors.versionMismatch",
    });
    expect(screen.getByRole("status").textContent).toContain(
      "não é a última verificada",
    );
  });

  it("shows download progress and cancels", async () => {
    const user = userEvent.setup();
    const { container, onCancel } = banner({
      state: "downloading",
      version: "0.2.0",
      receivedBytes: 5,
      totalBytes: 20,
    });
    const progress = screen.getByRole("progressbar", {
      name: "Progresso do download",
    });
    expect(progress.getAttribute("value")).toBe("5");
    expect(progress.getAttribute("max")).toBe("20");
    await user.click(screen.getByRole("button", { name: "Cancelar download" }));
    expect(onCancel).toHaveBeenCalledTimes(1);
    expect((await axe(container)).violations).toEqual([]);
  });

  it("renders nothing visible while idle", () => {
    banner({ state: "idle" });
    expect(screen.queryByRole("button")).toBeNull();
    expect(screen.getByRole("status").textContent).toBe("");
  });
});

describe("UpdateConfirmDialog", () => {
  function dialog() {
    const onConfirm = vi.fn();
    const onCancel = vi.fn();
    const view = render(
      <UpdateConfirmDialog
        version="0.2.0"
        notes={"linha 1\n<script>x</script>"}
        onConfirm={onConfirm}
        onCancel={onCancel}
      />,
    );
    return { ...view, onConfirm, onCancel };
  }

  it("is operable by keyboard only and traps focus", async () => {
    const user = userEvent.setup();
    const { onConfirm } = dialog();
    const cancel = screen.getByRole("button", { name: "Agora não" });
    const confirm = screen.getByRole("button", { name: "Baixar e instalar" });
    expect(document.activeElement).toBe(cancel);
    await user.tab();
    expect(document.activeElement).toBe(confirm);
    await user.tab({ shift: true });
    expect(document.activeElement).toBe(cancel);
    await user.tab({ shift: true });
    expect(document.activeElement).toBe(confirm);
    await user.keyboard("{Enter}");
    expect(onConfirm).toHaveBeenCalledWith("0.2.0");
  });

  it("closes with Escape", async () => {
    const user = userEvent.setup();
    const { onCancel, onConfirm } = dialog();
    await user.keyboard("{Escape}");
    expect(onCancel).toHaveBeenCalledTimes(1);
    expect(onConfirm).not.toHaveBeenCalled();
  });

  it("shows notes as text and has no axe violations", async () => {
    const { container } = dialog();
    expect(container.querySelector("script")).toBeNull();
    expect(screen.getByRole("dialog").textContent).toContain(
      "<script>x</script>",
    );
    expect((await axe(container)).violations).toEqual([]);
  });
});

describe("useUpdateStatus", () => {
  function client(): UpdatesClient & {
    push: (status: UpdateStatusDto) => void;
  } {
    let listener: ((status: UpdateStatusDto) => void) | null = null;
    return {
      push: (status) => listener?.(status),
      subscribeUpdates: vi.fn((onStatus: (s: UpdateStatusDto) => void) => {
        listener = onStatus;
        return Promise.resolve<UpdateStatusDto>({ state: "notice_pending" });
      }),
      updateCheck: vi.fn(() =>
        Promise.resolve<UpdateStatusDto>({ state: "checking" }),
      ),
      updateInstall: vi.fn(() =>
        Promise.reject({
          code: "version_mismatch",
          messageKey: "errors.versionMismatch",
        }),
      ),
      updateCancel: vi.fn(() =>
        Promise.resolve<UpdateStatusDto>({ state: "idle" }),
      ),
      updateNoticeAcknowledge: vi.fn(() => Promise.resolve({} as never)),
    };
  }

  it("follows the channel and reports command errors", async () => {
    const api = client();
    const { result } = renderHook(() => useUpdateStatus(api));
    await waitFor(() =>
      expect(result.current.status).toEqual({ state: "notice_pending" }),
    );
    act(() => api.push(available));
    expect(result.current.status).toEqual(available);
    act(() => result.current.install("0.2.0"));
    await waitFor(() =>
      expect(result.current.error?.code).toBe("version_mismatch"),
    );
    expect(api.updateInstall).toHaveBeenCalledWith("0.2.0");
    act(() => result.current.acknowledge());
    expect(api.updateNoticeAcknowledge).toHaveBeenCalledTimes(1);
  });
});

describe("Settings › Atualizações", () => {
  const settings = {
    uiLanguage: "pt-BR" as const,
    consentReminderDismissed: false,
    analysisLimits: {
      triggerFinals: 6,
      triggerSpeechSecs: 90,
      minIntervalSecs: 30,
      maxCallsPerHour: 40,
      maxTokensPerSession: 200000,
    },
    updateCheckEnabled: true,
    updateNoticeAcknowledged: true,
  };

  it("toggle calls settings_update({ updateCheckEnabled })", async () => {
    const user = userEvent.setup();
    const api = {
      settingsGet: vi.fn(() => Promise.resolve(settings)),
      settingsUpdate: vi.fn(() =>
        Promise.resolve({ ...settings, updateCheckEnabled: false }),
      ),
    };
    const onCheckUpdates = vi.fn();
    render(
      <SettingsPage
        client={api}
        updateStatus={{ state: "idle" }}
        onCheckUpdates={onCheckUpdates}
      />,
    );
    await user.click(
      await screen.findByRole("button", { name: "Verificar agora" }),
    );
    expect(onCheckUpdates).toHaveBeenCalledTimes(1);
    await user.click(
      screen.getByRole("checkbox", {
        name: "Verificar atualizações automaticamente",
      }),
    );
    expect(api.settingsUpdate).toHaveBeenCalledWith({
      updateCheckEnabled: false,
    });
    await waitFor(() =>
      expect(
        (
          screen.getByRole("button", {
            name: "Verificar agora",
          }) as HTMLButtonElement
        ).disabled,
      ).toBe(true),
    );
  });
});
