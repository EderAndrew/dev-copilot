// Shared Vitest setup: unmount rendered trees between tests and start i18n in pt-BR.
import { cleanup } from "@testing-library/react";
import { afterEach } from "vitest";

import { initI18n } from "../i18n";

initI18n("pt-BR");

afterEach(() => {
  cleanup();
});
