import { describe, expect, it } from "vitest";

import en from "./en.json";
import ptBR from "./pt-BR.json";

function flatten(value: unknown, prefix = ""): Record<string, unknown> {
  if (typeof value !== "object" || value === null) return { [prefix]: value };
  return Object.entries(value).reduce<Record<string, unknown>>(
    (acc, [key, child]) => ({
      ...acc,
      ...flatten(child, prefix ? `${prefix}.${key}` : key),
    }),
    {},
  );
}

describe("i18n catalogs (SC-019)", () => {
  const pt = flatten(ptBR);
  const english = flatten(en);

  it("have identical key sets", () => {
    expect(Object.keys(english).sort()).toEqual(Object.keys(pt).sort());
  });

  it("have no empty translations", () => {
    for (const [key, value] of [
      ...Object.entries(pt),
      ...Object.entries(english),
    ]) {
      expect(typeof value === "string" && value.trim().length > 0, key).toBe(
        true,
      );
    }
  });

  it("cover every IPC error message key", () => {
    const codes = [
      "invalidInput",
      "notFound",
      "invalidState",
      "permissionDenied",
      "sourceUnavailable",
      "noTranscriber",
      "secretStoreUnavailable",
      "authorizationRequired",
      "providerError",
      "storageError",
      "internal",
    ];
    for (const code of codes) {
      expect(pt).toHaveProperty([`errors.${code}`]);
    }
  });
});
