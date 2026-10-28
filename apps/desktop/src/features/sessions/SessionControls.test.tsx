import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { axe } from "vitest-axe";

import type { SessionDto, SessionState } from "../../ipc/types";
import { SessionControls } from "./SessionControls";

const base: SessionDto = {
  id: "019a2b3c-4d5e-7f60-8a1b-2c3d4e5f6a7b",
  title: "Refinamento técnico",
  language: "pt-BR",
  state: "idle",
  sources: ["microphone"],
  createdAt: 0,
  activeDurationMs: 0,
};

function renderControls(state: SessionState, extra: Partial<SessionDto> = {}) {
  const handlers = {
    onStart: vi.fn(),
    onCancelStart: vi.fn(),
    onStop: vi.fn(),
  };
  const utils = render(
    <SessionControls session={{ ...base, state, ...extra }} {...handlers} />,
  );
  return { handlers, ...utils };
}

describe("SessionControls", () => {
  it("starts with the keyboard from idle", async () => {
    const user = userEvent.setup();
    const { handlers } = renderControls("idle");
    expect(screen.getByText("ociosa")).toBeTruthy();
    await user.tab();
    expect(document.activeElement).toBe(
      screen.getByRole("button", { name: "Iniciar captura" }),
    );
    await user.keyboard("{Enter}");
    expect(handlers.onStart).toHaveBeenCalledOnce();
  });

  it("offers cancel while starting and stop while capturing", async () => {
    const user = userEvent.setup();
    const starting = renderControls("starting");
    await user.tab();
    await user.keyboard(" ");
    expect(starting.handlers.onCancelStart).toHaveBeenCalledOnce();
    starting.unmount();

    const capturing = renderControls("capturing");
    expect(
      screen.queryByRole("button", { name: "Iniciar captura" }),
    ).toBeNull();
    await user.tab();
    await user.keyboard("{Enter}");
    expect(capturing.handlers.onStop).toHaveBeenCalledOnce();
  });

  it("disables controls while stopping and hides them when ended", () => {
    const stopping = renderControls("stopping");
    const button = screen.getByRole("button", { name: "Parando…" });
    expect((button as HTMLButtonElement).disabled).toBe(true);
    stopping.unmount();
    renderControls("ended");
    expect(screen.queryAllByRole("button")).toEqual([]);
  });

  it("explains failures", () => {
    renderControls("failed", { failure: "permission_denied" });
    expect(screen.getByRole("alert").textContent).toContain(
      "permissão de áudio foi negada",
    );
  });

  it("has no axe violations", async () => {
    const { container } = renderControls("capturing");
    expect((await axe(container)).violations).toEqual([]);
  });
});
