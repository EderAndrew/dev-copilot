import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { axe } from "vitest-axe";

import type {
  LocalModelDto,
  ModelDownloadEvent,
  ProviderDto,
} from "../../ipc/types";
import { LocalModels, type LocalModelsClient } from "./LocalModels";

const base: LocalModelDto = {
  id: "base",
  displayName: "Whisper base",
  origin: "huggingface.co",
  sizeBytes: 147_951_465,
  downloaded: false,
  downloading: false,
};

function setup(models: LocalModelDto[], providers: ProviderDto[] = []) {
  let emit: (event: ModelDownloadEvent) => void = () => undefined;
  const state = { models, providers };
  const client = {
    localModelList: vi.fn(async () => state.models),
    localModelDownload: vi.fn(
      async (_id: string, onEvent: (e: ModelDownloadEvent) => void) => {
        emit = onEvent;
        return { jobId: "j1" };
      },
    ),
    providerList: vi.fn(async () => state.providers),
    providerUpsert: vi.fn(async () => ({
      id: "p1",
      role: "transcription" as const,
      adapter: "whisper_local" as const,
      locality: "local" as const,
      model: "base",
      dataCategories: ["audio" as const],
      authorization: "not_required" as const,
      hasCredential: false,
      isDefault: false,
    })),
    providerSetDefault: vi.fn(async () => undefined),
  } satisfies LocalModelsClient;
  render(<LocalModels client={client} />);
  return { client, state, emit: (e: ModelDownloadEvent) => act(() => emit(e)) };
}

describe("LocalModels", () => {
  it("shows origin and size without downloading anything", async () => {
    const { client } = setup([base]);
    expect(await screen.findByText("Whisper base")).toBeTruthy();
    expect(screen.getByText("origem: huggingface.co")).toBeTruthy();
    expect(screen.getByText("141 MB")).toBeTruthy();
    expect(client.localModelDownload).not.toHaveBeenCalled();
  });

  it("downloads only on request and shows progress, then offers to use the model", async () => {
    const user = userEvent.setup();
    const { client, state, emit } = setup([base]);
    await user.click(
      await screen.findByRole("button", { name: "Baixar Whisper base" }),
    );
    expect(client.localModelDownload).toHaveBeenCalledWith(
      "base",
      expect.any(Function),
    );
    emit({ type: "progress", jobId: "j1", receivedBytes: 50, totalBytes: 100 });
    const bar = screen.getByRole("progressbar");
    expect(bar.getAttribute("value")).toBe("50");

    state.models = [{ ...base, downloaded: true }];
    emit({ type: "completed", jobId: "j1" });
    await user.click(
      await screen.findByRole("button", {
        name: "Usar Whisper base para transcrever",
      }),
    );
    expect(client.providerUpsert).toHaveBeenCalledWith({
      id: undefined,
      role: "transcription",
      adapter: "whisper_local",
      model: "base",
      dataCategories: ["audio"],
    });
    expect(client.providerSetDefault).toHaveBeenCalledWith({
      role: "transcription",
      providerId: "p1",
    });
  });

  it("reports a failed verification", async () => {
    const user = userEvent.setup();
    const { emit } = setup([base]);
    await user.click(
      await screen.findByRole("button", { name: "Baixar Whisper base" }),
    );
    emit({
      type: "failed",
      jobId: "j1",
      messageKey: "errors.modelVerificationFailed",
    });
    expect((await screen.findByRole("alert")).textContent).toContain(
      "não confere",
    );
  });

  it("marks the model in use and reuses its provider", async () => {
    setup(
      [{ ...base, downloaded: true }],
      [
        {
          id: "p9",
          role: "transcription",
          adapter: "whisper_local",
          locality: "local",
          model: "base",
          dataCategories: ["audio"],
          isDefault: true,
          authorization: "not_required",
          hasCredential: false,
        },
      ],
    );
    expect(await screen.findByText("(em uso)")).toBeTruthy();
    expect(screen.queryByRole("button")).toBeNull();
  });

  it("has no axe violations", async () => {
    setup([
      base,
      { ...base, id: "tiny", displayName: "Whisper tiny", downloaded: true },
    ]);
    await screen.findByText("Whisper base");
    expect((await axe(document.body)).violations).toEqual([]);
  });
});
