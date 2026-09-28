import js from "@eslint/js";
import globals from "globals";
import reactHooks from "eslint-plugin-react-hooks";
import reactRefresh from "eslint-plugin-react-refresh";
import tseslint from "typescript-eslint";

export default tseslint.config(
  { ignores: ["dist", "src-tauri", "src/bindings", "node_modules"] },
  {
    extends: [js.configs.recommended, ...tseslint.configs.recommended],
    files: ["**/*.{ts,tsx}"],
    languageOptions: {
      ecmaVersion: 2022,
      globals: globals.browser,
    },
    plugins: {
      "react-hooks": reactHooks,
      "react-refresh": reactRefresh,
    },
    rules: {
      ...reactHooks.configs.recommended.rules,
      "react-refresh/only-export-components": ["warn", { allowConstantExport: true }],
      "@typescript-eslint/no-unused-vars": ["error", { argsIgnorePattern: "^_", varsIgnorePattern: "^_" }],
      "no-restricted-syntax": [
        "error",
        {
          selector: "CallExpression[callee.name='invoke']",
          message: "Call the backend only through the typed client in src/lib/api.ts.",
        },
      ],
    },
  },
  {
    files: ["src/lib/api.ts"],
    rules: { "no-restricted-syntax": "off" },
  },
  {
    // The route table is configuration, not a hot-reloaded component module.
    files: ["src/router.tsx"],
    rules: { "react-refresh/only-export-components": "off" },
  },
);
