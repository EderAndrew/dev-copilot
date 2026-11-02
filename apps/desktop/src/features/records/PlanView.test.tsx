import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { axe } from "vitest-axe";

import type { PlanDto, RecordDto } from "../../ipc/types";
import { PlanView } from "./PlanView";

const base: RecordDto = {
  id: "r1",
  sessionId: "s1",
  kind: "task",
  text: "Criar endpoint de export",
  nature: "observed",
  status: "proposed",
  rationale: "",
  origin: "ai",
  evidence: ["g1"],
  acceptanceCriteria: ["Exporta 10 mil linhas"],
  alternatives: [],
  createdAt: 1,
  updatedAt: 1,
};

function plan(...items: RecordDto[]): PlanDto {
  const kinds = [
    "task",
    "requirement",
    "decision",
    "assumption",
    "risk",
    "open_question",
  ] as const;
  return {
    sessionId: "s1",
    sections: kinds.map((kind) => ({
      kind,
      items: items.filter((i) => i.kind === kind),
    })),
  };
}

function setup(initial: PlanDto) {
  const client = {
    planGet: vi.fn(() => Promise.resolve(initial)),
    recordSetStatus: vi.fn(() => Promise.resolve(base)),
    recordEdit: vi.fn(() => Promise.resolve(base)),
    recordCreate: vi.fn(() => Promise.resolve(base)),
  };
  const view = render(<PlanView sessionId="s1" client={client} />);
  return { client, ...view };
}

describe("PlanView", () => {
  it("shows sections per kind with status, nature and user marking", async () => {
    const mine: RecordDto = {
      ...base,
      id: "r2",
      kind: "assumption",
      text: "Equipe de três pessoas",
      nature: "suggested",
      status: "confirmed",
      origin: "user",
      evidence: [],
      acceptanceCriteria: [],
    };
    const { container } = setup(plan(base, mine));
    const tasks = await screen.findByRole("region", {
      name: "Tarefas técnicas (1)",
    });
    expect(tasks.textContent).toContain("Criar endpoint de export");
    expect(tasks.textContent).toContain("Exporta 10 mil linhas");
    expect(tasks.textContent).toContain("Proposto");
    expect(tasks.textContent).toContain("Observado");
    const assumptions = screen.getByRole("region", { name: "Premissas (1)" });
    expect(assumptions.textContent).toContain("Criado por você");
    expect(
      screen.getByRole("region", { name: "Riscos (0)" }).textContent,
    ).toContain("Nenhum item.");
    expect((await axe(container)).violations).toEqual([]);
  });

  it("filters by status", async () => {
    const user = userEvent.setup();
    setup(
      plan(base, {
        ...base,
        id: "r3",
        text: "Item rejeitado",
        status: "rejected",
      }),
    );
    expect(await screen.findByText("Item rejeitado")).toBeTruthy();
    await user.click(screen.getByRole("checkbox", { name: "Rejeitado" }));
    expect(screen.queryByText("Item rejeitado")).toBeNull();
    expect(screen.getByText("Criar endpoint de export")).toBeTruthy();
  });

  it("edits inline by keyboard and reloads from the core", async () => {
    const user = userEvent.setup();
    const { client } = setup(plan(base));
    const task = await screen.findByRole("region", {
      name: "Tarefas técnicas (1)",
    });
    within(task).getByRole("button", { name: "Editar" }).focus();
    await user.keyboard("{Enter}");
    const editor = task.querySelector("form") as HTMLFormElement;
    const text = within(editor).getByRole("textbox", { name: "Texto" });
    expect(document.activeElement).toBe(text);
    await user.clear(text);
    await user.type(text, "Criar endpoint paginado");
    const criteria = within(editor).getByRole("textbox", {
      name: "Critérios de aceitação (um por linha)",
    });
    await user.clear(criteria);
    await user.type(criteria, "Página de 100{Enter}  {Enter}Ordena por data");
    await user.click(within(editor).getByRole("button", { name: "Salvar" }));
    expect(client.recordEdit).toHaveBeenCalledWith({
      recordId: "r1",
      text: "Criar endpoint paginado",
      rationale: "",
      acceptanceCriteria: ["Página de 100", "Ordena por data"],
      alternatives: undefined,
    });
    expect(client.planGet).toHaveBeenCalledTimes(2);
  });

  it("Escape cancels an edit without saving", async () => {
    const user = userEvent.setup();
    const { client } = setup(plan(base));
    await user.click(await screen.findByRole("button", { name: "Editar" }));
    await user.keyboard("{Escape}");
    // Only the add-item form's text field remains.
    expect(screen.getAllByRole("textbox", { name: "Texto" })).toHaveLength(1);
    expect(client.recordEdit).not.toHaveBeenCalled();
  });

  it("changes status and adds manual items", async () => {
    const user = userEvent.setup();
    const { client } = setup(plan(base));
    await user.click(await screen.findByRole("button", { name: "Confirmar" }));
    expect(client.recordSetStatus).toHaveBeenCalledWith({
      recordId: "r1",
      status: "confirmed",
    });

    const form = screen.getByRole("form", { name: "Adicionar item ao plano" });
    await user.selectOptions(
      within(form).getByRole("combobox", { name: "Tipo" }),
      "risk",
    );
    await user.type(
      within(form).getByRole("textbox", { name: "Texto" }),
      "Prazo curto",
    );
    await user.selectOptions(
      within(form).getByRole("combobox", { name: "Status" }),
      "pending",
    );
    await user.click(within(form).getByRole("button", { name: "Adicionar" }));
    expect(client.recordCreate).toHaveBeenCalledWith({
      sessionId: "s1",
      kind: "risk",
      text: "Prazo curto",
      status: "pending",
    });
  });

  it("shows core errors", async () => {
    const user = userEvent.setup();
    const { client } = setup(plan(base));
    client.recordSetStatus.mockRejectedValueOnce({
      code: "invalid_state",
      messageKey: "errors.invalidState",
    });
    await user.click(await screen.findByRole("button", { name: "Rejeitar" }));
    expect(await screen.findByRole("alert")).toBeTruthy();
    expect(screen.getByText("Criar endpoint de export")).toBeTruthy();
  });
});
