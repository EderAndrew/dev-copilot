import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { axe } from "vitest-axe";

import type {
  AuthorizationPreviewDto,
  LocalModelDto,
  ProviderAuthorizationDto,
  ProviderDto,
} from "../../ipc/types";
import { ProvidersPage } from "./ProvidersPage";

const remote: ProviderDto = {
  id: "p1",
  role: "analysis",
  adapter: "anthropic",
  locality: "remote",
  endpoint: "https://api.anthropic.com",
  model: "claude-opus-5-5",
  dataCategories: ["transcript", "questions"],
  isDefault: true,
  authorization: "configured",
  hasCredential: true,
};

const preview: AuthorizationPreviewDto = {
  providerId: "p1",
  destination: "api.anthropic.com",
  dataCategories: ["transcript", "questions"],
  retentionNotice: "Retido conforme os termos do serviço.",
};

const authorization: ProviderAuthorizationDto = {
  ...preview,
  state: "granted",
  grantedAt: 1,
};

function setup(initial: ProviderDto[]) {
  let providers = initial;
  const client = {
    localModelList: vi.fn((): Promise<LocalModelDto[]> => Promise.resolve([])),
    localModelDownload: vi.fn(() => Promise.resolve({ jobId: "j" })),
    providerList: vi.fn(() => Promise.resolve(providers)),
    providerUpsert: vi.fn(() => Promise.resolve(remote)),
    providerSetDefault: vi.fn(() => Promise.resolve()),
    providerSetSecret: vi.fn(() => Promise.resolve()),
    providerAuthorizationPreview: vi.fn(() => Promise.resolve(preview)),
    providerAuthorize: vi.fn(() => {
      providers = providers.map((p) => ({ ...p, authorization: "authorized" }));
      return Promise.resolve(authorization);
    }),
    providerRevoke: vi.fn(() => {
      providers = providers.map((p) => ({ ...p, authorization: "revoked" }));
      return Promise.resolve({ ...authorization, state: "revoked" as const });
    }),
  };
  return { client, ...render(<ProvidersPage client={client} />) };
}

describe("ProvidersPage", () => {
  it("authorizes a remote provider through the dialog, then revokes it", async () => {
    const user = userEvent.setup();
    const { client, container } = setup([remote]);
    const item = await screen.findByRole("listitem", {
      name: "Anthropic (claude-opus-5-5)",
    });
    expect(within(item).getByText("Não autorizado: nada é enviado")).toBeTruthy();
    expect(
      within(item).getByText("Chave de API guardada no cofre do sistema"),
    ).toBeTruthy();
    expect((await axe(container)).violations).toEqual([]);

    await user.click(
      within(item).getByRole("button", {
        name: "Autorizar Anthropic (claude-opus-5-5)…",
      }),
    );
    await user.click(await screen.findByRole("button", { name: "Autorizar" }));
    expect(client.providerAuthorize).toHaveBeenCalledWith("p1");
    await screen.findByText("Autorizado");

    await user.click(
      screen.getByRole("button", {
        name: "Revogar autorização de Anthropic (claude-opus-5-5)",
      }),
    );
    expect(client.providerRevoke).toHaveBeenCalledWith("p1");
    expect(
      await screen.findByText("Autorização revogada: nada é enviado"),
    ).toBeTruthy();
  });

  it("adds a remote provider and stores its key without showing it back", async () => {
    const user = userEvent.setup();
    const { client } = setup([]);
    await screen.findByText("Nenhum provedor configurado.");
    await user.click(screen.getByRole("button", { name: "Adicionar provedor" }));
    expect(
      (screen.getByLabelText("Endereço") as HTMLInputElement).value,
    ).toBe("https://api.anthropic.com");
    await user.type(screen.getByLabelText("Chave de API (opcional)"), "sk-xyz");
    await user.click(screen.getByRole("button", { name: "Salvar" }));
    expect(client.providerUpsert).toHaveBeenCalledWith({
      id: undefined,
      role: "analysis",
      adapter: "anthropic",
      endpoint: "https://api.anthropic.com",
      model: "claude-opus-5-5",
      dataCategories: ["transcript", "records", "questions"],
    });
    expect(client.providerSetSecret).toHaveBeenCalledWith("p1", "sk-xyz");
    expect(screen.queryByDisplayValue("sk-xyz")).toBeNull();
  });
});
