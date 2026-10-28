import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { axe } from "vitest-axe";

import type { QuestionDto, RecordDto } from "../../ipc/types";
import { QuestionsPanel } from "./QuestionsPanel";
import {
  applyQuestionEvent,
  citedSegments,
  hasPending,
  upsertQuestion,
} from "./questionState";

const base: QuestionDto = {
  id: "q1",
  sessionId: "s1",
  question: "O que foi decidido sobre autenticação?",
  evidence: [],
  status: "pending",
  providerId: "p1",
  askedAt: 1_000,
};

const answered: QuestionDto = {
  ...base,
  status: "answered",
  answer: "Foi decidido usar OAuth2.",
  certainty: "supported",
  evidence: [
    { type: "segment", id: "seg-2" },
    { type: "record", id: "r1" },
  ],
  answeredAt: 2_000,
};

const decision: RecordDto = {
  id: "r1",
  sessionId: "s1",
  kind: "decision",
  text: "Usar OAuth2",
  nature: "observed",
  status: "confirmed",
  rationale: "",
  origin: "ai",
  evidence: ["seg-2"],
  acceptanceCriteria: [],
  alternatives: [],
  createdAt: 1,
  updatedAt: 1,
};

// The second segment was spoken in English in a pt-BR session.
const segments = citedSegments([
  {
    kind: "final",
    segment: { id: "seg-1", seq: 0, startMs: 0, endMs: 3000, text: "Bom dia" },
  },
  {
    kind: "final",
    segment: {
      id: "seg-2",
      seq: 1,
      startMs: 3000,
      endMs: 6000,
      text: "We agreed on OAuth2 for login",
    },
  },
]);

function renderPanel(questions: QuestionDto[], records = [decision]) {
  const client = {
    questionAsk: vi.fn((request: { sessionId: string; question: string }) =>
      Promise.resolve<QuestionDto>({
        ...base,
        id: "q-new",
        question: request.question,
      }),
    ),
    questionToRecord: vi.fn(() =>
      Promise.resolve<RecordDto>({
        ...decision,
        id: "r-new",
        status: "proposed",
        nature: "suggested",
        origin: "user",
      }),
    ),
  };
  const onAsked = vi.fn();
  const onRecordCreated = vi.fn();
  const view = render(
    <>
      <p id="segment-seg-2" tabIndex={-1}>
        trecho alvo
      </p>
      <QuestionsPanel
        sessionId="s1"
        questions={questions}
        segments={segments}
        records={records}
        client={client}
        onAsked={onAsked}
        onRecordCreated={onRecordCreated}
      />
    </>,
  );
  return { ...view, client, onAsked, onRecordCreated };
}

describe("QuestionsPanel", () => {
  it("asks by keyboard and reports the pending question", async () => {
    const user = userEvent.setup();
    const { client, onAsked } = renderPanel([]);
    expect(screen.getByText(/o que foi decidido sobre um tema/)).toBeTruthy();
    const input = screen.getByRole("textbox", { name: "Sua pergunta" });
    await user.click(input);
    await user.type(input, "Qual banco foi escolhido?");
    await user.keyboard("{Control>}{Enter}{/Control}");
    expect(client.questionAsk).toHaveBeenCalledWith({
      sessionId: "s1",
      question: "Qual banco foi escolhido?",
    });
    expect(onAsked).toHaveBeenCalledWith(
      expect.objectContaining({ id: "q-new", status: "pending" }),
    );
    expect((input as HTMLTextAreaElement).value).toBe("");
  });

  it("shows a pending question and blocks another until it is answered", () => {
    renderPanel([base]);
    expect(
      screen.getByText(
        /Aguardando resposta… A captura e a transcrição continuam/,
      ),
    ).toBeTruthy();
    expect(
      (screen.getByRole("button", { name: "Perguntar" }) as HTMLButtonElement)
        .disabled,
    ).toBe(true);
    expect(
      screen.getByText(/Aguarde a resposta da pergunta anterior/),
    ).toBeTruthy();
  });

  it("explains a failed question without blaming capture", () => {
    renderPanel([{ ...base, status: "failed", answeredAt: 2_000 }]);
    const status = screen.getByText(/Não foi possível responder/);
    expect(status.textContent).toContain("Nada foi alterado");
    expect(status.textContent).toContain("captura e a transcrição seguem");
    expect(screen.queryByRole("button", { name: /item proposto/ })).toBeNull();
  });

  it("labels certainty in words", () => {
    renderPanel([
      answered,
      {
        ...answered,
        id: "q2",
        askedAt: 3_000,
        certainty: "partial",
        evidence: [{ type: "segment", id: "seg-1" }],
      },
      {
        ...answered,
        id: "q3",
        askedAt: 4_000,
        question: "Qual nuvem?",
        answer: "A sessão não trata de hospedagem.",
        certainty: "no_evidence",
        evidence: [],
      },
    ]);
    expect(screen.getByText("Sustentada pela sessão")).toBeTruthy();
    expect(screen.getByText(/Parcialmente sustentada/)).toBeTruthy();
    expect(screen.getByText("Sem evidência na sessão")).toBeTruthy();
    expect(
      screen.getByText(/Nenhum trecho ou registro da sessão sustenta/),
    ).toBeTruthy();
  });

  it("quotes cited segments in their original language and links to them", async () => {
    const user = userEvent.setup();
    renderPanel([answered]);
    const answer = screen.getByRole("article");
    // Quoted as stored, never translated (FR-053b).
    expect(
      within(answer).getByText("We agreed on OAuth2 for login").tagName,
    ).toBe("BLOCKQUOTE");
    expect(within(answer).getByText(/Registro \(Decisão\)/)).toBeTruthy();
    expect(within(answer).getByText("Usar OAuth2")).toBeTruthy();
    await user.click(within(answer).getByRole("button", { name: "trecho 2" }));
    expect(document.activeElement?.id).toBe("segment-seg-2");
  });

  it("marks citations no longer available", () => {
    renderPanel(
      [
        {
          ...answered,
          evidence: [
            { type: "segment", id: "gone" },
            { type: "record", id: "deleted" },
          ],
        },
      ],
      [],
    );
    expect(
      screen.getByRole("button", { name: "trecho indisponível" }),
    ).toBeTruthy();
    expect(screen.getByText("Registro indisponível.")).toBeTruthy();
  });

  it("turns an answer into a proposed item only by explicit action", async () => {
    const user = userEvent.setup();
    const { client, onRecordCreated } = renderPanel([answered]);
    expect(client.questionToRecord).not.toHaveBeenCalled();
    const group = screen.getByRole("group", { name: /Transformar a resposta/ });
    await user.selectOptions(
      within(group).getByRole("combobox", { name: "Tipo do item" }),
      "risk",
    );
    await user.click(
      within(group).getByRole("button", {
        name: "Transformar em item proposto",
      }),
    );
    expect(client.questionToRecord).toHaveBeenCalledWith({
      questionId: "q1",
      kind: "risk",
    });
    expect(onRecordCreated).toHaveBeenCalledWith(
      expect.objectContaining({ id: "r-new", status: "proposed" }),
    );
    expect(
      await screen.findByText(/Item proposto adicionado ao plano/),
    ).toBeTruthy();
  });

  it("shows command errors", async () => {
    const user = userEvent.setup();
    const { client } = renderPanel([]);
    client.questionAsk.mockRejectedValueOnce({
      code: "provider_error",
      messageKey: "errors.providerError",
    });
    await user.type(
      screen.getByRole("textbox", { name: "Sua pergunta" }),
      "Algo?",
    );
    await user.click(screen.getByRole("button", { name: "Perguntar" }));
    expect((await screen.findByRole("alert")).textContent).toContain(
      "O provedor falhou",
    );
  });

  it("has no accessibility violations", async () => {
    const { container } = renderPanel([
      answered,
      { ...base, id: "q2", askedAt: 3_000 },
    ]);
    expect((await axe(container)).violations).toEqual([]);
  });
});

describe("questionState", () => {
  it("keeps ask order and replaces answers in place", () => {
    const later = { ...base, id: "q2", askedAt: 5_000 };
    let questions = upsertQuestion([], later);
    questions = upsertQuestion(questions, base);
    expect(questions.map((q) => q.id)).toEqual(["q1", "q2"]);
    questions = applyQuestionEvent(questions, {
      type: "question_answered",
      seq: 3,
      question: answered,
    });
    expect(questions[0]).toEqual(answered);
    expect(hasPending(questions)).toBe(true);
  });

  it("never goes back to pending when the command result arrives late", () => {
    const questions = upsertQuestion([answered], base);
    expect(questions).toEqual([answered]);
    expect(hasPending(questions)).toBe(false);
  });

  it("numbers final segments only", () => {
    const cited = citedSegments([
      { kind: "gap", reason: "paused", startMs: 0, durationMs: 10 },
      {
        kind: "final",
        segment: { id: "a", seq: 1, startMs: 10, endMs: 20, text: "Olá" },
      },
    ]);
    expect(cited.get("a")).toEqual({ number: 1, text: "Olá" });
  });
});
