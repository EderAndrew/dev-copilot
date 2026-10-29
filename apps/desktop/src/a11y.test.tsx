// Accessibility pass (T160, SC-016, FR-053): axe on every page of the real app shell, and the
// main flows — start/stop, history, review, export — driven with the keyboard only. The IPC
// client is replaced by fakes answering with the shared contract fixtures.
import { act, render, screen } from "@testing-library/react";
import userEvent, { type UserEvent } from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { axe } from "vitest-axe";

import plan from "../../../specs/001-meeting-sessions/contracts/fixtures/PlanDto/plan.json";
import detail from "../../../specs/001-meeting-sessions/contracts/fixtures/SessionDetailDto/ended.json";
import settings from "../../../specs/001-meeting-sessions/contracts/fixtures/SettingsDto/default.json";
import type {
  ProviderDto,
  SessionDetailDto,
  SessionDto,
  SessionEvent,
} from "./ipc/types";

const ended = (detail as SessionDetailDto).session;
const idle: SessionDto = {
  ...ended,
  id: "019a2b3c-4d5e-7f60-8a1b-2c3d4e5f6aff",
  title: "Daily",
  state: "idle",
  startedAt: undefined,
  endedAt: undefined,
  activeDurationMs: 0,
};
const remote: ProviderDto = {
  id: "p1",
  role: "analysis",
  adapter: "anthropic",
  locality: "remote",
  endpoint: "https://api.anthropic.com",
  model: "claude-opus-5-5",
  dataCategories: ["transcript"],
  isDefault: true,
  authorization: "authorized",
  hasCredential: true,
};

const state: { emit: (event: SessionEvent) => void } = { emit: () => undefined };

vi.mock("./ipc/client", async (importActual) => {
  const actual = await importActual<typeof import("./ipc/client")>();
  const resolve = <T,>(value: T) => vi.fn(() => Promise.resolve(value));
  const ipc = {
    ...Object.fromEntries(
      Object.keys(actual.ipc).map((key) => [
        key,
        vi.fn(() => Promise.reject(actual.toIpcError(null))),
      ]),
    ),
    settingsGet: resolve(settings),
    settingsUpdate: resolve(settings),
    providerList: resolve([remote]),
    localModelList: resolve([]),
    audioSourcesList: resolve([
      { kind: "microphone", availability: "available", permission: "granted" },
      {
        kind: "system",
        availability: "unavailable",
        unavailableReason: "not_validated_on_platform",
        permission: "not_requested",
      },
    ]),
    sessionCreate: resolve(idle),
    sessionStart: resolve({ ...idle, state: "starting" }),
    sessionStop: resolve({ ...idle, state: "stopping" }),
    sessionPause: resolve({ ...idle, state: "paused" }),
    sessionResume: resolve({ ...idle, state: "capturing" }),
    sessionList: resolve({ items: [ended] }),
    sessionGet: resolve(detail),
    planGet: resolve(plan),
    sessionSubscribe: vi.fn(
      (_id: string, onEvent: (event: SessionEvent) => void) => {
        state.emit = onEvent;
        return Promise.resolve(idle);
      },
    ),
  };
  return { ...actual, ipc };
});

// Imported after the mock is registered.
const { default: App } = await import("./App");
const { ipc } = await import("./ipc/client");

/** Presses Tab until the element named `name` with `role` has focus. */
async function tabTo(user: UserEvent, role: string, name: string | RegExp) {
  for (let presses = 0; presses < 80; presses += 1) {
    const active = document.activeElement;
    const target = screen.queryByRole(role, { name });
    if (target && active === target) return target;
    await user.tab();
  }
  throw new Error(`could not reach ${role} "${String(name)}" with Tab`);
}

async function page(user: UserEvent, name: string) {
  await tabTo(user, "button", name);
  await user.keyboard("{Enter}");
}

async function noViolations() {
  expect((await axe(document.body)).violations).toEqual([]);
}

describe("accessibility", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("every page passes axe", async () => {
    const user = userEvent.setup();
    render(<App />);
    await screen.findByRole("button", { name: "Criar sessão" });
    await noViolations();

    await page(user, "Histórico");
    await screen.findByRole("button", { name: ended.title });
    await noViolations();

    await user.click(screen.getByRole("button", { name: ended.title }));
    await screen.findByRole("button", { name: "Exportar…" });
    await noViolations();

    await page(user, "Provedores");
    await screen.findByText("Autorizado");
    await noViolations();

    await page(user, "Configurações");
    await screen.findByRole("heading", { name: "Configurações" });
    await noViolations();
  });

  it("starts, pauses and stops a session with the keyboard only", async () => {
    const user = userEvent.setup();
    render(<App />);
    await screen.findByRole("button", { name: "Criar sessão" });

    await tabTo(user, "textbox", "Título");
    await user.keyboard("Daily");
    await page(user, "Criar sessão");
    await page(user, "Iniciar captura");
    // The consent reminder takes focus on its confirm button.
    await screen.findByRole("dialog");
    expect(document.activeElement).toBe(
      screen.getByRole("button", { name: "Entendi, iniciar captura" }),
    );
    await user.keyboard("{Enter}");
    expect(ipc.sessionStart).toHaveBeenCalled();

    act(() => {
      state.emit({ type: "state_changed", seq: 1, state: "capturing" });
      state.emit({
        type: "capture_indicator",
        seq: 2,
        active: true,
        paused: false,
        sources: ["microphone"],
      });
    });
    // The capture state is announced through a polite live region, by text.
    const indicator = screen
      .getAllByRole("status")
      .find((element) => element.className.includes("capture-indicator"));
    expect(indicator?.getAttribute("aria-live")).toBe("polite");
    expect(indicator?.textContent).toContain("Capturando: Microfone");
    await noViolations();

    await page(user, "Pausar captura");
    expect(ipc.sessionPause).toHaveBeenCalled();
    act(() =>
      state.emit({ type: "state_changed", seq: 3, state: "paused" }),
    );
    await page(user, "Parar captura");
    expect(ipc.sessionStop).toHaveBeenCalled();
  });

  it("reviews a saved session and opens export with the keyboard only", async () => {
    const user = userEvent.setup();
    render(<App />);
    await screen.findByRole("button", { name: "Criar sessão" });
    await page(user, "Histórico");
    await screen.findByRole("button", { name: ended.title });
    await page(user, ended.title);
    await page(user, "Exportar…");
    const dialog = await screen.findByRole("dialog");
    expect(dialog.contains(document.activeElement)).toBe(true);
    await noViolations();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).toBeNull();
  });
});
