import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { runInNewContext } from "node:vm";

const source = readFileSync(new URL("../ui/app.js", import.meta.url), "utf8");
const backend = readFileSync(new URL("../src-tauri/src/main.rs", import.meta.url), "utf8");
const paint = source.slice(source.indexOf("function setPluginHint("),
  source.indexOf("function paintSliderTracks("));
const render = source.slice(source.indexOf("function render("), source.indexOf("\nfunction call("));
const styles = readFileSync(new URL("../ui/styles.css", import.meta.url), "utf8");

function paintStatus(overrides = {}) {
  const context = {
    status: {
      ime_installed: true, ime_version: "v0.9.0.0",
      verified_ime_versions: ["v0.9.0.0"], ime_structure_matches: true,
      ime_structure_detail: "安装结构一致（29 个必需文件）",
      ime_compatible: true, skin_applied: false, installed_label: "官方浅色",
      ...overrides,
    },
  };
  for (const name of ["imeState", "imeStateText", "imeVersionTipText",
    "pluginState", "pluginText", "pluginTipText"]) context[name] = {};
  context.imeVersionTip = { classList: { toggle: (_, off) => { context.tipOff = off; } } };
  context.pluginThemeTip = { classList: { toggle: (_, off) => { context.pluginTipOff = off; } } };
  runInNewContext(`${paint}\npaintIme(status); paintPlugin(status);`, context);
  return context;
}

function installDisabled(overrides = {}) {
  const elements = new Map();
  const get = (id) => {
    if (!elements.has(id)) elements.set(id, {});
    return elements.get(id);
  };
  const status = {
    ime_installed: true, ime_version: "v0.10.0.0",
    verified_ime_versions: ["v0.9.0.0"], ime_structure_matches: true,
    ime_compatible: true, skin_applied: false, recovery_needed: false,
    can_restore: false, review_mode: false, has_custom_logo: false,
    warning: "", colors: {}, fonts: {}, theme_id: "dark", glass_opacity: 100,
    custom_themes: [], ...overrides,
  };
  const context = { $: get, lastStatus: null, customPanel: null, glassPanel: null, status };
  for (const name of ["setWorkdirTip", "refreshThemeChoices", "setThemeInput",
    "closeColorCard", "setPickers", "setFontInputs", "setGlassInput",
    "paintThemeActions", "paintPreview", "paintIme", "paintPlugin"]) {
    context[name] = () => {};
  }
  runInNewContext(`${render}\nrender(status);`, context);
  return get("btn-install").disabled;
}

test("uninstalled plugin has no theme tooltip", () => {
  const view = paintStatus();
  assert.equal(view.pluginText.textContent, "未安装");
  assert.equal(view.pluginTipText.textContent, "");
  assert.equal(view.pluginTipOff, true);
  assert.equal(view.pluginThemeTip.tabIndex, -1);
});

test("installed tooltip describes the installed theme, never the preview", () => {
  const view = paintStatus({ skin_applied: true, installed_label: "Dark 70%", theme_id: "light" });
  assert.equal(view.pluginText.textContent, "已安装");
  assert.equal(view.pluginTipText.textContent, "Dark 70%");
  assert.equal(view.pluginTipOff, false);
  assert.equal(view.pluginThemeTip.tabIndex, 0);
});

for (const verified of [true, false]) {
  for (const structure of [true, false]) {
    test(`release verified=${verified}, structure matches=${structure} are independent`, () => {
      const version = verified ? "v0.9.0.0" : "v0.10.0.0";
      const view = paintStatus({ ime_version: version, ime_structure_matches: structure,
        ime_structure_detail: structure ? "安装结构一致（29 个必需文件）" : "必需文件 window.xml 缺失",
        ime_compatible: false });
      assert.equal(view.imeStateText.textContent, version);
      assert.equal(view.imeState.className, `plugin-state ${structure ? "on" : "warn"}`);
      assert.equal(view.imeVersionTipText.textContent,
        `${verified ? "已验证" : "未验证"}${structure ? "" : "\n可能不兼容"}`);
    });
  }
}

test("older releases and missing coverage metadata never imply verification", () => {
  for (const status of [
    { ime_version: "v0.8.0.0" },
    { verified_ime_versions: undefined, verified_ime_version: undefined },
    { verified_ime_versions: [], verified_ime_version: "v0.9.0.0" },
  ]) assert.equal(paintStatus(status).imeVersionTipText.textContent, "未验证");
  assert.equal(paintStatus({ verified_ime_versions: ["v0.8.0.0", "v0.9.0.0"],
    ime_version: "v0.8.0.0" }).imeVersionTipText.textContent, "已验证");
});

test("unverified but compatible structure remains installable", () => {
  assert.equal(installDisabled(), false);
  assert.equal(installDisabled({ ime_compatible: false, ime_structure_matches: false }), true);
  assert.equal(installDisabled({ review_mode: true }), true);
});

test("no IME and ambiguous version have honest fallback text", () => {
  const missing = paintStatus({ ime_installed: false });
  assert.equal(missing.imeStateText.textContent, "未安装");
  assert.equal(missing.imeVersionTipText.textContent, "");
  assert.equal(missing.tipOff, true);
  assert.equal(missing.pluginTipText.textContent, "");
  assert.equal(missing.pluginTipOff, true);
  const unknown = paintStatus({ ime_version: null, ime_structure_matches: false,
    ime_structure_detail: "无法检测安装结构：多个版本正在运行" });
  assert.equal(unknown.imeStateText.textContent, "版本未知");
  assert.equal(unknown.imeVersionTipText.textContent, "未验证\n可能不兼容");
});

test("recovery and transient states do not show stale theme tooltips", () => {
  const recovery = paintStatus({ recovery_needed: true, installed_label: "操作中断，点击恢复" });
  assert.equal(recovery.pluginState.className, "plugin-state warn");
  assert.equal(recovery.pluginText.textContent, "需要恢复");
  assert.equal(recovery.pluginTipText.textContent, "");
  assert.equal(recovery.pluginTipOff, true);
  runInNewContext(`${paint}\nsetPluginHint("wait", "等待授权");`, recovery);
  assert.equal(recovery.pluginText.textContent, "等待授权");
  assert.equal(recovery.pluginTipText.textContent, "");
  assert.equal(recovery.pluginTipOff, true);
  assert.match(source,
    /setPluginHint\("wait", name === "import_logo" \? "处理头像…" : "等待授权"\)/);
});

test("tooltips use the requested directions and compact cache label", () => {
  const context = {
    workdirTipText: {}, lastStatus: { workdir: String.raw`D:\actual\cache` },
    WORKDIR: String.raw`C:\fallback`,
  };
  runInNewContext(`${paint}\nsetWorkdirTip();`, context);
  assert.equal(context.workdirTipText.textContent, String.raw`默认缓存目录：D:\actual\cache`);
  assert.match(styles, /#ime-version-tip \.ime-tip-bubble,[\s\S]*bottom: calc\(100% \+ 6px\)/);
  assert.match(styles, /#workdir-tip \.ime-tip-bubble[\s\S]*left: calc\(100% \+ 8px\)[\s\S]*white-space: nowrap/);
});

test("GitHub menu button stays visible and both launch paths use the same URL", () => {
  const frontendUrl = source.match(/const PUBLIC_REPO_URL = "([^"]+)"/)?.[1];
  const backendUrl = backend.match(/const PUBLIC_REPO_URL: &str = "([^"]+)"/)?.[1];
  assert.equal(frontendUrl, "https://github.com");
  assert.equal(backendUrl, frontendUrl);
  assert.match(source, /repo\.hidden = !PUBLIC_REPO_URL/);
});
