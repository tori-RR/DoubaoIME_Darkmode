// Optional real-Chromium regression, using an existing local Playwright runtime.
// Usage: node scripts/verify-preview-pixels.mjs <absolute playwright/index.mjs>
// Synthetic pixels only: no vendor images, configuration or installation writes.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { fileURLToPath, pathToFileURL } from "node:url";

if (!process.argv[2]) throw new Error("Pass the installed Playwright index.mjs path");
const { chromium } = await import(pathToFileURL(process.argv[2]).href);
const { PNG } = createRequire(pathToFileURL(process.argv[2]))("pngjs");
const root = new URL("../", import.meta.url);
const html = readFileSync(new URL("ui/index.html", root), "utf8")
  .replace(/<script\b[^>]*>[\s\S]*?<\/script>/g, "")
  .replace(/<link\b[^>]*>/g, "");
const image = new PNG({ width: 1024, height: 1024 });
for (let n = 0; n < image.data.length; n += 4) image.data.set([255, 0, 255, 255], n);
const source = `data:image/png;base64,${PNG.sync.write(image).toString("base64")}`;
for (let y = 0; y < image.height; y++) for (let x = 0; x < image.width; x++) {
  const n = (y * image.width + x) * 4;
  image.data.set([x % 256, y % 256, ((x >> 6) + (y >> 7)) % 2 ? 235 : 20, 255], n);
}
const texture = `data:image/png;base64,${PNG.sync.write(image).toString("base64")}`;
for (const dpr of [1, 1.25, 1.5, 1.75, 2]) {
  // Native scale matters: viewport-only DPR emulation keeps the 1x layout grid,
  // unlike the desktop WebView's fractional-DPI layout and rasterization.
  const browser = await chromium.launch({ channel: "msedge", headless: true, args: [`--force-device-scale-factor=${dpr}`] });
  try {
    const context = await browser.newContext({ viewport: { width: 464, height: 480 }, deviceScaleFactor: dpr });
    const page = await context.newPage();
    await page.setContent(html);
    await page.addStyleTag({ path: fileURLToPath(new URL("ui/styles.css", root)) });
    await page.evaluate(async src => {
      document.getElementById("preview-toolbar").hidden = false;
      for (const id of ["toolbar-logo", "logo-picker-preview", "taskbar-logo-picker-preview"]) {
        const image = document.getElementById(id);
        image.src = src;
        image.hidden = false;
        await image.decode();
      }
    }, source);
    await page.addScriptTag({ path: fileURLToPath(new URL("ui/preview-pixels.js", root)) });
    await page.evaluate(() => {
      window.PreviewPixels.schedule();
      return new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    });
    const rects = await page.evaluate(() => ["toolbar-logo", "logo-picker-preview", "taskbar-logo-picker-preview"].map(id => ({ id, ...document.getElementById(id).getBoundingClientRect().toJSON() })));
    const screenshot = PNG.sync.read(await page.screenshot());
    const result = [];
    for (const rect of rects) {
      const left = Math.floor(rect.left * dpr) - 2;
      const top = Math.floor(rect.top * dpr) - 2;
      const right = Math.ceil(rect.right * dpr) + 2;
      const bottom = Math.ceil(rect.bottom * dpr) + 2;
      const xs = [], ys = [];
      for (let y = top; y < bottom; y++) for (let x = left; x < right; x++) {
        const n = (y * screenshot.width + x) * 4;
        if (screenshot.data[n] === 255 && screenshot.data[n + 1] === 0 && screenshot.data[n + 2] === 255) { xs.push(x); ys.push(y); }
      }
      assert.ok(xs.length, `${rect.id}: expected visible synthetic pixels`);
      const painted = [Math.max(...xs) - Math.min(...xs) + 1, Math.max(...ys) - Math.min(...ys) + 1];
      assert.deepEqual(painted, [Math.round(26 * dpr), Math.round(26 * dpr)], `${dpr}× ${rect.id}: actual painted device pixels`);
      result.push({ id: rect.id, painted });
    }
    await page.evaluate(async src => {
      for (const id of ["toolbar-logo", "logo-picker-preview", "taskbar-logo-picker-preview"]) {
        const image = document.getElementById(id);
        image.src = src;
        await image.decode();
      }
      window.PreviewPixels.schedule();
      return new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    }, texture);
    const textured = PNG.sync.read(await page.screenshot());
    const crops = rects.map(rect => {
      const size = Math.round(26 * dpr), left = Math.round(rect.left * dpr), top = Math.round(rect.top * dpr);
      return Buffer.concat(Array.from({ length: size }, (_, y) => {
        const start = ((top + y) * textured.width + left) * 4;
        return textured.data.subarray(start, start + size * 4);
      }));
    });
    assert.deepEqual(crops[0], crops[1], `${dpr}× toolbar and picker texture pixels must match`);
    assert.deepEqual(crops[1], crops[2], `${dpr}× both picker texture pixels must match`);
    console.log(JSON.stringify({ dpr, result, identicalTexturePixels: true }));
    await context.close();
  } finally { await browser.close(); }
}
