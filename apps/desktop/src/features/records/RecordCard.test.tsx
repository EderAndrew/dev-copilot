import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { axe } from "vitest-axe";

import type { RecordDto } from "../../ipc/types";
import { RecordCard } from "./RecordCard";
import { RecordsPanel } from "./RecordsPanel";
import { applyRecordEvent, emptyRecords, nextStatuses } from "./recordState";

const record: RecordDto = {
  id: "r1",
  sessionId: "s1",
  kind: "requirement",
  text: "O export precisa suportar CSV",
  nature: "observed",
  status: "proposed",
  rationale: "Pedido do cliente",
  origin: "ai",
  providerId: "p1",
  analysisRunId: "a1",
  evidence: ["seg-1"],
  acceptanceCriteria: [],
  alternatives: [],
  createdAt: 1,
  updatedAt: 1,
};

function renderCard(overrides: Partial<RecordDto> = {}) {
  const onSetStatus = vi.fn();
  const onShowEvidence = vi.fn();
  const view = render(
    <RecordCard
      record={{ ...record, ...overrides }}
      evidenceLabel={(_, i) => `trecho ${i + 1}`}
      busy={false}
      onSetStatus={onSetStatus}
      onShowEvidence={onShowEvidence}
    />,
  );
  return { ...view, onSetStatus, onShowEvidence };
}

describe("RecordCard", () => {
  it("shows kind, nature and status as text", () => {
    renderCard();
    const badges = screen.getByRole("list", { name: "Classificação" });
    expect(badges.textContent).toContain("Requisito");
    expect(badges.textContent).toContain("Observado");
    expect(badges.textContent).toContain("Proposto");
    expect(badges.textContent).not.toContain("Conflita");
  });

  it("marks conflicts, duplicates and user records in words", () => {
    renderCard({
      origin: "user",
      conflictsWith: "r0",
      duplicateOf: "r9",
      nature: "suggested",
      evidence: [],
    });
    const badges = screen.getByRole("list", { name: "Classificação" });
    expect(badges.textContent).toContain("Conflita com outro registro");
    expect(badges.textContent).toContain("Possível duplicata");
    expect(badges.textContent).toContain("Criado por você");
    expect(badges.textContent).toContain("Sugerido");
    expect(screen.getByText(/Sem trecho de evidência/)).toBeTruthy();
  });

  it("confirms, rejects or keeps pending by keyboard", async () => {
    const user = userEvent.setup();
    const { onSetStatus } = renderCard();
    const actions = screen.getByRole("group", { name: /Revisar/ });
    expect(
      within(actions)
        .getAllByRole("button")
        .map((b) => b.textContent),
    ).toEqual(["Confirmar", "Rejeitar", "Marcar como pendente"]);
    await user.tab(); // evidence link
    await user.tab();
    await user.keyboard("{Enter}");
    expect(onSetStatus).toHaveBeenLastCalledWith("confirmed");
    await user.tab();
    await user.keyboard(" ");
    expect(onSetStatus).toHaveBeenLastCalledWith("rejected");
    await user.tab();
    await user.keyboard("{Enter}");
    expect(onSetStatus).toHaveBeenLastCalledWith("pending");
  });

  it("offers only the transitions of the data model", () => {
    renderCard({ status: "confirmed" });
    const actions = screen.getByRole("group", { name: /Revisar/ });
    expect(
      within(actions)
        .getAllByRole("button")
        .map((b) => b.textContent),
    ).toEqual(["Marcar como pendente", "Rejeitar"]);
    expect(nextStatuses("pending")).toEqual(["confirmed", "rejected"]);
    expect(nextStatuses("rejected")).not.toContain("proposed");
  });

  it("evidence links point at the cited segment", async () => {
    const user = userEvent.setup();
    const { onShowEvidence } = renderCard();
    await user.click(screen.getByRole("button", { name: "trecho 1" }));
    expect(onShowEvidence).toHaveBeenCalledWith("seg-1");
  });

  it("has no axe violations", async () => {
    const { container } = renderCard({ conflictsWith: "r0" });
    expect((await axe(container)).violations).toEqual([]);
  });
});

describe("RecordsPanel", () => {
  it("applies the record returned by the core and reports analysis failures", async () => {
    const user = userEvent.setup();
    const confirmed = { ...record, status: "confirmed" as const };
    const client = { recordSetStatus: vi.fn(() => Promise.resolve(confirmed)) };
    const onUpdated = vi.fn();
    let state = applyRecordEvent(emptyRecords, {
      type: "record_proposed",
      seq: 1,
      record,
    });
    state = applyRecordEvent(state, {
      type: "analysis_status",
      seq: 2,
      runId: "a2",
      status: "provider_error",
    });
    render(
      <RecordsPanel
        records={state}
        segmentNumbers={new Map([["seg-1", 3]])}
        client={client}
        onUpdated={onUpdated}
      />,
    );
    expect(screen.getByRole("status").textContent).toContain(
      "registros existentes foram preservados",
    );
    expect(screen.getByRole("button", { name: "trecho 3" })).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Confirmar" }));
    expect(client.recordSetStatus).toHaveBeenCalledWith({
      recordId: "r1",
      status: "confirmed",
    });
    expect(onUpdated).toHaveBeenCalledWith(confirmed);
  });

  it("shows command errors without dropping records", async () => {
    const user = userEvent.setup();
    const client = {
      recordSetStatus: vi.fn(() =>
        Promise.reject({
          code: "invalid_state",
          messageKey: "errors.invalidState",
        }),
      ),
    };
    const state = applyRecordEvent(emptyRecords, {
      type: "record_proposed",
      seq: 1,
      record,
    });
    render(
      <RecordsPanel
        records={state}
        segmentNumbers={new Map()}
        client={client}
        onUpdated={vi.fn()}
      />,
    );
    await user.click(screen.getByRole("button", { name: "Rejeitar" }));
    expect(await screen.findByRole("alert")).toBeTruthy();
    expect(screen.getByText(record.text)).toBeTruthy();
  });
});
