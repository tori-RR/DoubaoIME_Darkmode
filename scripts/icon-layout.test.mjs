import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

const markup = readFileSync(new URL("../ui/index.html", import.meta.url), "utf8");
const styles = readFileSync(new URL("../ui/styles.css", import.meta.url), "utf8");
const config = JSON.parse(readFileSync(new URL("../src-tauri/tauri.conf.json", import.meta.url), "utf8"));

// These are source-level geometry guards; the packaged WebView still needs a visual check.
function declarations(selector) {
  const result = {};
  for (const match of styles.matchAll(/([^{}]+)\{([^{}]*)\}/g)) {
    const selectors = match[1].replace(/\/\*[\s\S]*?\*\//g, "").split(",").map((part) => part.trim());
    if (!selectors.includes(selector)) continue;
    for (const declaration of match[2].replace(/\/\*[\s\S]*?\*\//g, "").split(";")) {
      const colon = declaration.indexOf(":");
      if (colon !== -1) result[declaration.slice(0, colon).trim()] = declaration.slice(colon + 1).trim();
    }
  }
  return result;
}

function pixels(selector, property) {
  const value = declarations(selector)[property];
  assert.match(value ?? "", /^-?\d+(?:\.\d+)?px$/, `${selector} ${property} must have an explicit pixel budget`);
  return Number.parseFloat(value);
}

const actionMarkup = markup.slice(markup.indexOf('<div class="control-row action-row">'), markup.indexOf('<p id="status-note"'));

test("the action row keeps icon settings and install controls in two columns", () => {
  assert.equal(declarations(".action-row")["grid-template-columns"], "minmax(0, 1fr) auto");
  assert.match(actionMarkup, /<section class="icon-fields"[\s\S]*?<\/section>\s*<div class="row-actions">/);
  for (const selector of [".icon-fields", ".icon-field", ".row-actions"]) {
    assert.equal(declarations(selector).display, "flex");
    assert.notEqual(declarations(selector)["flex-wrap"], "wrap");
  }
});

test("the independent Lucide image glyph remains before both icon pickers", () => {
  const glyph = actionMarkup.match(/<svg\b[^>]*class="[^"]*\bicon-settings-glyph\b[^"]*"[^>]*>[\s\S]*?<\/svg>/)?.[0];
  assert.ok(glyph, "the row needs its own always-visible image glyph, not only button placeholders");
  assert.match(glyph, /\bfield-glyph\b/);
  assert.match(glyph, /width="16"/);
  assert.match(glyph, /height="16"/);
  assert.match(glyph, /<rect\b/);
  assert.match(glyph, /<circle\b/);
  assert.match(glyph, /<path\b/);
  assert.doesNotMatch(glyph, /\shidden(?:\s|=|\/?>)/);
  assert.ok(actionMarkup.indexOf(glyph) < actionMarkup.indexOf('class="icon-field"'));
  assert.ok(actionMarkup.indexOf(glyph) < actionMarkup.indexOf('id="btn-pick-logo"'));
  assert.ok(actionMarkup.indexOf(glyph) < actionMarkup.indexOf('id="btn-pick-taskbar-logo"'));
});

test("picker buttons stay square without shrinking and previews preserve their aspect ratio", () => {
  const picker = { ...declarations(".icon-picker"), ...declarations(".control-row .icon-picker") };
  assert.equal(declarations("*")["box-sizing"], "border-box");
  for (const property of ["width", "height", "min-width", "min-height", "max-width", "max-height"]) {
    assert.equal(picker[property], "38px", property);
  }
  assert.match(picker["aspect-ratio"], /^1(?:\s*\/\s*1)?$/);
  assert.match(picker.flex, /^(?:none|0 0 auto)$/);
  assert.equal(picker.padding, "0");
  const preview = declarations(".icon-picker img");
  assert.equal(preview.width, "26px");
  assert.equal(preview.height, "26px");
  assert.equal(preview["object-fit"], "contain");
  assert.match(preview["aspect-ratio"], /^1(?:\s*\/\s*1)?$/);
  assert.equal(declarations("[hidden]").display, "none !important");
  assert.equal(declarations(".icon-picker img:not([hidden]) + .icon-placeholder").display, "none");
});

test("the compact row fits without reserving layout space for restore overlays", () => {
  const appPadding = declarations(".app").padding.split(/\s+/).map(Number.parseFloat);
  const available = config.app.windows[0].width - pixels("body::-webkit-scrollbar", "width") - 2 * appPadding[1];
  const glyph = 16;
  const labelWidth = 3 * pixels(":root", "--label-size");
  const groupGap = pixels(".icon-field", "gap");
  const flyoutWidth = pixels(".icon-action-pop", "width");
  const toolbar = flyoutWidth + groupGap + labelWidth;
  const taskbar = toolbar + groupGap + 14 + pixels("#taskbar-icon-tip", "margin-left");
  const fields = glyph + toolbar + taskbar + 2 * pixels(".icon-fields", "gap");
  const actions = 2 * pixels(".row-actions button", "width") + pixels(".row-actions", "gap");
  const required = fields + pixels(".action-row", "column-gap") + actions;
  assert.ok(required <= available, `${required}px controls must fit ${available}px content without wrapping or shrinking`);
  assert.equal(flyoutWidth, pixels(".control-row .icon-picker", "width"), "only the square picker reserves layout space");
  const overlay = declarations(".control-row .logo-reset");
  assert.equal(overlay.position, "absolute");
  assert.equal(overlay.left, "100%", "overlay touches the picker, leaving no dead hover gap");
  assert.ok(pixels(".control-row .logo-reset", "width") >= groupGap + labelWidth, "overlay covers its label");
  assert.ok(pixels(".control-row .logo-reset", "width") <= groupGap + labelWidth + pixels(".icon-fields", "gap"), "overlay stops before the next picker");
});

test("restore overlays fade into the page and dim only the disabled glyph", () => {
  const overlay = declarations(".control-row .logo-reset");
  assert.match(overlay.background, /^linear-gradient\(to right, transparent, var\(--bg\) 8px, var\(--bg\) calc\(100% - 8px\), transparent\)$/);
  assert.equal(overlay["z-index"], "2");
  assert.equal(overlay["pointer-events"], "none");
  for (const trigger of ["hover", "focus-within"]) {
    assert.equal(declarations(`.icon-action-pop:${trigger} .logo-reset`).opacity, "1");
    assert.equal(declarations(`.icon-action-pop:${trigger} .logo-reset`)["pointer-events"], "auto");
    assert.equal(declarations(`.icon-action-pop:${trigger} .logo-reset:disabled`).opacity, "1", "disabled background must still mask the label");
  }
  assert.equal(declarations(".logo-reset:disabled svg").opacity, "0.45");
  assert.equal(declarations(".control-row .logo-reset:hover:not(:disabled)").background, undefined, "hover must retain the gradient");
});
