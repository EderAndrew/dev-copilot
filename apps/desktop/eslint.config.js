import tseslint from "typescript-eslint";
import { config } from "@repo/eslint-config/react-internal";

/** @type {import("eslint").Linter.Config[]} */
export default [
  {
    files: ["**/*.{js,ts,tsx}"],
  },
  ...config,
  // The shared config parses with Babel, which does not handle TSX here; use the TS parser.
  {
    files: ["**/*.{ts,tsx}"],
    languageOptions: {
      parser: tseslint.parser,
      parserOptions: { ecmaFeatures: { jsx: true } },
    },
    plugins: { "@typescript-eslint": tseslint.plugin },
    rules: {
      // The core rule flags parameter names inside TS function types; use the TS-aware one.
      "no-unused-vars": "off",
      "@typescript-eslint/no-unused-vars": "warn",
    },
  },
  {
    ignores: ["dist/**", "src-tauri/**", "coverage/**"],
  },
];
