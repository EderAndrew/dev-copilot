// CSP violation reporting (specs/002 T019, research R1, contracts/ipc-updates.md): the webview
// sends only the directive, the blocked scheme and the disposition — never the sample or the URL.
import { describe, expect, it, vi } from "vitest";

import type { CspViolationDto } from "../ipc/types";
import { installCspReporter, toCspViolationDto } from "./cspReporter";

type ViolationFields = {
  effectiveDirective: string;
  blockedURI: string;
  disposition: string;
  sample?: string;
  documentURI?: string;
  sourceFile?: string;
};

function violation(fields: ViolationFields): Event {
  const event = new Event("securitypolicyviolation");
  for (const [key, value] of Object.entries(fields)) {
    Object.defineProperty(event, key, { value });
  }
  return event;
}

// The DOM type of `disposition`, without naming a global the linter does not know.
type Disposition = Parameters<typeof toCspViolationDto>[0]["disposition"];

const flush = () => new Promise((resolve) => setTimeout(resolve, 0));

describe("toCspViolationDto", () => {
  it("keeps known directives and maps unknown ones to other", () => {
    for (const directive of [
      "style-src",
      "style-src-elem",
      "style-src-attr",
      "script-src",
      "script-src-elem",
      "img-src",
      "font-src",
      "connect-src",
      "default-src",
    ] as const) {
      expect(
        toCspViolationDto({
          effectiveDirective: directive,
          blockedURI: "inline",
          disposition: "enforce",
        }).effectiveDirective,
      ).toBe(directive);
    }
    for (const directive of ["frame-src", "worker-src", "", "STYLE-SRC "]) {
      expect(
        toCspViolationDto({
          effectiveDirective: directive,
          blockedURI: "inline",
          disposition: "enforce",
        }).effectiveDirective,
      ).toBe("other");
    }
  });

  it("reduces the blocked URI to its scheme, inline or eval", () => {
    const cases: [string, string][] = [
      ["inline", "inline"],
      ["", "inline"],
      ["eval", "eval"],
      ["wasm-eval", "eval"],
      ["https://cdn.example.com/a.css?token=abc", "https"],
      ["HTTP://Example.com/", "http"],
      ["data:text/css;base64,Ym9keXt9", "data"],
      ["data", "data"],
      ["blob:http://localhost/1234", "blob"],
      ["asset://localhost/x.png", "asset"],
      ["averyveryverylongscheme://x", "other"],
      ["not a uri", "other"],
      ["1http://x", "other"],
    ];
    for (const [blockedURI, scheme] of cases) {
      expect(
        toCspViolationDto({
          effectiveDirective: "img-src",
          blockedURI,
          disposition: "enforce",
        }).blockedScheme,
        blockedURI,
      ).toBe(scheme);
    }
  });

  it("maps the disposition", () => {
    const base = { effectiveDirective: "style-src", blockedURI: "inline" };
    expect(
      toCspViolationDto({ ...base, disposition: "report" }).disposition,
    ).toBe("report");
    expect(
      toCspViolationDto({ ...base, disposition: "enforce" }).disposition,
    ).toBe("enforce");
    expect(
      toCspViolationDto({
        ...base,
        disposition: "odd" as Disposition,
      }).disposition,
    ).toBe("enforce");
  });
});

describe("installCspReporter", () => {
  it("sends only the minimal DTO, never the sample or the full URL", async () => {
    const sent: CspViolationDto[] = [];
    const uninstall = installCspReporter(document, async (dto) => {
      sent.push(dto);
    });
    document.dispatchEvent(
      violation({
        effectiveDirective: "style-src-elem",
        blockedURI: "https://evil.example/steal.css?q=SECRET-TOKEN",
        disposition: "enforce",
        sample: "body { background: url(SECRET-SAMPLE) }",
        documentURI: "http://tauri.localhost/sessions/SECRET-ID",
        sourceFile: "https://evil.example/src.js",
      }),
    );
    await flush();
    uninstall();

    expect(sent).toEqual([
      {
        effectiveDirective: "style-src-elem",
        blockedScheme: "https",
        disposition: "enforce",
      },
    ]);
    expect(Object.keys(sent[0]).sort()).toEqual([
      "blockedScheme",
      "disposition",
      "effectiveDirective",
    ]);
    const payload = JSON.stringify(sent);
    for (const secret of [
      "evil.example",
      "SECRET",
      "background",
      "tauri.localhost",
    ]) {
      expect(payload).not.toContain(secret);
    }
  });

  it("stops listening once uninstalled", async () => {
    const report = vi.fn(async () => {});
    const uninstall = installCspReporter(document, report);
    uninstall();
    document.dispatchEvent(
      violation({
        effectiveDirective: "img-src",
        blockedURI: "data",
        disposition: "enforce",
      }),
    );
    await flush();
    expect(report).not.toHaveBeenCalled();
  });

  // Vitest fails the run on any unhandled rejection, so a leaked IPC error fails this test.
  it("swallows IPC failures, rejected or thrown", async () => {
    const reject = vi.fn(() =>
      Promise.reject({
        code: "invalid_input",
        messageKey: "errors.invalidInput",
      }),
    );
    const fail = vi.fn((): Promise<void> => {
      throw new Error("ipc unavailable");
    });
    const rejecting = installCspReporter(document, reject);
    const throwing = installCspReporter(document, fail);
    const event = violation({
      effectiveDirective: "style-src-attr",
      blockedURI: "inline",
      disposition: "enforce",
    });
    expect(() => document.dispatchEvent(event)).not.toThrow();
    await flush();
    await flush();
    rejecting();
    throwing();
    expect(reject).toHaveBeenCalledTimes(1);
    expect(fail).toHaveBeenCalledTimes(1);
  });
});
