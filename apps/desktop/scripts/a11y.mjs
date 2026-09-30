// Dev-only: run axe-core accessibility checks on the main MCPanel pages (use with
// scripts/cdp.mjs against `pnpm dev`). Prints violations grouped by rule.
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const axeSource = readFileSync(require.resolve("axe-core/axe.min.js"), "utf8");

async function scan(page, label) {
  await page.waitForTimeout(800);
  await page.evaluate(axeSource);
  const result = await page.evaluate(async () => {
    const r = await axe.run(document, { resultTypes: ["violations"] });
    return r.violations.map((v) => ({
      id: v.id,
      impact: v.impact,
      help: v.help,
      nodes: v.nodes.slice(0, 4).map((n) => n.target.join(" ")),
      count: v.nodes.length,
    }));
  });
  console.log(`\n=== ${label}: ${result.length} rule(s) violated`);
  for (const v of result) console.log(`- [${v.impact}] ${v.id} (${v.count}): ${v.help}\n    ${v.nodes.join("\n    ")}`);
  return result;
}

export default async (page) => {
  await page.goto("http://localhost:1420/");
  await page.waitForSelector("text=Dashboard", { timeout: 60000 });
  const all = [];
  all.push(...(await scan(page, "Dashboard")));
  for (const [label, href] of [
    ["Backups", "Backups"],
    ["Java", "Java runtimes"],
    ["Templates", "Templates"],
    ["Activity", "Activity"],
    ["Playit.gg", "Playit.gg"],
    ["Settings", "Settings"],
  ]) {
    await page.locator(`nav a[aria-label="${href}"]`).first().click();
    all.push(...(await scan(page, label)));
  }
  await page.getByText("Fake SMP").first().click();
  for (const tab of ["Manage", "Overview", "Console", "Files", "Players", "Plugins", "Bedrock", "Properties", "Backups", "Settings", "Activity"]) {
    await page.getByRole("link", { name: tab, exact: true }).last().click();
    all.push(...(await scan(page, `Server / ${tab}`)));
  }
  const ids = [...new Set(all.map((v) => v.id))];
  console.log(`\nDistinct rules violated: ${ids.join(", ") || "none"}`);
};
