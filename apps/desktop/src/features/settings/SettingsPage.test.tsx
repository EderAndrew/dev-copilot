import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { axe } from "vitest-axe";

import type { SettingsDto } from "../../ipc/types";
import { SettingsPage } from "./SettingsPage";

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
  updateCheckEnabled: true,
  updateNoticeAcknowledged: false,
};

function client(
  overrides: Partial<{ settingsGet: () => Promise<SettingsDto> }> = {},
) {
  return {
    settingsGet: vi.fn(() => Promise.resolve(settings)),
    settingsUpdate: vi.fn((update: Partial<SettingsDto>) =>
      Promise.resolve({ ...settings, ...update } as SettingsDto),
    ),
    ...overrides,
  };
}

describe("SettingsPage", () => {
  it("loads, edits and saves analysis limits", async () => {
    const user = userEvent.setup();
    const api = client();
    const onLanguageChange = vi.fn();
    render(<SettingsPage client={api} onLanguageChange={onLanguageChange} />);

    const field = await screen.findByLabelText("Máximo de chamadas por hora");
    await user.clear(field);
    await user.type(field, "20");
    await user.selectOptions(
      screen.getByLabelText("Idioma da interface"),
      "en",
    );
    await user.click(screen.getByRole("button", { name: "Salvar" }));

    expect(api.settingsUpdate).toHaveBeenCalledWith({
      uiLanguage: "en",
      analysisLimits: { ...settings.analysisLimits, maxCallsPerHour: 20 },
    });
    expect(
      (await screen.findByText("Configurações salvas.")).getAttribute("role"),
    ).toBe("status");
    expect(onLanguageChange).toHaveBeenCalledWith("en");
  });

  it("flags non-numeric input without calling the core", async () => {
    const user = userEvent.setup();
    const api = client();
    render(<SettingsPage client={api} />);

    const field = await screen.findByLabelText(
      "Intervalo mínimo entre análises (segundos)",
    );
    await user.clear(field);
    await user.type(field, "abc");
    await user.click(screen.getByRole("button", { name: "Salvar" }));

    expect(field.getAttribute("aria-invalid")).toBe("true");
    expect(api.settingsUpdate).not.toHaveBeenCalled();
  });

  it("shows the translated core error", async () => {
    const api = client({
      settingsGet: () =>
        Promise.reject({
          code: "secret_store_unavailable",
          messageKey: "errors.secretStoreUnavailable",
        }),
    });
    render(<SettingsPage client={api} />);
    expect((await screen.findByRole("alert")).textContent).toContain(
      "O cofre de credenciais do sistema não está disponível.",
    );
  });

  it("shows the updates section with last check and destinations", async () => {
    const api = client({
      settingsGet: () =>
        Promise.resolve({
          ...settings,
          updateNoticeAcknowledged: true,
          updateLastCheckedAt: "2026-11-01T12:00:00Z",
        }),
    });
    render(
      <SettingsPage
        client={api}
        updateStatus={{
          state: "up_to_date",
          lastCheckedAt: "2026-11-01T12:00:00Z",
        }}
      />,
    );
    expect(
      await screen.findByRole("group", { name: "Atualizações" }),
    ).toBeTruthy();
    expect(screen.getByText(/^Última verificação: /)).toBeTruthy();
    expect(screen.getByText(/objects\.githubusercontent\.com/)).toBeTruthy();
    expect(
      screen.getByText("Você já está na versão mais recente."),
    ).toBeTruthy();
    expect(
      (
        screen.getByRole("button", {
          name: "Verificar agora",
        }) as HTMLButtonElement
      ).disabled,
    ).toBe(false);
  });

  it("disables checking until the notice is dismissed", async () => {
    render(
      <SettingsPage
        client={client()}
        updateStatus={{ state: "notice_pending" }}
      />,
    );
    const button = await screen.findByRole("button", {
      name: "Verificar agora",
    });
    expect((button as HTMLButtonElement).disabled).toBe(true);
    expect(screen.getByText("Nenhuma verificação feita ainda.")).toBeTruthy();
  });

  it("has no axe violations", async () => {
    const { container } = render(<SettingsPage client={client()} />);
    await screen.findByLabelText("Máximo de chamadas por hora");
    const results = await axe(container);
    expect(results.violations).toEqual([]);
  });
});
