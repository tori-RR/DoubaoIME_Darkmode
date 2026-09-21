// Optional local browser regression. No app IPC, user settings or vendor assets.
// Usage: node scripts/verify-icon-overlay.mjs <absolute playwright/index.mjs>
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath, pathToFileURL } from "node:url";

if (!process.argv[2]) throw new Error("Pass the installed Playwright index.mjs path");
const { chromium } = await import(pathToFileURL(process.argv[2]).href);
const root = new URL("../", import.meta.url);
const html = readFileSync(new URL("ui/index.html", root), "utf8")
  .replace(/<script\b[^>]*>[\s\S]*?<\/script>/g, "").replace(/<link\b[^>]*>/g, "");
const browser = await chromium.launch({ channel: "msedge", headless: true, args: ["--force-device-scale-factor=1.25"] });
try {
  const context = await browser.newContext({ viewport: { width: 464, height: 480 }, deviceScaleFactor: 1.25 });
  const page = await context.newPage();
  await page.setContent(html);
  await page.addStyleTag({ path: fileURLToPath(new URL("ui/styles.css", root)) });
  console.log(await page.locator(".action-row").ariaSnapshot());
  const rect = id => page.locator(`#${id}`).evaluate(el => el.getBoundingClientRect().toJSON());
  const opacity = id => page.locator(`#${id}`).evaluate(el => getComputedStyle(el).opacity);
  const shown = id => page.waitForFunction(id => getComputedStyle(document.getElementById(id)).opacity === "1", id);
  const toolbar = await rect("btn-pick-logo"), taskbar = await rect("btn-pick-taskbar-logo");
  assert.equal(toolbar.width, 38);
  assert.equal(taskbar.width, 38);
  const positions = await page.locator(".icon-label").evaluateAll(nodes => nodes.map(el => el.getBoundingClientRect().toJSON()));
  assert.ok(Math.abs(positions[0].left - toolbar.right - 8) < 0.05);
  assert.ok(Math.abs(positions[1].left - taskbar.right - 8) < 0.05);
  await page.waitForFunction(() => ["btn-clear-logo", "btn-clear-taskbar-logo"].every(id => getComputedStyle(document.getElementById(id)).opacity === "0"));
  for (const [picker, reset] of [["btn-pick-logo", "btn-clear-logo"], ["btn-pick-taskbar-logo", "btn-clear-taskbar-logo"]]) {
    await page.locator(`#${picker}`).hover();
    await shown(reset);
    const pick = await rect(picker), overlay = await rect(reset);
    assert.ok(Math.abs(overlay.left - pick.right) < 0.05, "no dead gap between picker and restore hit area");
    for (let x = pick.right - 1; x <= overlay.left + overlay.width / 2; x += 2) {
      await page.mouse.move(x, pick.top + pick.height / 2);
      assert.equal(await opacity(reset), "1", "crossing the old gap must not collapse the restore button");
    }
    await page.locator(`#${reset}`).evaluate(el => { window.restoreClicks = 0; el.addEventListener("click", () => window.restoreClicks++); });
    await page.locator(`#${reset}`).click();
    assert.equal(await page.evaluate(() => window.restoreClicks), 1);
    await page.locator(`#${reset}`).evaluate(el => el.blur());
    await page.mouse.move(5, 5);
    await page.waitForFunction(id => getComputedStyle(document.getElementById(id)).opacity === "0", reset);
    await page.locator(`#${picker}`).focus();
    await shown(reset);
    await page.keyboard.press("Tab");
    assert.equal(await page.evaluate(() => document.activeElement.id), reset);
    assert.equal(await opacity(reset), "1");
    assert.equal(await page.locator(`#${reset}`).evaluate(el => getComputedStyle(el).outlineOffset), "-4px");
    await page.keyboard.press("Shift+Tab");
    assert.equal(await page.evaluate(() => document.activeElement.id), picker);
    await page.locator(`#${picker}`).evaluate(el => el.blur());
  }
  await page.locator("#btn-pick-taskbar-logo").hover();
  await shown("btn-clear-taskbar-logo");
  const overlay = await rect("btn-clear-taskbar-logo"), tip = await rect("taskbar-icon-tip");
  assert.ok(overlay.right <= tip.left + 0.05, "restore overlay must not cover the taskbar hint");
  await page.locator("#taskbar-icon-tip").hover();
  await page.waitForFunction(() => getComputedStyle(document.getElementById("btn-clear-taskbar-logo")).opacity === "0");
  assert.equal(await page.locator("#taskbar-icon-tip-text").isVisible(), true);
  await page.mouse.move(5, 5);
  await page.locator("#taskbar-icon-tip").focus();
  assert.equal(await page.locator("#taskbar-icon-tip-text").isVisible(), true);
  await page.locator("#taskbar-icon-tip").evaluate(el => el.blur());
  await page.locator("#btn-clear-logo").evaluate(el => { el.disabled = true; });
  await page.locator("#btn-pick-logo").hover();
  await shown("btn-clear-logo");
  const disabled = await page.locator("#btn-clear-logo").evaluate(el => ({ background: getComputedStyle(el).backgroundImage, pointer: getComputedStyle(el).pointerEvents, glyphOpacity: getComputedStyle(el.querySelector("svg")).opacity }));
  assert.match(disabled.background, /^linear-gradient/);
  assert.equal(disabled.pointer, "none");
  assert.equal(disabled.glyphOpacity, "0.45");
  assert.deepEqual(await rect("btn-pick-logo"), toolbar, "hover and focus must not shift layout");
  assert.deepEqual(await rect("btn-pick-taskbar-logo"), taskbar);
  if (process.env.PREVIEW_OVERLAY_SCREENSHOT) await page.locator(".action-row").screenshot({ path: process.env.PREVIEW_OVERLAY_SCREENSHOT });
  console.log("Overlay geometry, continuous hover, click, keyboard, disabled mask and hint checks passed.");
} finally { await browser.close(); }
