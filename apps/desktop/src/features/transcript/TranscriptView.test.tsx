import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { axe } from "vitest-axe";

import type { SessionEvent } from "../../ipc/types";
import { TranscriptView } from "./TranscriptView";
import {
  applyTranscriptEvent,
  emptyTranscript,
  type TranscriptState,
} from "./transcriptState";

/** An event without `seq`, for each member of the union. */
type Unsequenced = SessionEvent extends infer E
  ? E extends SessionEvent
    ? Omit<E, "seq">
    : never
  : never;

function build(events: Unsequenced[]): TranscriptState {
  return events.reduce(
    (state, event, index) =>
      applyTranscriptEvent(state, { ...event, seq: index + 1 } as SessionEvent),
    emptyTranscript,
  );
}

const final = (
  id: string,
  startMs: number,
  endMs: number,
  text: string,
  source?: "microphone" | "system",
): Unsequenced => ({
  type: "final_segment",
  segment: { id, seq: 0, startMs, endMs, text, ...(source ? { source } : {}) },
});

const scroller = () =>
  screen.getByRole("region", { name: "Transcrição da sessão" });

describe("TranscriptView", () => {
  it("shows finals with timestamps and partials marked distinctly", () => {
    const transcript = build([
      final("a", 0, 2_000, "Precisamos de paginação."),
      { type: "partial_segment", startMs: 65_000, text: "E também de cache" },
    ]);
    render(<TranscriptView transcript={transcript} showSources={false} />);
    const items = within(scroller()).getAllByRole("listitem");
    expect(items).toHaveLength(2);
    expect(items[0].className).toBe("transcript-final");
    expect(items[0].textContent).toContain("0:00");
    expect(items[0].textContent).toContain("Precisamos de paginação.");
    expect(items[1].className).toBe("transcript-partial");
    expect(items[1].textContent).toContain("1:05");
    // The partial is labelled for screen readers, not only styled.
    expect(
      within(items[1]).getByText(/Parcial, ainda em reconhecimento/),
    ).toBeTruthy();
    expect(within(items[0]).queryByText(/Parcial/)).toBeNull();
  });

  it("labels timestamps for screen readers", () => {
    render(
      <TranscriptView
        transcript={build([final("a", 3_725_000, 3_726_000, "x")])}
        showSources={false}
      />,
    );
    expect(screen.getByLabelText("aos 1:02:05").textContent).toBe("1:02:05");
  });

  it("replaces a partial by the final that covers it", () => {
    const transcript = build([
      { type: "partial_segment", startMs: 0, text: "olá mun" },
      final("a", 0, 1_500, "olá mundo"),
    ]);
    render(<TranscriptView transcript={transcript} showSources={false} />);
    expect(screen.queryByText("olá mun")).toBeNull();
    expect(screen.getByText("olá mundo")).toBeTruthy();
  });

  it("renders gaps as gaps, never as content, in time order", () => {
    const transcript = build([
      final("b", 20_000, 22_000, "depois"),
      { type: "partial_segment", startMs: 4_000, text: "perdido" },
      { type: "gap", reason: "overload", startMs: 4_000, durationMs: 15_000 },
    ]);
    render(<TranscriptView transcript={transcript} showSources={false} />);
    const items = within(scroller()).getAllByRole("listitem");
    expect(items.map((i) => i.className)).toEqual([
      "transcript-gap",
      "transcript-final",
    ]);
    expect(items[0].textContent).toContain(
      "Lacuna na transcrição (0:15): áudio descartado por sobrecarga",
    );
    expect(screen.queryByText("perdido")).toBeNull();
  });

  it("shows the lag notice while transcription lags and clears it on recovery", () => {
    const lagging = build([{ type: "transcription_lag", lagMs: 18_000 }]);
    const { rerender } = render(
      <TranscriptView transcript={lagging} showSources={false} />,
    );
    expect(screen.getByRole("status").textContent).toContain("cerca de 18 s");
    const recovered = applyTranscriptEvent(lagging, {
      type: "transcription_lag",
      seq: 9,
      lagMs: 0,
    });
    rerender(<TranscriptView transcript={recovered} showSources={false} />);
    expect(screen.queryByRole("status")).toBeNull();
  });

  it("tags sources only for two-source sessions", () => {
    const transcript = build([final("a", 0, 1_000, "do sistema", "system")]);
    const { rerender } = render(
      <TranscriptView transcript={transcript} showSources={false} />,
    );
    expect(screen.queryByText("Áudio do sistema")).toBeNull();
    rerender(<TranscriptView transcript={transcript} showSources />);
    expect(screen.getByText("Áudio do sistema")).toBeTruthy();
  });

  it("pauses auto-scroll when the user scrolls up and offers to resume", async () => {
    const user = userEvent.setup();
    render(
      <TranscriptView
        transcript={build([final("a", 0, 1_000, "x")])}
        showSources={false}
      />,
    );
    const region = scroller();
    Object.defineProperties(region, {
      scrollHeight: { configurable: true, value: 1_000 },
      clientHeight: { configurable: true, value: 200 },
    });
    region.scrollTop = 100;
    region.dispatchEvent(new Event("scroll"));
    const resume = await screen.findByRole("button", {
      name: "Voltar ao trecho mais recente",
    });
    await user.click(resume);
    expect(region.scrollTop).toBe(1_000);
    expect(
      screen.queryByRole("button", { name: "Voltar ao trecho mais recente" }),
    ).toBeNull();
  });

  it("is keyboard-reachable and has no axe violations", async () => {
    const { container } = render(
      <TranscriptView
        transcript={build([
          final("a", 0, 1_000, "olá"),
          {
            type: "gap",
            reason: "provider_failure",
            startMs: 1_000,
            durationMs: 2_000,
          },
          { type: "partial_segment", startMs: 3_000, text: "e" },
          { type: "transcription_lag", lagMs: 16_000 },
        ])}
        showSources={false}
      />,
    );
    expect(scroller().tabIndex).toBe(0);
    expect((await axe(container)).violations).toEqual([]);
  });
});

describe("transcript state", () => {
  it("ignores duplicate finals and drops partials when capture ends", () => {
    let state = build([final("a", 0, 1_000, "x"), final("a", 0, 1_000, "x")]);
    expect(state.entries).toHaveLength(1);
    state = applyTranscriptEvent(state, {
      type: "partial_segment",
      seq: 5,
      startMs: 1_000,
      text: "y",
    });
    state = applyTranscriptEvent(state, {
      type: "state_changed",
      seq: 6,
      state: "ended",
    });
    expect(state.partials).toEqual({});
  });

  it("keeps one partial per source", () => {
    const state = build([
      { type: "partial_segment", source: "microphone", startMs: 0, text: "a" },
      { type: "partial_segment", source: "system", startMs: 0, text: "b" },
      { type: "partial_segment", source: "microphone", startMs: 0, text: "a2" },
    ]);
    expect(
      Object.values(state.partials)
        .map((p) => p.text)
        .sort(),
    ).toEqual(["a2", "b"]);
  });
});
