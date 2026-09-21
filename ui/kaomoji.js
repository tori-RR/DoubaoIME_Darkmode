(function () {
  const GROUPS = {
    casual: [],
    welcome: [],
    great: [],
    cancel: [],
    bad: [],
    sleep: [],
    wake: [],
  };
  const FILES = {
    casual: "Group_casual.txt",
    welcome: "Group_welcom.txt",
    great: "Group_great.txt",
    cancel: "Group_cancel.txt",
    bad: "Group_bad.txt",
    sleep: "Group_sleep.txt",
    wake: "Group_wake.txt",
  };

  const word = document.getElementById("kaomoji-word");
  if (!word) return;
  const gallery = document.getElementById("kaomoji-gallery");
  const galleryBody = document.getElementById("kaomoji-gallery-body");
  const refreshButton = document.getElementById("btn-refresh-kaomoji");

  const GROUP_LABELS = {
    casual: "日常",
    welcome: "欢迎",
    great: "成功",
    cancel: "取消",
    bad: "失败",
    sleep: "睡眠",
    wake: "唤醒",
  };

  const COOLDOWN_MS = 300;
  let lastRoll = 0;
  let ready = false;

  function parseFaces(text) {
    const out = [];
    const seen = new Set();
    for (const line of String(text || "").split(/\r?\n/)) {
      const face = line.replace(/^\uFEFF/, "");
      if (!face.trim() || seen.has(face)) continue;
      seen.add(face);
      out.push(face);
    }
    return out;
  }

  function showFace(face) {
    word.textContent = face;
    word.classList.remove("shake");
    void word.offsetWidth;
    word.classList.add("shake");
  }

  function pick(group, force) {
    const pool = (GROUPS[group] && GROUPS[group].length && GROUPS[group]) || GROUPS.casual;
    if (!pool.length) return;
    const now = Date.now();
    if (!force && now - lastRoll < COOLDOWN_MS) return;
    lastRoll = now;
    const current = word.textContent;
    const choices = pool.filter((face) => face !== current);
    const next = (choices.length ? choices : pool)[Math.floor(Math.random() * (choices.length || pool.length))];
    showFace(next);
  }

  function renderGallery() {
    if (!gallery || !galleryBody) return;
    galleryBody.replaceChildren();
    for (const [group, faces] of Object.entries(GROUPS)) {
      const section = document.createElement("section");
      section.className = "kaomoji-group";

      const heading = document.createElement("h3");
      heading.className = "kaomoji-group-title";
      heading.textContent = `${GROUP_LABELS[group] || group}（${faces.length}）`;
      section.append(heading);

      const list = document.createElement("div");
      list.className = "kaomoji-group-list";
      for (const face of faces) {
        const item = document.createElement("button");
        item.type = "button";
        item.className = "kaomoji-item";
        item.textContent = face;
        item.title = "点击替换预览；可选中文本调整格式";
        item.addEventListener("click", (event) => {
          event.stopPropagation();
          showFace(face);
        });
        list.append(item);
      }
      section.append(list);
      galleryBody.append(section);
    }
    gallery.hidden = false;
  }

  function isFontFace(el) {
    return !!(el && (el.id === "font-face" || (el.closest && el.closest("#font-face"))));
  }

  function isActionButton(el) {
    return !!(
      el &&
      el.closest &&
      el.closest("#btn-install, #btn-uninstall, #btn-clear-logo, #btn-preview-bg, #kaomoji-gallery")
    );
  }

  function inForm(el) {
    if (isFontFace(el)) return false;
    return !!(el && el.closest && el.closest("input, textarea, select, [contenteditable='true']"));
  }

  document.addEventListener("click", (e) => {
    if (!ready || inForm(e.target) || isActionButton(e.target)) return;
    pick("casual", false);
  });

  document.addEventListener("keydown", (e) => {
    if (!ready || e.repeat || e.ctrlKey || e.metaKey || e.altKey) return;
    if (inForm(e.target)) return;
    if (e.isComposing && !isFontFace(e.target)) return;
    if (e.key.length === 1 || e.key === "Enter" || e.key === "Backspace") pick("casual", false);
  });

  document.addEventListener("compositionend", (e) => {
    if (!ready || inForm(e.target) || !e.data) return;
    pick("casual", false);
  });

  const fontFace = document.getElementById("font-face");
  if (fontFace) {
    fontFace.addEventListener("input", () => {
      if (ready) pick("casual", false);
    });
  }

  window.Kaomoji = {
    show(group) {
      pick(group, true);
    },
  };

  async function loadGroups() {
    if (window.__TAURI__) {
      const data = await window.__TAURI__.core.invoke("get_kaomoji_groups");
      Object.assign(GROUPS, data || {});
      return;
    }
    await Promise.all(
      Object.entries(FILES).map(async ([key, file]) => {
        const res = await fetch(`../kaomoji/${file}`);
        GROUPS[key] = parseFaces(await res.text());
      })
    );
  }

  async function refreshGroups() {
    if (!ready || !refreshButton || refreshButton.disabled) return;
    refreshButton.disabled = true;
    refreshButton.classList.add("is-loading");
    try {
      await loadGroups();
      renderGallery();
    } finally {
      refreshButton.classList.remove("is-loading");
      refreshButton.disabled = false;
    }
  }

  if (refreshButton) refreshButton.addEventListener("click", (event) => {
    event.stopPropagation();
    void refreshGroups();
  });

  loadGroups()
    .catch(() => {})
    .then(() => {
      ready = true;
      renderGallery();
      pick("welcome", true);
    });
})();
