(function () {
  const knobs = [
    { id: "ui-size", css: "--ui-size", label: "界面正文", min: 10, max: 22, step: 0.1, value: 14, unit: "px" },
    { id: "ui-weight", css: "--ui-weight", label: "界面字重", min: 100, max: 900, step: 10, value: 400, unit: "" },
    { id: "title-size", css: "--title-size", label: "标题", min: 12, max: 28, step: 0.1, value: 26, unit: "px" },
    { id: "title-weight", css: "--title-weight", label: "标题字重", min: 100, max: 900, step: 10, value: 700, unit: "" },
    { id: "lede-size", css: "--lede-size", label: "副标题", min: 10, max: 22, step: 0.1, value: 14, unit: "px" },
    { id: "lede-weight", css: "--lede-weight", label: "副标题字重", min: 100, max: 900, step: 10, value: 400, unit: "" },
    { id: "state-size", css: "--state-size", label: "状态栏", min: 9, max: 18, step: 0.1, value: 13.5, unit: "px" },
    { id: "state-weight", css: "--state-weight", label: "状态栏字重", min: 100, max: 900, step: 10, value: 400, unit: "" },
    { id: "hint-size", css: "--hint-size", label: "说明文字", min: 9, max: 18, step: 0.1, value: 12, unit: "px" },
    { id: "hint-weight", css: "--hint-weight", label: "说明字重", min: 100, max: 900, step: 10, value: 400, unit: "" },
    { id: "label-size", css: "--label-size", label: "表单标签", min: 9, max: 18, step: 0.1, value: 13, unit: "px" },
    { id: "label-weight", css: "--label-weight", label: "标签字重", min: 100, max: 900, step: 10, value: 400, unit: "" },
    { id: "btn-size", css: "--btn-size", label: "按钮", min: 10, max: 22, step: 0.1, value: 14.5, unit: "px" },
    { id: "btn-weight", css: "--btn-weight", label: "按钮字重", min: 100, max: 900, step: 10, value: 400, unit: "" },
    { id: "field-size", css: "--field-size", label: "输入框", min: 10, max: 22, step: 0.1, value: 14, unit: "px" },
    { id: "field-weight", css: "--field-weight", label: "输入框字重", min: 100, max: 900, step: 10, value: 400, unit: "" },
    { id: "chip-size", css: "--chip-size", label: "候选词", min: 10, max: 24, step: 0.01, value: 16.5, unit: "px" },
    { id: "chip-line", css: "--chip-line", label: "候选行高", min: 14, max: 28, step: 0.01, value: 21.25, unit: "px" },
    { id: "chip-weight", css: "--chip-weight", label: "候选字重", min: 100, max: 900, step: 10, value: 400, unit: "" },
    { id: "chip-index-size", css: "--chip-index-size", label: "候选序号", min: 8, max: 18, step: 0.01, value: 12.5, unit: "px" },
    { id: "chip-index-weight", css: "--chip-index-weight", label: "序号字重", min: 100, max: 900, step: 10, value: 400, unit: "" },
    { id: "toolbar-type-size", css: "--toolbar-type-size", label: "工具栏字", min: 8, max: 20, step: 0.1, value: 13.8, unit: "px" },
    { id: "toolbar-type-weight", css: "--toolbar-type-weight", label: "工具栏字重", min: 100, max: 900, step: 10, value: 400, unit: "" },
  ];

  const root = document.documentElement;
  const list = document.getElementById("tuner-list");
  const dump = document.getElementById("tuner-dump");
  if (!list) return;

  function format(knob, raw) {
    const n = Number(raw);
    const shown = knob.step < 1 ? n.toFixed(2).replace(/0+$/, "").replace(/\.$/, "") : String(Math.round(n));
    return knob.unit ? `${shown}${knob.unit}` : shown;
  }

  function apply(knob, raw) {
    root.style.setProperty(knob.css, format(knob, raw));
  }

  function snapshot() {
    return knobs
      .map((knob) => {
        const input = document.getElementById(knob.id);
        if (!input) return `${knob.css}: ${format(knob, knob.value)};`;
        return `${knob.css}: ${format(knob, input.value)};`;
      })
      .join("\n");
  }

  function paintDump() {
    if (dump) dump.textContent = snapshot();
  }

  knobs.forEach((knob) => {
    const row = document.createElement("label");
    row.className = "tuner-row";
    row.innerHTML = `<span>${knob.label}</span><input id="${knob.id}" type="range" min="${knob.min}" max="${knob.max}" step="${knob.step}" value="${knob.value}" /><output for="${knob.id}"></output>`;
    list.appendChild(row);
    const input = row.querySelector("input");
    const out = row.querySelector("output");
    const sync = () => {
      out.textContent = format(knob, input.value);
      apply(knob, input.value);
      paintDump();
    };
    input.addEventListener("input", sync);
    sync();
  });

  const copy = document.getElementById("tuner-copy");
  if (copy) {
    copy.addEventListener("click", async () => {
      const text = snapshot();
      try {
        await navigator.clipboard.writeText(text);
        copy.textContent = "已复制";
        window.setTimeout(() => {
          copy.textContent = "复制当前值";
        }, 1200);
      } catch {
        copy.textContent = "复制失败";
      }
    });
  }

  const reset = document.getElementById("tuner-reset");
  if (reset) {
    reset.addEventListener("click", () => {
      knobs.forEach((knob) => {
        const input = document.getElementById(knob.id);
        input.value = String(knob.value);
        input.dispatchEvent(new Event("input"));
      });
    });
  }
})();
