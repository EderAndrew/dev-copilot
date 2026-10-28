import { describe, expect, it } from "vitest";

import type { SessionDto, SessionEvent } from "../../ipc/types";
import {
  initialSessionState,
  sessionReducer,
  type SessionViewState,
} from "./sessionStore";

const session: SessionDto = {
  id: "019a2b3c-4d5e-7f60-8a1b-2c3d4e5f6a7b",
  title: "Refinamento",
  language: "pt-BR",
  state: "idle",
  sources: ["microphone"],
  createdAt: 0,
  activeDurationMs: 0,
};

function apply(state: SessionViewState, ...events: SessionEvent[]) {
  return events.reduce(
    (current, event) => sessionReducer(current, { type: "event", event }),
    state,
  );
}

const loaded = sessionReducer(initialSessionState, {
  type: "loaded",
  session,
});

describe("sessionReducer", () => {
  it("applies state and indicator events in seq order", () => {
    const state = apply(
      loaded,
      { type: "state_changed", seq: 1, state: "starting" },
      { type: "state_changed", seq: 2, state: "capturing" },
      {
        type: "capture_indicator",
        seq: 3,
        active: true,
        paused: false,
        sources: ["microphone"],
      },
    );
    expect(state.session?.state).toBe("capturing");
    expect(state.indicator).toEqual({
      active: true,
      paused: false,
      sources: ["microphone"],
    });
    expect(state.lastSeq).toBe(3);
  });

  it("drops stale and duplicate events", () => {
    const state = apply(
      loaded,
      { type: "state_changed", seq: 5, state: "stopping" },
      { type: "state_changed", seq: 4, state: "capturing" },
      { type: "state_changed", seq: 5, state: "capturing" },
    );
    expect(state.session?.state).toBe("stopping");
  });

  it("records failures and source health", () => {
    const state = apply(
      loaded,
      {
        type: "source_status",
        seq: 1,
        source: "microphone",
        status: "permission_revoked",
      },
      {
        type: "state_changed",
        seq: 2,
        state: "failed",
        failure: "permission_denied",
      },
      { type: "storage_error", seq: 3, messageKey: "errors.storageError" },
    );
    expect(state.sourceHealth.microphone).toBe("permission_revoked");
    expect(state.session?.failure).toBe("permission_denied");
    expect(state.storageErrorKey).toBe("errors.storageError");
  });

  it("keeps seq when the same session is reloaded and resets on another", () => {
    const advanced = apply(loaded, {
      type: "state_changed",
      seq: 9,
      state: "starting",
    });
    const reloaded = sessionReducer(advanced, {
      type: "loaded",
      session: { ...session, state: "capturing" },
    });
    expect(reloaded.lastSeq).toBe(9);
    expect(reloaded.session?.state).toBe("capturing");
    const other = sessionReducer(advanced, {
      type: "loaded",
      session: { ...session, id: "other" },
    });
    expect(other.lastSeq).toBe(0);
  });
});
