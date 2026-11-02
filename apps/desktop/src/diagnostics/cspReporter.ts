// Reports CSP violations seen by the webview to the core log (specs/002 T025, research R1).
//
// Only three fields leave the webview: the directive, the blocked scheme (or `inline`/`eval`)
// and the disposition. The sample, the blocked URL and the document address are never sent, so a
// violation cannot leak session content into the log (Art. IV.7). Reporting is best effort: an
// IPC failure is swallowed, because diagnostics must never break the UI.
import { ipc } from "../ipc/client";
import type { CspDirective, CspViolationDto } from "../ipc/types";

const DIRECTIVES: ReadonlySet<string> = new Set<CspDirective>([
  "style-src",
  "style-src-elem",
  "style-src-attr",
  "script-src",
  "script-src-elem",
  "img-src",
  "font-src",
  "connect-src",
  "default-src",
]);

/** Same pattern the core checks: lowercase scheme of at most 16 characters. */
const SCHEME = /^[a-z][a-z0-9+.-]{0,15}$/;

type ViolationFields = Pick<
  SecurityPolicyViolationEvent,
  "effectiveDirective" | "blockedURI" | "disposition"
>;

function blockedScheme(blockedURI: string): string {
  if (blockedURI === "" || blockedURI === "inline") return "inline";
  if (blockedURI === "eval" || blockedURI === "wasm-eval") return "eval";
  const colon = blockedURI.indexOf(":");
  const scheme = (
    colon === -1 ? blockedURI : blockedURI.slice(0, colon)
  ).toLowerCase();
  return SCHEME.test(scheme) ? scheme : "other";
}

/** The minimal report for a violation event. */
export function toCspViolationDto(event: ViolationFields): CspViolationDto {
  return {
    effectiveDirective: DIRECTIVES.has(event.effectiveDirective)
      ? (event.effectiveDirective as CspDirective)
      : "other",
    blockedScheme: blockedScheme(event.blockedURI),
    disposition: event.disposition === "report" ? "report" : "enforce",
  };
}

type Report = (dto: CspViolationDto) => Promise<unknown>;

/**
 * Listens for `securitypolicyviolation` on `target` and reports each one. Returns a function
 * that removes the listener.
 */
export function installCspReporter(
  target: Pick<Document, "addEventListener" | "removeEventListener"> = document,
  report: Report = ipc.reportCspViolation,
): () => void {
  const listener = (event: Event) => {
    try {
      const dto = toCspViolationDto(event as SecurityPolicyViolationEvent);
      report(dto).catch(() => {});
    } catch {
      // Best effort: a failing report must never reach the UI.
    }
  };
  target.addEventListener("securitypolicyviolation", listener);
  return () => target.removeEventListener("securitypolicyviolation", listener);
}
