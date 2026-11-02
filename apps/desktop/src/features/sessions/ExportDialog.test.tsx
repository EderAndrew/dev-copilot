import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { axe } from "vitest-axe";

import type { ExportResultDto, IpcError } from "../../ipc/types";
import { ExportDialog } from "./ExportDialog";

function client(result: () => Promise<ExportResultDto>) {
  return { exportSession: vi.fn(result) };
}

describe("ExportDialog", () => {
  it("warns that exported files are not encrypted", async () => {
    const { container } = render(
      <ExportDialog
        sessionId="s1"
        client={client(() => Promise.resolve({ written: [] }))}
        onClose={vi.fn()}
      />,
    );
    const dialog = screen.getByRole("dialog", { name: "Exportar sessão" });
    expect(dialog.getAttribute("aria-describedby")).toBeTruthy();
    expect(
      screen.getByText(/arquivos exportados não são cifrados/i),
    ).toBeTruthy();
    // The first option takes focus so the flow works from the keyboard.
    expect(document.activeElement).toBe(
      screen.getByRole("checkbox", { name: "Markdown (legível)" }),
    );
    expect((await axe(container)).violations).toEqual([]);
  });

  it("exports the chosen formats and content and lists the files written", async () => {
    const user = userEvent.setup();
    const ipc = client(() =>
      Promise.resolve({ written: ["revisao-2026-10-27.json"] }),
    );
    render(<ExportDialog sessionId="s1" client={ipc} onClose={vi.fn()} />);

    await user.click(
      screen.getByRole("checkbox", { name: "Markdown (legível)" }),
    );
    await user.click(screen.getByRole("checkbox", { name: "Transcrição" }));
    await user.click(screen.getByRole("button", { name: "Exportar" }));

    expect(ipc.exportSession).toHaveBeenCalledWith({
      sessionId: "s1",
      formats: ["json"],
      content: ["plan"],
    });
    expect((await screen.findByRole("status")).textContent).toContain(
      "Arquivos gravados: revisao-2026-10-27.json",
    );
  });

  it("keeps the contract order however boxes are ticked", async () => {
    const user = userEvent.setup();
    const ipc = client(() => Promise.resolve({ written: [] }));
    render(<ExportDialog sessionId="s1" client={ipc} onClose={vi.fn()} />);
    const markdown = screen.getByRole("checkbox", {
      name: "Markdown (legível)",
    });
    await user.click(markdown);
    await user.click(markdown);
    await user.click(screen.getByRole("button", { name: "Exportar" }));
    expect(ipc.exportSession).toHaveBeenCalledWith({
      sessionId: "s1",
      formats: ["markdown", "json"],
      content: ["plan", "transcript"],
    });
  });

  it("says nothing was written when the save dialog is cancelled", async () => {
    const user = userEvent.setup();
    render(
      <ExportDialog
        sessionId="s1"
        client={client(() => Promise.resolve({ written: [] }))}
        onClose={vi.fn()}
      />,
    );
    await user.click(screen.getByRole("button", { name: "Exportar" }));
    expect((await screen.findByRole("status")).textContent).toContain(
      "nenhum arquivo foi gravado",
    );
  });

  it("needs a format and a content", async () => {
    const user = userEvent.setup();
    const ipc = client(() => Promise.resolve({ written: [] }));
    render(<ExportDialog sessionId="s1" client={ipc} onClose={vi.fn()} />);
    await user.click(
      screen.getByRole("checkbox", { name: "Plano, perguntas e respostas" }),
    );
    await user.click(screen.getByRole("checkbox", { name: "Transcrição" }));
    const submit = screen.getByRole("button", {
      name: "Exportar",
    }) as HTMLButtonElement;
    expect(submit.disabled).toBe(true);
    expect(screen.getByText(/ao menos um formato e um conteúdo/)).toBeTruthy();
  });

  it("shows failures and closes with Escape", async () => {
    const user = userEvent.setup();
    const onClose = vi.fn();
    const failure: IpcError = {
      code: "storage_error",
      messageKey: "errors.storageError",
    };
    render(
      <ExportDialog
        sessionId="s1"
        client={client(() => Promise.reject(failure))}
        onClose={onClose}
      />,
    );
    await user.click(screen.getByRole("button", { name: "Exportar" }));
    expect((await screen.findByRole("alert")).textContent).toContain(
      "Não foi possível gravar",
    );
    await user.keyboard("{Escape}");
    expect(onClose).toHaveBeenCalled();
  });
});
