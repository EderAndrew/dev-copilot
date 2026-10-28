import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { axe } from "vitest-axe";

import type {
  SessionDetailDto,
  SessionDto,
  SessionPageDto,
} from "../../ipc/types";
import { SessionDetail } from "./SessionDetail";
import { SessionHistory } from "./SessionHistory";

const ended: SessionDto = {
  id: "s1",
  title: "Planejamento do export",
  language: "pt-BR",
  state: "ended",
  sources: ["microphone"],
  createdAt: Date.UTC(2026, 9, 27, 13, 0),
  startedAt: Date.UTC(2026, 9, 27, 13, 1),
  endedAt: Date.UTC(2026, 9, 27, 13, 31),
  activeDurationMs: 30 * 60 * 1000,
};

const failed: SessionDto = {
  ...ended,
  id: "s2",
  title: "Retro",
  state: "failed",
  failure: "interrupted",
  activeDurationMs: 65_000,
};

describe("SessionHistory", () => {
  it("lists title, date, duration and state, and opens a session", async () => {
    const user = userEvent.setup();
    const onOpen = vi.fn();
    const client = {
      sessionList: vi.fn(() =>
        Promise.resolve<SessionPageDto>({ items: [ended, failed] }),
      ),
    };
    const { container } = render(
      <SessionHistory client={client} onOpen={onOpen} />,
    );
    const items = await screen.findAllByRole("listitem");
    expect(items).toHaveLength(2);
    expect(items[0].textContent).toContain("Planejamento do export");
    expect(items[0].textContent).toContain("Duração: 30:00");
    expect(items[0].textContent).toContain("encerrada");
    expect(items[0].textContent).toMatch(/27\/10\/2026/);
    // Failed sessions are marked in words, with an icon, not by color alone.
    expect(items[1].textContent).toContain("falhou");
    expect(items[1].querySelector("[aria-hidden='true']")?.textContent).toBe(
      "⚠ ",
    );
    await user.click(within(items[1]).getByRole("button", { name: "Retro" }));
    expect(onOpen).toHaveBeenCalledWith("s2");
    expect((await axe(container)).violations).toEqual([]);
  });

  it("loads more pages with the cursor", async () => {
    const user = userEvent.setup();
    const client = {
      sessionList: vi
        .fn()
        .mockResolvedValueOnce({ items: [ended], nextCursor: "s1" })
        .mockResolvedValueOnce({ items: [failed] }),
    };
    render(<SessionHistory client={client} onOpen={vi.fn()} />);
    await user.click(
      await screen.findByRole("button", { name: "Carregar mais" }),
    );
    expect(client.sessionList).toHaveBeenLastCalledWith({
      cursor: "s1",
      limit: 20,
    });
    expect(await screen.findByText("Retro")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Carregar mais" })).toBeNull();
  });

  it("shows an empty state", async () => {
    const client = {
      sessionList: vi.fn(() => Promise.resolve({ items: [] })),
    };
    render(<SessionHistory client={client} onOpen={vi.fn()} />);
    expect(await screen.findByText("Nenhuma sessão salva ainda.")).toBeTruthy();
  });
});

describe("SessionDetail", () => {
  function client(detail: SessionDetailDto) {
    return {
      sessionGet: vi.fn(() => Promise.resolve(detail)),
      sessionRename: vi.fn((_id: string, title: string) =>
        Promise.resolve({ ...detail.session, title }),
      ),
      analysisRunFinal: vi.fn(() => Promise.resolve({ runId: "a1" })),
      sessionSubscribe: vi.fn(() => Promise.resolve(detail.session)),
      planGet: vi.fn(() =>
        Promise.resolve({ sessionId: detail.session.id, sections: [] }),
      ),
      recordSetStatus: vi.fn(),
      recordEdit: vi.fn(),
      recordCreate: vi.fn(),
    };
  }

  it("shows preserved content and the failure of a failed session", async () => {
    const detail: SessionDetailDto = {
      session: failed,
      transcript: [
        {
          type: "final",
          segment: {
            id: "g1",
            seq: 0,
            startMs: 0,
            endMs: 3000,
            text: "O export precisa suportar CSV",
          },
        },
        { type: "gap", reason: "interrupted", startMs: 3000, durationMs: 0 },
      ],
      records: [],
    };
    render(
      <SessionDetail sessionId="s2" client={client(detail)} onBack={vi.fn()} />,
    );
    expect(await screen.findByRole("heading", { level: 1 })).toBeTruthy();
    expect(
      screen.getByText(/O aplicativo foi encerrado durante a captura/),
    ).toBeTruthy();
    expect(screen.getByText("O export precisa suportar CSV")).toBeTruthy();
    expect(screen.getByText("1:05")).toBeTruthy();
  });

  it("renames and runs the final analysis by explicit action", async () => {
    const user = userEvent.setup();
    const c = client({ session: ended, transcript: [], records: [] });
    render(<SessionDetail sessionId="s1" client={c} onBack={vi.fn()} />);
    await user.click(await screen.findByRole("button", { name: "Renomear" }));
    const input = screen.getByRole("textbox", { name: "Título" });
    await user.clear(input);
    await user.type(input, "Export final{Enter}");
    expect(c.sessionRename).toHaveBeenCalledWith("s1", "Export final");
    expect(
      await screen.findByRole("heading", { name: /Export final/ }),
    ).toBeTruthy();

    await user.click(
      screen.getByRole("button", { name: "Executar análise final" }),
    );
    expect(c.analysisRunFinal).toHaveBeenCalledWith("s1");
    expect(c.sessionSubscribe).toHaveBeenCalled();
  });
});
