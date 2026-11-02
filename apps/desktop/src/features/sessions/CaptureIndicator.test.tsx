import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { axe } from "vitest-axe";

import { CaptureIndicator } from "./CaptureIndicator";
import { INDICATOR_OFF } from "./sessionStore";

describe("CaptureIndicator", () => {
  it("shows text and icon with the active sources", () => {
    render(
      <CaptureIndicator
        indicator={{ active: true, paused: false, sources: ["microphone"] }}
      />,
    );
    const status = screen.getByRole("status");
    expect(status.textContent).toContain("Capturando: Microfone");
    const icon = status.querySelector("[aria-hidden='true']");
    expect(icon?.textContent).toBe("●");
  });

  it("is a polite live region so changes are announced", () => {
    render(<CaptureIndicator indicator={INDICATOR_OFF} />);
    const status = screen.getByRole("status");
    expect(status.getAttribute("aria-live")).toBe("polite");
    expect(status.getAttribute("aria-atomic")).toBe("true");
    expect(status.textContent).toContain("Sem captura");
  });

  it("distinguishes off, on and paused without relying on color", () => {
    const { rerender } = render(<CaptureIndicator indicator={INDICATOR_OFF} />);
    const icon = () =>
      screen.getByRole("status").querySelector("[aria-hidden='true']")
        ?.textContent;
    const off = icon();
    rerender(
      <CaptureIndicator
        indicator={{ active: true, paused: true, sources: ["microphone"] }}
      />,
    );
    expect(screen.getByRole("status").textContent).toContain(
      "Captura pausada: Microfone",
    );
    const paused = icon();
    rerender(
      <CaptureIndicator
        indicator={{
          active: true,
          paused: false,
          sources: ["microphone", "system"],
        }}
      />,
    );
    expect(screen.getByRole("status").textContent).toContain(
      "Microfone, Áudio do sistema",
    );
    expect(new Set([off, paused, icon()]).size).toBe(3);
  });

  it("has no axe violations", async () => {
    const { container } = render(
      <CaptureIndicator
        indicator={{ active: true, paused: false, sources: ["microphone"] }}
      />,
    );
    expect((await axe(container)).violations).toEqual([]);
  });
});
