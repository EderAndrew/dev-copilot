import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { axe } from "vitest-axe";

import { Layout } from "./Layout";

function renderLayout(overrides: Partial<Parameters<typeof Layout>[0]> = {}) {
  const props = {
    page: "sessions" as const,
    onNavigate: vi.fn(),
    language: "pt-BR" as const,
    onLanguageChange: vi.fn(),
    children: <h1>Conteúdo</h1>,
    ...overrides,
  };
  return { props, ...render(<Layout {...props} />) };
}

describe("Layout", () => {
  it("marks the current page and navigates with the keyboard", async () => {
    const user = userEvent.setup();
    const { props } = renderLayout();
    expect(
      screen
        .getByRole("button", { name: "Sessões" })
        .getAttribute("aria-current"),
    ).toBe("page");
    // Language selector, then Sessões, Histórico, Provedores.
    for (let i = 0; i < 4; i += 1) await user.tab();
    expect(document.activeElement).toBe(
      screen.getByRole("button", { name: "Provedores" }),
    );
    await user.keyboard("{Enter}");
    expect(props.onNavigate).toHaveBeenCalledWith("providers");
  });

  it("reports language changes", async () => {
    const user = userEvent.setup();
    const { props } = renderLayout();
    await user.selectOptions(
      screen.getByLabelText("Idioma da interface"),
      "en",
    );
    expect(props.onLanguageChange).toHaveBeenCalledWith("en");
  });

  it("renders the capture indicator slot in the header", () => {
    renderLayout({ captureIndicator: <span>indicador</span> });
    expect(
      screen.getByRole("region", { name: "Estado da captura" }).textContent,
    ).toContain("indicador");
  });

  it("has no axe violations", async () => {
    const { container } = renderLayout();
    const results = await axe(container);
    expect(results.violations).toEqual([]);
  });
});
