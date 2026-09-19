import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { runInNewContext } from "node:vm";

// Execute the actual render function and registered click handler without a
// WebView or live IPC. Any accidental uninstall call is recorded, never run.
const source = readFileSync(new URL("../ui/app.js", import.meta.url), "utf8");
const render = source.slice(source.indexOf("function render("), source.indexOf("\nfunction call("));
const binding = source.slice(
  source.indexOf('$("btn-uninstall").addEventListener'),
  source.indexOf('$("btn-pick-logo").addEventListener'),
);

function clickUninstall(overrides = {}) {
  const effects = [];
  const elements = new Map();
  const get = (id) => {
    if (!elements.has(id)) elements.set(id, {
      disabled: false,
      addEventListener(event, handler) { this[event] = handler; },
    });
    return elements.get(id);
  };
  const status = {
    ime_installed: true, ime_compatible: true, skin_applied: false,
    can_restore: false, recovery_needed: false, review_mode: false,
    theme_id: "light", colors: {}, fonts: {}, ...overrides,
  };
  const context = {
    $: get, lastStatus: null, customPanel: null, glassPanel: null,
    window: { Kaomoji: { show: (group) => effects.push(["kaomoji", group]) } },
    call: (name) => effects.push(["ipc", name]), status,
  };
  for (const name of ["setWorkdirTip", "refreshThemeChoices", "setThemeInput",
    "closeColorCard", "setPickers", "setFontInputs", "setGlassInput",
    "paintThemeActions", "paintPreview", "paintIme", "paintPlugin"]) {
    context[name] = () => {};
  }
  runInNewContext(`${render}\n${binding}\nrender(status);`, context);
  const button = get("btn-uninstall");
  if (!button.disabled) button.click();
  return { effects, disabled: button.disabled };
}

for (const [name, status] of [
  ["fresh official skin", {}],
  ["official skin with retained backup", { can_restore: true }],
  ["IME not installed", { ime_installed: false, ime_compatible: false }],
  ["unknown state never attempts uninstall", { ime_compatible: false }],
]) {
  test(`${name}: only the original bad kaomoji feedback`, () => {
    assert.deepEqual(clickUninstall(status), { disabled: false, effects: [["kaomoji", "bad"]] });
  });
}

test("installed skin still reaches the restore command", () => {
  assert.deepEqual(clickUninstall({ skin_applied: true, can_restore: true }).effects,
    [["kaomoji", "cancel"], ["ipc", "uninstall"]]);
});

test("interrupted transactions and review mode retain their protection", () => {
  for (const status of [{ recovery_needed: true }, { review_mode: true },
    { skin_applied: true, can_restore: false }]) {
    assert.deepEqual(clickUninstall(status), { disabled: true, effects: [] });
  }
});
