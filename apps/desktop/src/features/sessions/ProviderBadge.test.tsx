import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { axe } from "vitest-axe";

import { ProviderBadge } from "./ProviderBadge";

describe("ProviderBadge", () => {
  it("renders nothing before any provider is reported", () => {
    const { container } = render(<ProviderBadge providers={{}} />);
    expect(container.textContent).toBe("");
  });

  it("states locality per stage in text", async () => {
    const { container } = render(
      <ProviderBadge
        providers={{
          transcription: { locality: "local", providerId: "p1" },
          analysis: { locality: "remote", providerId: "p2" },
        }}
      />,
    );
    const list = screen.getByRole("list", { name: "Provedores em uso" });
    expect(list.textContent).toContain("Transcrição: local (neste computador)");
    expect(list.textContent).toContain("Análise: remoto");
    expect((await axe(container)).violations).toEqual([]);
  });
});
