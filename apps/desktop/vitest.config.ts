import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  test: {
    environment: "jsdom",
    include: ["src/**/*.test.{ts,tsx}"],
    // Feature tests arrive from Phase 2 onwards; an empty suite must not fail CI.
    passWithNoTests: true,
  },
});
