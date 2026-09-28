// Dev-only helper: drive the running MCPanel WebView over the Chrome DevTools Protocol.
// Requires `pnpm tauri dev` started with
//   WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222
// Usage: node scripts/cdp.mjs <script.mjs>   (script default-exports async (page) => {})
import { chromium } from "playwright-core";
import { pathToFileURL } from "node:url";

const browser = await chromium.connectOverCDP("http://127.0.0.1:9222");
const page = browser
  .contexts()
  .flatMap((c) => c.pages())
  .find((p) => !p.url().startsWith("devtools"));
if (!page) throw new Error("MCPanel page not found");
const mod = await import(pathToFileURL(process.argv[2]).href);
try {
  await mod.default(page);
} finally {
  await browser.close();
}
