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
      // The production CSP has no 'unsafe-inline' in style-src (specs/002, research R1): inline
      // `style` props would be blocked at runtime, and raw HTML bypasses React's escaping.
      // Core rule instead of eslint-plugin-react, which adds a dependency for two checks.
      "no-restricted-syntax": [
        "error",
        {
          selector: "JSXAttribute[name.name='style']",
          message: "Inline styles are blocked by the production CSP. Use a CSS class in styles.css.",
        },
        {
          selector: "JSXAttribute[name.name='dangerouslySetInnerHTML']",
          message: "Raw HTML is not allowed; render text through React.",
        },
      ],
    },
  },
  {
    ignores: ["dist/**", "src-tauri/**", "coverage/**"],
  },
];
