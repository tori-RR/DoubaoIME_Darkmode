const invoke = (...args) => window.__TAURI__.core.invoke(...args);

const $ = (id) => document.getElementById(id);
const imeState = $("ime-state");
const imeStateText = $("ime-state-text");
const imeVersionTip = $("ime-version-tip");
const imeVersionTipText = $("ime-version-tip-text");
const workdirTipText = $("workdir-tip-text");
const WORKDIR = String.raw`C:\tmp\DoubaoIME Darkmode`;
const PUBLIC_REPO_URL = "https://github.com/tori-RR/DoubaoIME_Darkmode";
const errorEl = $("error");
const customPanel = $("custom-panel");
const glassPanel = $("glass-panel");
const previewBar = $("preview-bar");
const pluginState = $("plugin-state");
const pluginText = $("plugin-state-text");
const pluginThemeTip = $("plugin-theme-tip");
const pluginTipText = $("plugin-theme-tip-text");

const fields = {
  accent: [$("accent"), $("accent-hex")],
  background: [$("background"), $("background-hex")],
  foreground: [$("foreground"), $("foreground-hex")],
  emphasis: [$("emphasis"), $("emphasis-hex")],
};
const COLOR_LABELS = {
  accent: "主题色",
  background: "背景色",
  foreground: "字体颜色",
  emphasis: "强调色",
};
const colorCard = $("color-card");
const colorPreview = $("color-preview");
const colorHex = $("color-hex");
const colorEyedrop = $("color-eyedrop");
const colorR = $("color-r");
const colorG = $("color-g");
const colorB = $("color-b");
const colorH = $("color-h");
const colorS = $("color-s");
const colorL = $("color-l");
let colorCardKey = "";
let colorSyncing = false;
let colorEyedropping = false;
let lastHue = 0;

let flashTimer = 0;
let lastStatus = null;
let inflight = null;
let commandQueue = Promise.resolve();
let holdPlusHome = false;

function showError(msg) {
  errorEl.hidden = !msg;
  errorEl.textContent = msg || "";
}

function uninstallError(err) {
  const raw = String(err || "");
  if (
    raw.includes("未检测到安装") ||
    raw.includes("没有找到官方备份") ||
    raw.includes("未找到豆包输入法")
  ) {
    return "未检测到安装";
  }
  return raw;
}

function cssFace(name) {
  return `"${String(name).replace(/\\/g, "\\\\").replace(/"/g, '\\"')}"`;
}

function clampGlassOpacity(pct) {
  const n = Number(pct);
  if (!Number.isFinite(n)) return 100;
  return Math.min(100, Math.max(20, Math.round(n)));
}

function glassEnabled(pct) {
  return clampGlassOpacity(pct) < 100;
}

function readGlassOpacity() {
  return clampGlassOpacity($("glass-opacity") && $("glass-opacity").value);
}

function setGlassInput(pct) {
  const slider = $("glass-opacity");
  const label = $("glass-opacity-val");
  if (!slider) return;
  const value = clampGlassOpacity(pct);
  if (document.activeElement !== slider) slider.value = String(value);
  if (label) label.textContent = `${value}%`;
}

function parseHex(value) {
  const raw = String(value || "").trim();
  const hex = raw.startsWith("#") ? raw : `#${raw}`;
  return /^#[0-9A-Fa-f]{6}$/.test(hex) ? hex.toUpperCase() : "";
}

function hexToRgb(hex) {
  const next = parseHex(hex);
  if (!next) return null;
  const n = Number.parseInt(next.slice(1), 16);
  return { r: (n >> 16) & 255, g: (n >> 8) & 255, b: n & 255 };
}

function rgbToHex(r, g, b) {
  const to = (n) => Math.max(0, Math.min(255, Math.round(Number(n) || 0))).toString(16).padStart(2, "0");
  return `#${to(r)}${to(g)}${to(b)}`.toUpperCase();
}

function clampByte(value) {
  const n = Number(value);
  if (!Number.isFinite(n)) return null;
  return Math.max(0, Math.min(255, Math.round(n)));
}

function rgbToHsl(r, g, b) {
  const rr = r / 255;
  const gg = g / 255;
  const bb = b / 255;
  const max = Math.max(rr, gg, bb);
  const min = Math.min(rr, gg, bb);
  const l = (max + min) / 2;
  let h = lastHue;
  let s = 0;
  if (max !== min) {
    const d = max - min;
    s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
    if (max === rr) h = ((gg - bb) / d + (gg < bb ? 6 : 0)) * 60;
    else if (max === gg) h = ((bb - rr) / d + 2) * 60;
    else h = ((rr - gg) / d + 4) * 60;
  }
  return { h, s: s * 100, l: l * 100 };
}

function hslToRgb(h, s, l) {
  const hh = ((Number(h) % 360) + 360) % 360;
  const ss = Math.max(0, Math.min(100, Number(s))) / 100;
  const ll = Math.max(0, Math.min(100, Number(l))) / 100;
  const c = (1 - Math.abs(2 * ll - 1)) * ss;
  const x = c * (1 - Math.abs(((hh / 60) % 2) - 1));
  const m = ll - c / 2;
  let r = 0;
  let g = 0;
  let b = 0;
  if (hh < 60) [r, g, b] = [c, x, 0];
  else if (hh < 120) [r, g, b] = [x, c, 0];
  else if (hh < 180) [r, g, b] = [0, c, x];
  else if (hh < 240) [r, g, b] = [0, x, c];
  else if (hh < 300) [r, g, b] = [x, 0, c];
  else [r, g, b] = [c, 0, x];
  return {
    r: Math.round((r + m) * 255),
    g: Math.round((g + m) * 255),
    b: Math.round((b + m) * 255),
  };
}

function hexRgba(hex, alpha) {
  const h = String(hex || "").replace("#", "");
  if (h.length !== 6) return hex;
  const n = Number.parseInt(h, 16);
  if (!Number.isFinite(n)) return hex;
  return `rgba(${(n >> 16) & 255}, ${(n >> 8) & 255}, ${n & 255}, ${alpha})`;
}

function paintPreview(colors, fonts, themeId, opacity) {
  document.documentElement.style.setProperty("--preview-bg", colors.background);
  document.documentElement.style.setProperty("--preview-fg", colors.foreground);
  document.documentElement.style.setProperty("--emphasis", colors.emphasis || "#FFFFFF");
  if (fonts && fonts.face) {
    document.documentElement.style.setProperty("--font-face", cssFace(fonts.face));
  }
  const barPct = clampGlassOpacity(
    opacity ?? (lastStatus && lastStatus.glass_opacity) ?? readGlassOpacity()
  );
  const alpha = barPct / 100;
  document.documentElement.style.setProperty("--accent", hexRgba(colors.accent, alpha));
  previewBar.style.background = hexRgba(colors.background, alpha);
  syncToolbarOpacity(barPct);
  if (lastStatus) {
    const next = {
      ...lastStatus,
      colors,
      theme_id: themeId || lastStatus.theme_id,
      glass_opacity: barPct,
    };
    if (toolbarKey(next, barPct) !== toolbarCacheKey) {
      paintLogo(next);
    }
  }
}

function paintSwatches(colors) {
  for (const key of Object.keys(fields)) {
    const swatch = $(`swatch-${key}`);
    if (swatch && colors[key]) swatch.style.background = colors[key];
  }
}

function setPickers(colors) {
  for (const [key, [picker, hex]] of Object.entries(fields)) {
    const value = colors[key] || (key === "emphasis" ? "#FFFFFF" : "");
    if (!value || !picker || !hex) continue;
    picker.value = value.toLowerCase();
    hex.value = value.toUpperCase();
  }
  paintSwatches(colors);
  if (colorCard && !colorCard.hidden && colorCardKey && colors[colorCardKey]) {
    fillColorEditor(colors[colorCardKey], "state");
  }
}

function readPickers() {
  return {
    accent: $("accent-hex").value,
    background: $("background-hex").value,
    foreground: $("foreground-hex").value,
    emphasis: $("emphasis-hex").value || "#FFFFFF",
  };
}

function setPluginHint(kind, text, hint = "") {
  pluginState.className = `plugin-state ${kind}`;
  pluginText.textContent = text;
  if (pluginTipText) pluginTipText.textContent = hint;
  if (pluginThemeTip) {
    pluginThemeTip.classList.toggle("is-off", !hint);
    pluginThemeTip.tabIndex = hint ? 0 : -1;
  }
}

function isVerifiedImeVersion(status) {
  // Release coverage is explicit, not a numeric version range. Older does not
  // imply tested, and missing metadata must never imply verification.
  const versions = status?.verified_ime_versions
    ?? [status?.verified_ime_version].filter(Boolean);
  return !!status?.ime_version && versions.includes(status.ime_version);
}

function setImeTip(status, show) {
  if (!imeVersionTip) return;
  imeVersionTip.classList.toggle("is-off", !show);
  if (imeVersionTipText) {
    const verification = isVerifiedImeVersion(status) ? "已验证" : "未验证";
    imeVersionTipText.textContent = show
      ? `${verification}${status?.ime_structure_matches ? "" : "\n可能不兼容"}`
      : "";
  }
}

function setWorkdirTip() {
  if (!workdirTipText) return;
  workdirTipText.textContent = `默认缓存目录：${lastStatus?.workdir || WORKDIR}`;
}

function paintIme(status) {
  if (!status.ime_installed) {
    imeState.className = "plugin-state off";
    imeStateText.textContent = "未安装";
    setImeTip(status, false);
    return;
  }
  imeStateText.textContent = status.ime_version || "版本未知";
  setImeTip(status, true);
  if (status.ime_structure_matches) {
    imeState.className = "plugin-state on";
  } else {
    imeState.className = "plugin-state warn";
  }
}

function paintPlugin(status) {
  if (!status.ime_installed) {
    setPluginHint("off", "未安装");
    return;
  }
  if (status.recovery_needed) {
    setPluginHint("warn", "需要恢复");
  } else if (status.skin_applied) {
    setPluginHint("on", "已安装", status.installed_label || "未知（旧版安装）");
  } else {
    setPluginHint("off", "未安装");
  }
}

function paintSliderTracks(h, s, l) {
  if (colorH) {
    colorH.style.background =
      "linear-gradient(90deg,#f00,#ff0,#0f0,#0ff,#00f,#f0f,#f00)";
  }
  if (colorS) {
    colorS.style.background = `linear-gradient(90deg,hsl(${h},0%,${l}%),hsl(${h},100%,${l}%))`;
  }
  if (colorL) {
    colorL.style.background = `linear-gradient(90deg,#000,hsl(${h},${s}%,50%),#fff)`;
  }
}

function fillColorEditor(hex, from) {
  const rgb = hexToRgb(hex);
  if (!rgb) return;
  const hsl = rgbToHsl(rgb.r, rgb.g, rgb.b);
  if (hsl.s >= 1 && hsl.l > 0.5 && hsl.l < 99.5) lastHue = hsl.h;
  const h = from === "hsl" && colorH
    ? Number(colorH.value)
    : Math.round(hsl.s < 1 ? lastHue : hsl.h);
  const s = from === "hsl" && colorS ? Number(colorS.value) : Math.round(hsl.s);
  const l = from === "hsl" && colorL ? Number(colorL.value) : Math.round(hsl.l);
  const next = rgbToHex(rgb.r, rgb.g, rgb.b);
  colorSyncing = true;
  if (from !== "hex" && colorHex) colorHex.value = next;
  if (from !== "rgb") {
    if (colorR) colorR.value = String(rgb.r);
    if (colorG) colorG.value = String(rgb.g);
    if (colorB) colorB.value = String(rgb.b);
  }
  if (from !== "hsl") {
    if (colorH) colorH.value = String(h);
    if (colorS) colorS.value = String(s);
    if (colorL) colorL.value = String(l);
  }
  if (colorPreview) colorPreview.style.background = next;
  paintSliderTracks(h, s, l);
  colorSyncing = false;
}

function writeActiveColor(hex) {
  const pair = fields[colorCardKey];
  if (!pair) return;
  const [picker, field] = pair;
  const next = parseHex(hex);
  if (!next) return;
  picker.value = next.toLowerCase();
  field.value = next;
  paintSwatches(readPickers());
  paintPreview(readPickers(), readFonts(), lastStatus ? lastStatus.theme_id : "dark");
}

function persistActiveColor() {
  /* Color drafts stay on the card until plus / save-pen. */
}

function applyColor(hex, from, persist) {
  const next = parseHex(hex);
  if (!next) return;
  fillColorEditor(next, from);
  writeActiveColor(next);
  if (persist) persistActiveColor();
}

function closeColorCard() {
  if (!colorCard) return;
  colorCard.hidden = true;
  colorCardKey = "";
  document.querySelectorAll(".swatch").forEach((el) => el.setAttribute("aria-expanded", "false"));
}

function placeColorCard(anchor) {
  if (!colorCard || !anchor) return;
  const rect = anchor.getBoundingClientRect();
  const gap = 8;
  const width = colorCard.offsetWidth || 240;
  const height = colorCard.offsetHeight || 180;
  let left = Math.round(rect.left);
  left = Math.min(left, window.innerWidth - width - 8);
  left = Math.max(8, left);
  const below = window.innerHeight - rect.bottom - gap - 8;
  const openUp = below < height && rect.top - gap - 8 > below;
  colorCard.style.left = `${left}px`;
  colorCard.style.right = "auto";
  if (openUp) {
    colorCard.style.top = "auto";
    colorCard.style.bottom = `${Math.round(window.innerHeight - rect.top + gap)}px`;
  } else {
    colorCard.style.top = `${Math.round(rect.bottom + gap)}px`;
    colorCard.style.bottom = "auto";
  }
}

function openColorCard(key, anchor) {
  if (!colorCard || !fields[key]) return;
  if (colorCardKey === key) {
    closeColorCard();
    return;
  }
  colorCardKey = key;
  colorCard.setAttribute("aria-label", COLOR_LABELS[key] || "自定义颜色");
  document.querySelectorAll(".swatch").forEach((el) => {
    el.setAttribute("aria-expanded", el.dataset.key === key ? "true" : "false");
  });
  colorCard.hidden = false;
  fillColorEditor(fields[key][1].value || fields[key][0].value, "state");
  placeColorCard(anchor);
  window.requestAnimationFrame(() => placeColorCard(anchor));
  if (colorHex) colorHex.focus();
}

function flashUninstalled(status) {
  window.clearTimeout(flashTimer);
  setPluginHint("removed", "插件已卸载");
  flashTimer = window.setTimeout(() => {
    paintPlugin(status);
  }, 3000);
}

function render(status, { keepPlugin, keepColors } = {}) {
  const prevId = lastStatus && lastStatus.theme_id;
  const colors = keepColors ? readPickers() : status.colors;
  lastStatus = keepColors ? { ...status, colors } : status;
  const ready = status.ime_installed;
  $("btn-install").disabled = !!status.review_mode || !ready || (!status.ime_compatible && !status.recovery_needed);
  $("btn-install").textContent = status.recovery_needed ? "恢复" : "安装";
  const needsRestore = status.skin_applied || status.recovery_needed;
  $("btn-uninstall").disabled = !!status.review_mode || (needsRestore && !status.can_restore);
  $("btn-clear-logo").disabled = false;
  $("btn-install").title = status.review_mode ? "检查模式不写入真实输入法" : "备份并应用当前预览，需要管理员授权";
  $("btn-uninstall").title = "恢复官方皮肤；保留已验证的原件备份";
  $("status-note").textContent = status.warning || (status.review_mode ? "检查模式 · 设置仅用于本次预览" : "");
  $("status-note").hidden = !$("status-note").textContent;
  setWorkdirTip();
  refreshThemeChoices(status.custom_themes);
  setThemeInput(status.theme_id);
  if (prevId && prevId !== status.theme_id && !keepColors) closeColorCard();
  if (customPanel) customPanel.hidden = false;
  if (glassPanel) glassPanel.hidden = false;
  if (!keepColors) setPickers(status.colors);
  setFontInputs(status.fonts);
  setGlassInput(status.glass_opacity);
  paintThemeActions(status.theme_id);
  paintPreview(colors, status.fonts, status.theme_id, status.glass_opacity);
  paintIme(status);
  if (!keepPlugin) paintPlugin(status);
}

function call(name, payload) {
  if (inflight && name !== "get_status") return inflight;
  const busy = ["install", "uninstall", "import_logo"].includes(name);
  const run = async () => {
    showError("");
    const controls = busy ? [...document.querySelectorAll("input, button")].map(el => [el, el.disabled]) : [];
    if (busy) {
      controls.forEach(([el]) => { el.disabled = true; });
      closeColorCard();
      document.querySelectorAll(".combo-menu").forEach(el => { el.hidden = true; });
      document.querySelector("main").setAttribute("aria-busy", "true");
      setPluginHint("wait", name === "import_logo" ? "处理头像…" : "等待授权");
    }
    try {
      const status = await invoke(name, payload);
      const keepColors = ["delete_custom_theme", "install", "set_fonts", "set_glass_opacity", "import_logo", "clear_logo"].includes(name);
      window.clearTimeout(flashTimer);
      render(status, { keepColors, keepPlugin: name === "uninstall" });
      if (name === "uninstall") flashUninstalled(status);
      return status;
    } catch (err) {
      // A failed commit may require recovery; never keep displaying stale success.
      const actual = await invoke("get_status").catch(() => lastStatus);
      if (actual) render(actual, { keepColors: true });
      showError(name === "uninstall" ? uninstallError(err) : String(err));
      return null;
    } finally {
      if (busy) {
        controls.forEach(([el, disabled]) => { el.disabled = disabled; });
        document.querySelector("main").removeAttribute("aria-busy");
        if (lastStatus) render(lastStatus, { keepColors: true, keepPlugin: name === "uninstall" });
      }
    }
  };
  const result = commandQueue.then(run, run);
  commandQueue = result.catch(() => {});
  if (busy) {
    inflight = result;
    result.finally(() => { if (inflight === result) inflight = null; });
  }
  return result;
}

const MOCK_FONTS = {
  faces: [
    { name: "微软雅黑", aliases: ["Microsoft YaHei"], keys: ["microsoftyahei", "weiruanyahei", "wryh"] },
    { name: "幼圆", aliases: ["YouYuan"], keys: ["youyuan", "yy"] },
    { name: "楷体", keys: ["kaiti", "kt"] },
    { name: "宋体", keys: ["songti", "st"] },
    { name: "黑体", keys: ["heiti", "ht"] },
    { name: "华文宋体", keys: ["huawensongti", "hwst"] },
    { name: "Noto Sans SC", keys: ["notosanssc"] },
  ],
};

function readFonts() {
  return { face: $("font-face").value.trim() };
}

function setFontInputs(fonts) {
  if (!fonts || !fonts.face) return;
  if (document.activeElement !== $("font-face")) $("font-face").value = fonts.face;
}

function isNamed(item, value) {
  return item.name === value || (item.aliases || []).includes(value);
}

function faceMatches(item, needle) {
  const q = needle.trim().toLowerCase().replace(/\s+/g, "");
  if (!q) return true;
  const name = String(item.name || "").toLowerCase().replace(/\s+/g, "");
  if (name.includes(q)) return true;
  return (item.keys || []).some((key) => String(key).toLowerCase().replace(/\s+/g, "").includes(q));
}

const PRESET_THEMES = [
  { name: "Dark", value: "dark", keys: ["dark"] },
  { name: "Light", value: "light", keys: ["light"] },
  { name: "Midlight", value: "mix", keys: ["mix", "midlight"] },
];

const themeChoices = PRESET_THEMES.slice();

function isCustomThemeId(id) {
  return String(id || "").startsWith("自定义");
}

function customThemeItem(id) {
  const n = String(id || "").replace(/^自定义/, "");
  return { name: id, value: id, keys: [id, `zidingyi${n}`, `zd${n}`] };
}

function refreshThemeChoices(customs) {
  const extra = (customs || []).map((theme) => customThemeItem(theme.id || theme));
  themeChoices.splice(0, themeChoices.length, ...PRESET_THEMES, ...extra);
}

function themeName(id) {
  return themeChoices.find((item) => item.value === id)?.name || "Dark";
}

function alignOpacitySlider() {
  const last = $("swatch-emphasis");
  const slider = $("glass-opacity");
  if (!last || !slider) return;
  slider.style.width = "";
  const width = last.getBoundingClientRect().right - slider.getBoundingClientRect().left;
  if (width > 0) slider.style.width = `${width.toFixed(1)}px`;
}

function alignFlyoutPlus() {
  const add = $("btn-theme-add");
  const pop = add?.closest(".theme-action-pop");
  const preview = document.querySelector(".preview");
  if (!add) return;
  add.style.right = "";
  if (!pop?.classList.contains("has-save") || !preview) return;
  const parent = add.offsetParent;
  if (!parent) return;
  add.style.right = `${(parent.getBoundingClientRect().right - preview.getBoundingClientRect().right).toFixed(1)}px`;
}

function alignThemeExtras() {
  alignOpacitySlider();
  alignFlyoutPlus();
}

function paintThemeActions(themeId) {
  const save = $("btn-theme-save");
  const add = $("btn-theme-add");
  const pop = add?.closest(".theme-action-pop") || save?.closest(".theme-action-pop");
  const showSave = isCustomThemeId(themeId) && !holdPlusHome;
  if (save) {
    save.hidden = !showSave;
    save.setAttribute("aria-haspopup", showSave ? "true" : "false");
    if (!showSave) save.setAttribute("aria-expanded", "false");
  }
  if (pop) {
    pop.classList.toggle("has-save", showSave);
    pop.classList.toggle("is-locked", holdPlusHome);
  }
  if (add) {
    add.setAttribute("aria-haspopup", "false");
    add.setAttribute("aria-expanded", "false");
  }
  requestAnimationFrame(alignThemeExtras);
}

function setThemeInput(id, { force = false } = {}) {
  const input = $("theme-select");
  if (!input) return;
  if (!force && document.activeElement === input) return;
  input.value = themeName(id);
}

function setupCombo(input, menu, toggle, families, onCommit, { previewFace = false, fallback, itemAction } = {}) {
  let active = -1;
  let browsing = false;
  const list = menu.querySelector(".combo-list") || menu;
  if (!list.id) list.id = `${input.id}-options`;
  input.setAttribute("role", "combobox");
  input.setAttribute("aria-controls", list.id);
  input.setAttribute("aria-autocomplete", "list");
  const rows = () => [...list.children];
  const setExpanded = (open) => {
    input.setAttribute("aria-expanded", String(open));
    if (!open) input.removeAttribute("aria-activedescendant");
    if (toggle) toggle.setAttribute("aria-expanded", open ? "true" : "false");
  };
  const restore = () => (typeof fallback === "function" ? fallback() : "");
  const closeOthers = () => {
    document.querySelectorAll(".combo-menu").forEach((el) => {
      if (el === menu) return;
      el.hidden = true;
      const btn = el.closest(".combo")?.querySelector(".combo-toggle");
      if (btn) btn.setAttribute("aria-expanded", "false");
    });
  };
  const close = () => {
    menu.hidden = true;
    active = -1;
    browsing = false;
    setExpanded(false);
  };
  const paintActive = () => {
    rows().forEach((li, i) => {
      li.classList.toggle("active", i === active);
      li.setAttribute("aria-selected", String(i === active));
    });
    const cur = rows()[active];
    if (cur) input.setAttribute("aria-activedescendant", cur.id);
    if (cur) cur.scrollIntoView({ block: "nearest" });
  };
  const matchedList = (q) =>
    q ? families.filter((item) => faceMatches(item, q)) : families.slice();
  const pick = (item) => {
    input.value = item.name;
    close();
    onCommit(item);
  };
  const render = (q) => {
    placeComboMenu(input, menu);
    const matched = matchedList(q);
    list.innerHTML = "";
    matched.forEach((item, index) => {
      const li = document.createElement("li");
      li.id = `${list.id}-${index}`;
      li.setAttribute("role", "option");
      li.dataset.name = item.name;
      if (item.value) li.dataset.value = item.value;
      const name = document.createElement("span");
      name.className = "combo-name";
      name.textContent = item.name;
      if (previewFace) name.style.fontFamily = cssFace(item.name);
      li.appendChild(name);
      const action = typeof itemAction === "function" ? itemAction(item) : null;
      if (action) {
        const btn = document.createElement("button");
        btn.type = "button";
        btn.className = "combo-delete";
        btn.setAttribute("aria-label", action.label || "删除");
        btn.innerHTML =
          '<svg viewBox="0 0 24 24" width="10" height="10" aria-hidden="true"><path d="M18 6 6 18" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/><path d="m6 6 12 12" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/></svg>';
        const stop = (e) => {
          e.preventDefault();
          e.stopPropagation();
        };
        btn.addEventListener("mousedown", stop);
        btn.addEventListener("click", (e) => {
          stop(e);
          action.onClick(item);
        });
        li.appendChild(btn);
      }
      li.addEventListener("mousedown", (e) => {
        if (e.target.closest(".combo-delete")) return;
        e.preventDefault();
        pick(item);
      });
      list.appendChild(li);
    });
    if (matched.length === 0) {
      const li = document.createElement("li");
      li.textContent = "无匹配";
      li.className = "empty";
      list.appendChild(li);
    }
    menu.hidden = false;
    setExpanded(true);
    const current = input.value.trim();
    const idx = matched.findIndex((item) => item.name === current);
    active = idx >= 0 ? idx : matched.length ? 0 : -1;
    paintActive();
  };
  const openBrowse = () => {
    browsing = true;
    closeOthers();
    placeComboMenu(input, menu);
    render("");
    input.focus();
    input.select();
  };
  if (toggle) {
    toggle.addEventListener("mousedown", (e) => e.preventDefault());
    toggle.addEventListener("click", (e) => {
      e.preventDefault();
      if (!menu.hidden && browsing) {
        close();
        return;
      }
      openBrowse();
    });
  }
  input.addEventListener("focus", () => {
    if (menu.hidden) openBrowse();
  });
  input.addEventListener("pointerdown", () => {
    if (menu.hidden && document.activeElement === input) openBrowse();
  });
  input.addEventListener("input", () => {
    browsing = input.value.trim() === "";
    if (previewFace) {
      paintPreview(readPickers(), readFonts());
    }
    closeOthers();
    render(browsing ? "" : input.value);
  });
  input.addEventListener("keydown", (e) => {
    if (e.isComposing) return;
    if (e.key === "Escape") {
      e.preventDefault();
      input.value = restore();
      if (previewFace) paintPreview(readPickers(), readFonts());
      close();
      return;
    }
    if (menu.hidden) return;
    const items = rows();
    if (e.key === "ArrowDown") {
      e.preventDefault();
      active = Math.min(items.length - 1, active + 1);
      paintActive();
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      active = Math.max(0, active - 1);
      paintActive();
    } else if (e.key === "Enter" && active >= 0 && items[active] && !items[active].classList.contains("empty")) {
      e.preventDefault();
      const row = items[active];
      const item = families.find(
        (entry) =>
          (row.dataset.value && entry.value === row.dataset.value) ||
          entry.name === row.dataset.name
      );
      if (item) pick(item);
    }
  });
  input.addEventListener("blur", () => {
    window.setTimeout(() => {
      close();
      const next = input.value.trim();
      const item = families.find((entry) => isNamed(entry, next));
      if (!item) {
        input.value = restore();
        return;
      }
      if (!isNamed(item, restore())) onCommit(item);
    }, 120);
  });
  const onMove = () => {
    if (!menu.hidden) placeComboMenu(input, menu);
  };
  window.addEventListener("resize", onMove);
  document.addEventListener("scroll", onMove, true);
  menu.refreshCombo = () => {
    if (!menu.hidden) render(browsing ? "" : input.value);
  };
}

function withCurrent(list, current) {
  if (current && !list.some((item) => isNamed(item, current))) {
    return [{ name: current, aliases: [], keys: [] }, ...list];
  }
  return list;
}

const DARK_COLORS = {
  accent: "#8A8A8A",
  background: "#2A2A2A",
  foreground: "#F2F2F2",
  emphasis: "#FFFFFF",
};
const LIGHT_COLORS = {
  accent: "#4F84FF",
  background: "#FFFFFF",
  foreground: "#000000",
  emphasis: "#FFFFFF",
};
const MIX_COLORS = {
  accent: "#7794E4",
  background: "#3D57D6",
  foreground: "#FEE5CA",
  emphasis: "#FFFFFF",
};

function refreshOpenThemeMenu() {
  const menu = $("theme-menu");
  if (menu && !menu.hidden && typeof menu.refreshCombo === "function") menu.refreshCombo();
}

async function removeCustomTheme(item) {
  if (!isCustomThemeId(item.value)) return;
  if (!window.__TAURI__) {
    const idx = themeChoices.findIndex((entry) => entry.value === item.value);
    if (idx >= 0) themeChoices.splice(idx, 1);
    const current = lastStatus ? lastStatus.theme_id : $("theme-select").value;
    if (current === item.value || $("theme-select").value === item.name) {
      const colors = readPickers();
      if (lastStatus) {
        lastStatus.theme_id = "dark";
        lastStatus.colors = colors;
      }
      setThemeInput("dark", { force: true });
      paintThemeActions("dark");
      paintPreview(colors, readFonts(), "dark", readGlassOpacity());
    }
    refreshOpenThemeMenu();
    return;
  }
  const status = await call("delete_custom_theme", { themeId: item.value });
  if (!status) return;
  setThemeInput(status.theme_id, { force: true });
  refreshOpenThemeMenu();
}

function bindThemeCombo() {
  setupCombo(
    $("theme-select"),
    $("theme-menu"),
    $("theme-toggle"),
    themeChoices,
    (item) => {
      if (!window.__TAURI__) {
        if (customPanel) customPanel.hidden = false;
        if (glassPanel) glassPanel.hidden = false;
        closeColorCard();
        paintThemeActions(item.value);
        const colors =
          item.value === "mix" ? MIX_COLORS : item.value === "light" ? LIGHT_COLORS : DARK_COLORS;
        if (isCustomThemeId(item.value)) {
          paintPreview(readPickers(), readFonts(), item.value, readGlassOpacity());
        } else {
          setPickers(colors);
          paintPreview(colors, readFonts(), item.value, readGlassOpacity());
        }
        lastStatus = {
          ...(lastStatus || {}),
          theme_id: item.value,
          colors: isCustomThemeId(item.value) ? readPickers() : colors,
          fonts: readFonts(),
          glass_opacity: readGlassOpacity(),
        };
        return;
      }
      call("set_theme_id", { themeId: item.value });
    },
    {
      fallback: () => themeName(lastStatus ? lastStatus.theme_id : "dark"),
      itemAction: (item) =>
        isCustomThemeId(item.value)
          ? { label: `删除${item.name}`, onClick: removeCustomTheme }
          : null,
    }
  );
}

function bindFontPickers(lists) {
  const raw = lists.faces || [];
  const faces = withCurrent(
    raw.map((item) => (typeof item === "string" ? { name: item, keys: [] } : item)),
    lastStatus ? lastStatus.fonts.face : "Microsoft YaHei"
  );
  setupCombo(
    $("font-face"),
    $("font-face-menu"),
    $("font-face-toggle"),
    faces,
    (item) => {
      paintPreview(readPickers(), { face: item.name });
      if (!window.__TAURI__) return;
      call("set_fonts", { fonts: { face: item.name } });
    },
    {
      previewFace: true,
      fallback: () => (lastStatus ? lastStatus.fonts.face : "Microsoft YaHei"),
    }
  );
}

const MAX_LOGO_BYTES = 8 * 1024 * 1024;
let logoObjectUrl = "";
let logoSeq = 0;
let toolbarTimer = 0;
let toolbarCacheKey = "";

function toolbarKey(status, pct) {
  const colors = status && status.colors;
  return JSON.stringify({
    bg: colors && colors.background,
    fg: colors && colors.foreground,
    ac: colors && colors.accent,
    keepOfficial: (status && status.theme_id) === "light" && !glassEnabled(pct),
    logo: (status && status.logo_revision) || !!(status && status.has_custom_logo),
  });
}

function syncToolbarOpacity(pct) {
  const bg = $("toolbar-bg");
  if (bg) bg.style.opacity = String(clampGlassOpacity(pct) / 100);
}

function placeComboMenu(input, menu) {
  if (!input || !menu) return;
  const list = menu.querySelector(".combo-list") || menu;
  const rect = input.getBoundingClientRect();
  const gap = 4;
  const below = window.innerHeight - rect.bottom - gap - 8;
  const above = rect.top - gap - 8;
  const openUp = below < 140 && above > below;
  list.style.maxHeight = `${Math.min(220, Math.max(96, openUp ? above : below))}px`;
  menu.style.left = `${Math.round(rect.left)}px`;
  menu.style.width = `${Math.round(rect.width)}px`;
  menu.style.right = "auto";
  if (openUp) {
    menu.style.top = "auto";
    menu.style.bottom = `${Math.round(window.innerHeight - rect.top + gap)}px`;
  } else {
    menu.style.top = `${Math.round(rect.bottom + gap)}px`;
    menu.style.bottom = "auto";
  }
}

function revokeLogoUrl() {
  if (logoObjectUrl) {
    URL.revokeObjectURL(logoObjectUrl);
    logoObjectUrl = "";
  }
}

function svgUrl(svg) {
  return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;
}

function applyLogoBytes(bytes, hasCustom) {
  const bar = $("toolbar-logo");
  revokeLogoUrl();
  $("btn-clear-logo").disabled = false;
  if (!bytes || !bytes.length) {
    bar.removeAttribute("src");
    bar.hidden = true;
    return;
  }
  const u8 = bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes);
  logoObjectUrl = URL.createObjectURL(new Blob([u8], { type: "image/png" }));
  bar.src = logoObjectUrl;
  bar.hidden = false;
}

function hideToolbar() {
  $("preview-toolbar").hidden = true;
  toolbarCacheKey = "";
}

function showToolbar(preview, hasCustom) {
  $("preview-toolbar").hidden = false;
  $("toolbar-bg").src = svgUrl(preview.bg);
  $("toolbar-divider").src = svgUrl(preview.divider);
  $("toolbar-mic").src = svgUrl(preview.mic);
  $("toolbar-lang").src = svgUrl(preview.lang);
  $("toolbar-punct").src = svgUrl(preview.punct);
  $("toolbar-half").src = svgUrl(preview.half);
  applyLogoBytes(preview.logo, hasCustom);
}

function paintLogo(status) {
  const seq = ++logoSeq;
  const pct = status ? status.glass_opacity : readGlassOpacity();
  toolbarCacheKey = toolbarKey(status, pct);
  if (!window.__TAURI__) {
    hideToolbar();
    applyLogoBytes(null, false);
    return;
  }
  window.clearTimeout(toolbarTimer);
  toolbarTimer = window.setTimeout(() => {
    invoke("get_toolbar_preview", {
      colors: status ? status.colors : readPickers(),
      glassOpacity: pct,
    })
      .then((preview) => {
        if (seq !== logoSeq) return;
        showToolbar(preview, !!(status && status.has_custom_logo));
        syncToolbarOpacity(pct);
      })
      .catch(() => {
        if (seq !== logoSeq) return;
        hideToolbar();
        applyLogoBytes(null, !!(status && status.has_custom_logo));
      });
  }, 16);
}

$("btn-install").addEventListener("click", () => {
  if (window.Kaomoji) window.Kaomoji.show("great");
  call("install", { colors: readPickers() });
});
$("btn-uninstall").addEventListener("click", () => {
  const missing = !lastStatus || (!lastStatus.skin_applied && !lastStatus.recovery_needed);
  if (window.Kaomoji) window.Kaomoji.show(missing ? "bad" : "cancel");
  if (missing) return;
  call("uninstall");
});
$("btn-pick-logo").addEventListener("click", () => $("logo-file").click());
$("btn-clear-logo").addEventListener("click", () => {
  const official = !lastStatus || !lastStatus.has_custom_logo;
  if (window.Kaomoji) window.Kaomoji.show(official ? "bad" : "cancel");
  if (official) return;
  call("clear_logo");
});
$("logo-file").addEventListener("change", async () => {
  const file = $("logo-file").files[0];
  $("logo-file").value = "";
  if (!file) return;
  if (file.size > MAX_LOGO_BYTES) {
    showError("图片太大（上限 8MB，边长 4096 像素）");
    return;
  }
  try {
    // Pass the File-owned ArrayBuffer directly. Tauri recognizes a top-level
    // ArrayBuffer as a raw IPC body; wrapping it in a typed view has fallen
    // back to JSON on some WebView2/Tauri combinations.
    await call("import_logo", await file.arrayBuffer());
  } catch (err) {
    showError(`读取图片失败：${err}`);
  }
});

$("glass-opacity").addEventListener("input", () => {
  const pct = readGlassOpacity();
  $("glass-opacity-val").textContent = `${pct}%`;
  paintPreview(
    lastStatus ? lastStatus.colors : readPickers(),
    readFonts(),
    lastStatus ? lastStatus.theme_id : "dark",
    pct
  );
});
$("glass-opacity").addEventListener("change", () => {
  if (!window.__TAURI__) return;
  call("set_glass_opacity", { opacity: readGlassOpacity() });
});

function bindColorEditor() {
  if (colorHex) {
    colorHex.addEventListener("input", () => {
      if (colorSyncing) return;
      const next = parseHex(colorHex.value);
      if (next) applyColor(next, "hex", false);
    });
    colorHex.addEventListener("change", () => {
      if (colorSyncing) return;
      const next = parseHex(colorHex.value);
      if (next) applyColor(next, "hex", true);
    });
  }

  const onRgb = (persist) => {
    if (colorSyncing) return;
    const r = clampByte(colorR && colorR.value);
    const g = clampByte(colorG && colorG.value);
    const b = clampByte(colorB && colorB.value);
    if (r === null || g === null || b === null) return;
    applyColor(rgbToHex(r, g, b), "rgb", persist);
  };
  [colorR, colorG, colorB].forEach((input) => {
    if (!input) return;
    input.addEventListener("input", () => onRgb(false));
    input.addEventListener("change", () => onRgb(true));
  });

  const onHsl = (persist) => {
    if (colorSyncing) return;
    lastHue = Number(colorH && colorH.value) || 0;
    const rgb = hslToRgb(
      colorH && colorH.value,
      colorS && colorS.value,
      colorL && colorL.value
    );
    applyColor(rgbToHex(rgb.r, rgb.g, rgb.b), "hsl", persist);
  };
  [colorH, colorS, colorL].forEach((input) => {
    if (!input) return;
    input.addEventListener("input", () => onHsl(false));
    input.addEventListener("change", () => onHsl(true));
  });

  if (colorEyedrop) {
    if (!window.EyeDropper) {
      colorEyedrop.hidden = true;
    } else {
      colorEyedrop.addEventListener("click", async () => {
        colorEyedropping = true;
        try {
          const result = await new window.EyeDropper().open();
          if (result && result.sRGBHex) applyColor(result.sRGBHex, "drop", true);
        } catch (_) {
          /* cancelled */
        } finally {
          colorEyedropping = false;
        }
      });
    }
  }
}

document.querySelectorAll(".swatch").forEach((swatch) => {
  swatch.addEventListener("click", (event) => {
    event.stopPropagation();
    openColorCard(swatch.dataset.key, swatch);
  });
});

document.addEventListener("pointerdown", (event) => {
  if (colorEyedropping || !colorCard || colorCard.hidden) return;
  const target = event.target;
  if (target.closest(".color-card") || target.closest(".swatch")) return;
  closeColorCard();
});

document.addEventListener("keydown", (event) => {
  if (event.key === "Escape" && !colorEyedropping) closeColorCard();
});

window.addEventListener("resize", () => {
  alignThemeExtras();
  if (!colorCard || colorCard.hidden || !colorCardKey) return;
  placeColorCard($(`swatch-${colorCardKey}`));
});

bindColorEditor();

function bindAutohideScrollbar() {
  let hideTimer = 0;
  const reveal = () => {
    document.body.classList.add("is-scrolling");
    window.clearTimeout(hideTimer);
    hideTimer = window.setTimeout(() => {
      document.body.classList.remove("is-scrolling");
    }, 2000);
  };
  document.addEventListener("scroll", reveal, { capture: true, passive: true });
  document.body.addEventListener("wheel", reveal, { passive: true });
}

function bindPreviewStage() {
  const preview = document.querySelector(".preview");
  const btn = $("btn-preview-bg");
  const flash = $("preview-bg-flash");
  if (!preview || !btn) return;
  let flashTimer = 0;
  const showFlash = (light) => {
    if (!flash) return;
    flash.classList.remove("is-on", "is-sun", "is-moon");
    void flash.offsetWidth;
    flash.classList.add("is-on", light ? "is-sun" : "is-moon");
    window.clearTimeout(flashTimer);
    flashTimer = window.setTimeout(() => {
      flash.classList.remove("is-on", "is-sun", "is-moon");
    }, 1000);
  };
  const apply = (light, announce) => {
    preview.classList.toggle("is-light", light);
    btn.setAttribute("aria-pressed", light ? "true" : "false");
    btn.setAttribute("aria-label", light ? "切换为深色预览底" : "切换为浅色预览底");
    if (announce) showFlash(light);
    if (announce && window.Kaomoji) window.Kaomoji.show(light ? "wake" : "sleep");
  };
  apply(false, false);
  btn.addEventListener("click", () => apply(!preview.classList.contains("is-light"), true));
}

setWorkdirTip();
bindAutohideScrollbar();
function bindThemeActions() {
  const add = $("btn-theme-add");
  const save = $("btn-theme-save");
  const pop = add?.closest(".theme-action-pop");
  const syncExpanded = () => {
    const open = Boolean(
      !holdPlusHome &&
        save &&
        !save.hidden &&
        pop &&
        (pop.matches(":hover") || pop.contains(document.activeElement))
    );
    if (save) save.setAttribute("aria-expanded", open ? "true" : "false");
    if (add) add.setAttribute("aria-expanded", "false");
  };
  if (pop) {
    pop.addEventListener("pointerenter", syncExpanded);
    pop.addEventListener("pointerleave", () => window.requestAnimationFrame(syncExpanded));
    pop.addEventListener("focusin", syncExpanded);
    pop.addEventListener("focusout", () => window.requestAnimationFrame(syncExpanded));
  }
  let lockTimer = 0;
  const setBusy = (busy) => {
    if (add) add.disabled = busy;
    if (save) save.disabled = busy;
  };
  const unlock = () => {
    holdPlusHome = false;
    setBusy(false);
    if (add) add.classList.remove("is-check");
    if (save) save.classList.remove("is-saved");
    paintThemeActions(lastStatus ? lastStatus.theme_id : "dark");
  };
  const flash = (kind, ms = 1000) => {
    setBusy(true);
    if (add) add.classList.toggle("is-check", kind === "add");
    if (save) save.classList.toggle("is-saved", kind === "save");
    window.clearTimeout(lockTimer);
    lockTimer = window.setTimeout(unlock, ms);
  };
  if (add) {
    add.addEventListener("click", async () => {
      if (add.disabled) return;
      const keepPlus = Boolean(save?.hidden);
      holdPlusHome = keepPlus;
      setBusy(true);
      if (!window.__TAURI__) {
        const used = new Set(
          themeChoices
            .filter((item) => isCustomThemeId(item.value))
            .map((item) => Number(String(item.value).replace(/^自定义/, "")))
            .filter((n) => Number.isFinite(n) && n > 0)
        );
        let n = 1;
        while (used.has(n)) n += 1;
        const next = `自定义${n}`;
        themeChoices.push(customThemeItem(next));
        setThemeInput(next, { force: true });
        lastStatus = {
          ...(lastStatus || {}),
          theme_id: next,
          colors: readPickers(),
          fonts: readFonts(),
          glass_opacity: readGlassOpacity(),
        };
        paintThemeActions(next);
        flash("add", keepPlus ? 2000 : 1000);
        return;
      }
      const status = await call("add_custom_theme", { colors: readPickers() });
      if (status) flash("add", keepPlus ? 2000 : 1000);
      else unlock();
    });
  }
  if (save) {
    save.addEventListener("click", async () => {
      if (save.disabled || save.hidden) return;
      setBusy(true);
      if (!window.__TAURI__) {
        flash("save");
        return;
      }
      const status = await call("save_custom_theme", { colors: readPickers() });
      if (status) flash("save");
      else unlock();
    });
  }
}

function bindAppMenu() {
  const menu = $("app-menu");
  const versionEl = $("app-menu-version");
  const repo = $("app-menu-repo");
  if (!menu) return;
  const hide = () => {
    menu.hidden = true;
  };
  const closeCombos = () => {
    document.querySelectorAll(".combo-menu").forEach((el) => {
      el.hidden = true;
      const btn = el.closest(".combo")?.querySelector(".combo-toggle");
      if (btn) btn.setAttribute("aria-expanded", "false");
    });
  };
  const paintVersion = async () => {
    if (!versionEl) return;
    let version = lastStatus?.app_version || "开发预览";
    try {
      if (window.__TAURI__?.app?.getVersion) {
        version = await window.__TAURI__.app.getVersion();
      }
    } catch {
      /* keep fallback */
    }
    versionEl.textContent = `v${String(version).replace(/^v/i, "")}`;
  };
  const place = (x, y) => {
    menu.hidden = false;
    menu.style.left = `${x}px`;
    menu.style.top = `${y}px`;
    const rect = menu.getBoundingClientRect();
    const pad = 8;
    let left = x;
    let top = y;
    if (rect.right > window.innerWidth - pad) left = window.innerWidth - rect.width - pad;
    if (rect.bottom > window.innerHeight - pad) top = window.innerHeight - rect.height - pad;
    menu.style.left = `${Math.max(pad, left)}px`;
    menu.style.top = `${Math.max(pad, top)}px`;
  };
  const toolbarBtn = $("app-menu-toolbar");
  let toolbarReadSeq = 0;
  const paintToolbar = async (shown) => {
    if (!toolbarBtn) return;
    const seq = ++toolbarReadSeq;
    toolbarBtn.disabled = true;
    const label = toolbarBtn.querySelector(".app-menu-label");
    if (label) label.textContent = "读取工具栏状态…";
    try {
      const on = shown === undefined ? await invoke("ime_toolbar_visible") : shown;
      if (seq !== toolbarReadSeq) return;
      toolbarBtn.classList.toggle("is-on", Boolean(on));
      toolbarBtn.setAttribute("aria-checked", on ? "true" : "false");
      toolbarBtn.title = lastStatus?.review_mode ? "检查模式不更改输入法设置" : "";
      toolbarBtn.disabled = !!lastStatus?.review_mode;
      if (label) label.textContent = "展示工具栏";
    } catch (err) {
      if (seq !== toolbarReadSeq) return;
      toolbarBtn.title = String(err);
      if (label) label.textContent = "工具栏状态不可用";
    }
  };

  const show = (x, y) => {
    closeCombos();
    paintVersion();
    paintToolbar();
    if (repo) {
      repo.hidden = !PUBLIC_REPO_URL;
      if (PUBLIC_REPO_URL) repo.setAttribute("href", PUBLIC_REPO_URL);
    }
    place(x, y);
  };
  document.addEventListener("contextmenu", (event) => {
    if (event.target.closest("input, textarea, [contenteditable]")) return;
    event.preventDefault();
    show(event.clientX, event.clientY);
  });
  document.addEventListener("pointerdown", (event) => {
    if (menu.hidden || event.target.closest("#app-menu")) return;
    hide();
  });
  document.addEventListener("keydown", (event) => {
    if ((event.key === "ContextMenu" || (event.shiftKey && event.key === "F10")) &&
        !event.target.closest("input, textarea, [contenteditable]")) {
      event.preventDefault();
      const rect = event.target.getBoundingClientRect();
      show(Math.min(rect.left + 12, window.innerWidth - 180), Math.min(rect.bottom, window.innerHeight - 180));
      menu.querySelector("button:not(:disabled)")?.focus();
    }
    if (event.key === "Escape") hide();
  });
  window.addEventListener("resize", hide);
  document.addEventListener("scroll", hide, true);
  const bindInvoke = (id, command) => {
    const btn = $(id);
    if (!btn) return;
    btn.addEventListener("click", async () => {
      hide();
      if (!window.__TAURI__) return;
      try {
        await invoke(command);
      } catch (err) {
        showError(String(err));
      }
    });
  };
  bindInvoke("app-menu-settings", "open_ime_settings");
  bindInvoke("app-menu-workdir", "open_workdir");
  if (toolbarBtn) {
    toolbarBtn.addEventListener("click", async () => {
      if (!window.__TAURI__) return;
      if (toolbarBtn.disabled || lastStatus?.review_mode) return;
      toolbarBtn.disabled = true;
      try {
        const shown = await invoke("toggle_ime_toolbar");
        await paintToolbar(shown);
      } catch (err) {
        await paintToolbar();
        showError(String(err));
      }
    });
  }
  if (repo) {
    repo.addEventListener("click", async (event) => {
      event.preventDefault();
      hide();
      if (!window.__TAURI__) {
        window.open(PUBLIC_REPO_URL, "_blank", "noopener,noreferrer");
        return;
      }
      try {
        await invoke("open_public_repo");
      } catch (err) {
        showError(String(err));
      }
    });
  }
  paintVersion();
}

bindThemeCombo();
bindThemeActions();
bindPreviewStage();
bindAppMenu();
if (!window.__TAURI__) {
  imeState.className = "plugin-state off";
  imeStateText.textContent = "请用 Tauri 窗口打开";
  setImeTip(null, false);
  setPluginHint("off", "未安装");
  setThemeInput("dark");
  paintThemeActions("dark");
  bindFontPickers(MOCK_FONTS);
  setFontInputs({ face: "Microsoft YaHei" });
  setPickers(DARK_COLORS);
  paintPreview(DARK_COLORS, { face: "Microsoft YaHei" });
} else {
  Promise.all([call("get_status"), invoke("list_system_fonts")])
    .then(([, lists]) => bindFontPickers(lists))
    .catch((err) => showError(String(err)));
}
