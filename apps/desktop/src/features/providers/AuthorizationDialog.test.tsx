import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { axe } from "vitest-axe";

import type {
  AuthorizationPreviewDto,
  ProviderAuthorizationDto,
} from "../../ipc/types";
import { AuthorizationDialog } from "./AuthorizationDialog";

const preview: AuthorizationPreviewDto = {
  providerId: "p1",
  destination: "api.anthropic.com",
  dataCategories: ["transcript", "questions"],
  retentionNotice:
    "Os dados enviados a api.anthropic.com podem ser retidos conforme os termos desse serviço.",
};

const granted: ProviderAuthorizationDto = {
  ...preview,
  state: "granted",
  grantedAt: 1,
};

function client() {
  return {
    providerAuthorizationPreview: vi.fn(() => Promise.resolve(preview)),
    providerAuthorize: vi.fn(() => Promise.resolve(granted)),
  };
}

describe("AuthorizationDialog", () => {
  it("shows destination, categories and retention before anything is sent", async () => {
    const ipc = client();
    const { container } = render(
      <AuthorizationDialog
        providerId="p1"
        client={ipc}
        onAuthorized={vi.fn()}
        onCancel={vi.fn()}
      />,
    );
    await screen.findByRole("dialog", {
      name: "Autorizar envio de dados para api.anthropic.com?",
    });
    expect(ipc.providerAuthorizationPreview).toHaveBeenCalledWith("p1");
    expect(screen.getByText("Destino").nextElementSibling?.textContent).toBe(
      "api.anthropic.com",
    );
    const categories = screen
      .getAllByRole("listitem")
      .map((item) => item.textContent);
    expect(categories).toEqual(["Transcrição", "Perguntas"]);
    expect(screen.getByText(preview.retentionNotice)).toBeTruthy();
    expect(screen.getByText(/Nada é enviado antes do seu aceite/)).toBeTruthy();
    expect(ipc.providerAuthorize).not.toHaveBeenCalled();
    // A stray Enter must not authorize: focus starts on Decline.
    expect(document.activeElement).toBe(
      screen.getByRole("button", { name: "Recusar" }),
    );
    expect((await axe(container)).violations).toEqual([]);
  });

  it("authorizes only on explicit acceptance", async () => {
    const user = userEvent.setup();
    const ipc = client();
    const onAuthorized = vi.fn();
    render(
      <AuthorizationDialog
        providerId="p1"
        client={ipc}
        onAuthorized={onAuthorized}
        onCancel={vi.fn()}
      />,
    );
    await user.click(await screen.findByRole("button", { name: "Autorizar" }));
    expect(ipc.providerAuthorize).toHaveBeenCalledWith("p1");
    expect(onAuthorized).toHaveBeenCalledWith(granted);
  });

  it("declining sends nothing", async () => {
    const user = userEvent.setup();
    const ipc = client();
    const onCancel = vi.fn();
    render(
      <AuthorizationDialog
        providerId="p1"
        client={ipc}
        onAuthorized={vi.fn()}
        onCancel={onCancel}
      />,
    );
    await screen.findByText("api.anthropic.com");
    await user.click(screen.getByRole("button", { name: "Recusar" }));
    await user.keyboard("{Escape}");
    expect(onCancel).toHaveBeenCalled();
    expect(ipc.providerAuthorize).not.toHaveBeenCalled();
  });

  it("cannot accept while the preview is missing and reports failures", async () => {
    const ipc = client();
    ipc.providerAuthorizationPreview.mockRejectedValueOnce({
      code: "invalid_input",
      messageKey: "errors.invalidInput",
    });
    render(
      <AuthorizationDialog
        providerId="p1"
        client={ipc}
        onAuthorized={vi.fn()}
        onCancel={vi.fn()}
      />,
    );
    expect((await screen.findByRole("alert")).textContent).toBeTruthy();
    expect(
      (screen.getByRole("button", { name: "Autorizar" }) as HTMLButtonElement)
        .disabled,
    ).toBe(true);
  });
});
