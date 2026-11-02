import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { axe } from "vitest-axe";

import type { IpcError, SessionDto } from "../../ipc/types";
import { DeleteSessionDialog } from "./DeleteSessionDialog";

const ended: SessionDto = {
  id: "s1",
  title: "Planejamento do export",
  language: "pt-BR",
  state: "ended",
  sources: ["microphone"],
  createdAt: 1_000,
  startedAt: 2_000,
  endedAt: 3_000,
  activeDurationMs: 1_000,
};

function client(
  result: () => Promise<Record<string, never>> = () => Promise.resolve({}),
) {
  return { sessionDelete: vi.fn(result) };
}

describe("DeleteSessionDialog", () => {
  it("states every deletion limit and starts on Cancel", async () => {
    const { container } = render(
      <DeleteSessionDialog
        session={ended}
        client={client()}
        onDeleted={vi.fn()}
        onCancel={vi.fn()}
      />,
    );
    screen.getByRole("alertdialog", {
      name: "Excluir “Planejamento do export”?",
    });
    const warnings = screen
      .getAllByRole("listitem")
      .map((item) => item.textContent ?? "");
    expect(warnings).toHaveLength(4);
    expect(warnings[0]).toMatch(/remoção segura não é garantida/);
    expect(warnings[1]).toMatch(/Backups do sistema/);
    expect(warnings[2]).toMatch(/Arquivos já exportados não são excluídos/);
    expect(warnings[3]).toMatch(/não pode ser desfeita/);
    // A stray Enter must not delete: focus starts on Cancel.
    expect(document.activeElement).toBe(
      screen.getByRole("button", { name: "Cancelar" }),
    );
    expect((await axe(container)).violations).toEqual([]);
  });

  it("deletes only on explicit confirmation", async () => {
    const user = userEvent.setup();
    const ipc = client();
    const onDeleted = vi.fn();
    const onCancel = vi.fn();
    render(
      <DeleteSessionDialog
        session={ended}
        client={ipc}
        onDeleted={onDeleted}
        onCancel={onCancel}
      />,
    );
    await user.keyboard("{Enter}");
    expect(onCancel).toHaveBeenCalledTimes(1);
    expect(ipc.sessionDelete).not.toHaveBeenCalled();

    await user.click(
      screen.getByRole("button", { name: "Excluir definitivamente" }),
    );
    expect(ipc.sessionDelete).toHaveBeenCalledWith("s1");
    expect(onDeleted).toHaveBeenCalled();
  });

  it("asks to stop an active session instead of offering deletion", () => {
    render(
      <DeleteSessionDialog
        session={{ ...ended, state: "capturing", endedAt: undefined }}
        client={client()}
        onDeleted={vi.fn()}
        onCancel={vi.fn()}
      />,
    );
    expect(screen.getByText(/Pare a captura antes de excluí-la/)).toBeTruthy();
    expect(
      screen.queryByRole("button", { name: "Excluir definitivamente" }),
    ).toBeNull();
  });

  it("reports a rejected deletion and stays open", async () => {
    const user = userEvent.setup();
    const failure: IpcError = {
      code: "invalid_state",
      messageKey: "errors.invalidState",
    };
    const onDeleted = vi.fn();
    render(
      <DeleteSessionDialog
        session={ended}
        client={client(() => Promise.reject(failure))}
        onDeleted={onDeleted}
        onCancel={vi.fn()}
      />,
    );
    await user.click(
      screen.getByRole("button", { name: "Excluir definitivamente" }),
    );
    expect((await screen.findByRole("alert")).textContent).toContain(
      "não é permitida no estado atual",
    );
    expect(onDeleted).not.toHaveBeenCalled();
    expect(
      screen.getByRole("button", { name: "Excluir definitivamente" }),
    ).toBeTruthy();
  });
});
