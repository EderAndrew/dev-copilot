import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { axe } from "vitest-axe";

import type {
  AudioSourceDto,
  SessionDto,
  SessionEvent,
  SettingsDto,
} from "../../ipc/types";
import { CaptureIndicator } from "./CaptureIndicator";
import { SessionsPage } from "./SessionsPage";
import { useSessionStore } from "./sessionStore";

const sources: AudioSourceDto[] = [
  { kind: "microphone", availability: "available", permission: "granted" },
  {
    kind: "system",
    availability: "unavailable",
    unavailableReason: "not_validated_on_platform",
    permission: "not_requested",
  },
];

const settings: SettingsDto = {
  uiLanguage: "pt-BR",
  consentReminderDismissed: false,
  analysisLimits: {
    triggerFinals: 6,
    triggerSpeechSecs: 90,
    minIntervalSecs: 30,
    maxCallsPerHour: 40,
    maxTokensPerSession: 200000,
  },
};

const idle: SessionDto = {
  id: "s1",
  title: "Daily",
  language: "pt-BR",
  state: "idle",
  sources: ["microphone"],
  createdAt: 0,
  activeDurationMs: 0,
};

/** The capture indicator's live region (the records panel has its own). */
function captureStatus(): HTMLElement {
  const status = screen
    .getAllByRole("status")
    .find((element) => element.querySelector("[aria-hidden='true']"));
  if (!status) throw new Error("capture indicator not found");
  return status;
}

function setup(overrides: Partial<SettingsDto> = {}) {
  let emit: (event: SessionEvent) => void = () => undefined;
  const client = {
    audioSourcesList: vi.fn(() => Promise.resolve(sources)),
    audioPermissionHelp: vi.fn(() =>
      Promise.resolve({ messageKey: "permissionHelp.macos.microphone" }),
    ),
    settingsGet: vi.fn(() => Promise.resolve({ ...settings, ...overrides })),
    settingsUpdate: vi.fn(() => Promise.resolve(settings)),
    sessionCreate: vi.fn(() => Promise.resolve(idle)),
    recordSetStatus: vi.fn(() => Promise.reject(new Error("unused"))),
    sessionStart: vi.fn(() =>
      Promise.resolve({ ...idle, state: "starting" as const }),
    ),
    sessionCancelStart: vi.fn(() => Promise.resolve(idle)),
    sessionStop: vi.fn(() =>
      Promise.resolve({ ...idle, state: "stopping" as const }),
    ),
    sessionSubscribe: vi.fn(
      (_id: string, onEvent: (event: SessionEvent) => void) => {
        emit = onEvent;
        return Promise.resolve(idle);
      },
    ),
  };
  function Harness() {
    const store = useSessionStore(client);
    return (
      <>
        <CaptureIndicator indicator={store.state.indicator} />
        <SessionsPage store={store} defaultLanguage="pt-BR" client={client} />
      </>
    );
  }
  const utils = render(<Harness />);
  return { client, emit: (e: SessionEvent) => act(() => emit(e)), ...utils };
}

describe("SessionsPage", () => {
  it("creates, confirms consent, starts and stops a session", async () => {
    const user = userEvent.setup();
    const { client, emit } = setup();

    expect(
      await screen.findByText("Ainda não validado nesta plataforma."),
    ).toBeTruthy();
    expect(
      (
        screen.getByRole("checkbox", {
          name: "Áudio do sistema",
        }) as HTMLInputElement
      ).disabled,
    ).toBe(true);
    await user.type(screen.getByLabelText("Título"), "Daily");
    await user.click(screen.getByRole("button", { name: "Criar sessão" }));
    expect(client.sessionCreate).toHaveBeenCalledWith({
      title: "Daily",
      language: "pt-BR",
      sources: ["microphone"],
    });

    await user.click(
      await screen.findByRole("button", { name: "Iniciar captura" }),
    );
    // The reminder must be confirmed before anything starts (FR-012).
    expect(screen.getByRole("dialog")).toBeTruthy();
    expect(client.sessionStart).not.toHaveBeenCalled();
    await user.click(screen.getByLabelText("Não mostrar novamente"));
    await user.click(
      screen.getByRole("button", { name: "Entendi, iniciar captura" }),
    );
    expect(client.settingsUpdate).toHaveBeenCalledWith({
      consentReminderDismissed: true,
    });
    expect(client.sessionStart).toHaveBeenCalledWith({
      sessionId: "s1",
      consentAcknowledged: true,
    });

    emit({ type: "state_changed", seq: 1, state: "starting" });
    emit({ type: "state_changed", seq: 2, state: "capturing" });
    emit({
      type: "capture_indicator",
      seq: 3,
      active: true,
      paused: false,
      sources: ["microphone"],
    });
    expect(captureStatus().textContent).toContain("Capturando: Microfone");

    await user.click(screen.getByRole("button", { name: "Parar captura" }));
    expect(client.sessionStop).toHaveBeenCalledWith("s1");
    emit({
      type: "capture_indicator",
      seq: 4,
      active: false,
      paused: false,
      sources: [],
    });
    emit({ type: "state_changed", seq: 5, state: "stopping" });
    emit({ type: "state_changed", seq: 6, state: "ended" });
    expect(captureStatus().textContent).toContain("Sem captura");
    expect(screen.getByText("encerrada")).toBeTruthy();
  });

  it("skips the reminder once dismissed", async () => {
    const user = userEvent.setup();
    const { client } = setup({ consentReminderDismissed: true });
    await user.type(await screen.findByLabelText("Título"), "Daily");
    await user.click(screen.getByRole("button", { name: "Criar sessão" }));
    await user.click(
      await screen.findByRole("button", { name: "Iniciar captura" }),
    );
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(client.sessionStart).toHaveBeenCalledWith({
      sessionId: "s1",
      consentAcknowledged: false,
    });
  });

  it("explains how to grant permission when it is denied", async () => {
    const user = userEvent.setup();
    const { client } = setup({ consentReminderDismissed: true });
    client.sessionStart.mockRejectedValueOnce({
      code: "permission_denied",
      messageKey: "errors.permissionDenied",
    });
    await user.type(await screen.findByLabelText("Título"), "Daily");
    await user.click(screen.getByRole("button", { name: "Criar sessão" }));
    await user.click(
      await screen.findByRole("button", { name: "Iniciar captura" }),
    );
    expect(
      await screen.findByText(/Privacidade e Segurança › Microfone/),
    ).toBeTruthy();
    expect(screen.getByRole("alert").textContent).toContain(
      "Permissão de captura negada",
    );
    expect(screen.getByText("ociosa")).toBeTruthy();
  });

  it("requires a title before creating", async () => {
    const user = userEvent.setup();
    const { client, container } = setup();
    await user.click(
      await screen.findByRole("button", { name: "Criar sessão" }),
    );
    expect(client.sessionCreate).not.toHaveBeenCalled();
    expect(screen.getByText("Informe um título.")).toBeTruthy();
    expect((await axe(container)).violations).toEqual([]);
  });
});
