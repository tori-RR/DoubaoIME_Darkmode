import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { runInNewContext } from "node:vm";

const source = readFileSync(new URL("../ui/app.js", import.meta.url), "utf8");
const binding = source.slice(
  source.indexOf('$("btn-pick-logo").addEventListener'),
  source.indexOf('$("glass-opacity").addEventListener("input"'),
);

function setupIcons(status = {}) {
  const calls = [];
  const errors = [];
  const reactions = [];
  const elements = new Map();
  const get = (id) => {
    if (!elements.has(id)) elements.set(id, {
      files: [], value: "", clicked: false,
      addEventListener(event, handler) { this[event] = handler; },
      click() { this.clicked = true; },
    });
    return elements.get(id);
  };
  const context = {
    $: get,
    MAX_LOGO_BYTES: 8 * 1024 * 1024,
    call: async (name, payload) => { calls.push([name, payload]); },
    showError: (message) => { errors.push(message); },
    lastStatus: status,
    inflight: null,
    window: { Kaomoji: { show: (reaction) => reactions.push(reaction) } },
  };
  runInNewContext(binding, context);
  return { calls, errors, reactions, get, context };
}

for (const [inputId, command] of [["logo-file", "import_logo"], ["taskbar-logo-file", "import_taskbar_logo"]]) {
  test(`${inputId} sends only its File ArrayBuffer to the matching draft command`, async () => {
    const expected = new Uint8Array([137, 80, 78, 71]).buffer;
    const result = setupIcons();
    const input = result.get(inputId);
    input.files = [{ size: expected.byteLength, arrayBuffer: async () => expected }];
    input.value = "selected.png";
    await input.change();
    assert.deepEqual(result.calls, [[command, expected]]);
    assert.equal(input.value, "");
    assert.deepEqual(result.errors, []);
  });

  test(`${inputId} rejects an oversized image before reading or invoking IPC`, async () => {
    let read = false;
    const result = setupIcons();
    result.get(inputId).files = [{
      size: 8 * 1024 * 1024 + 1,
      arrayBuffer: async () => { read = true; return new ArrayBuffer(0); },
    }];
    await result.get(inputId).change();
    assert.equal(read, false);
    assert.deepEqual(result.calls, []);
    assert.deepEqual(result.errors, ["图片太大（上限 8MB，边长 4096 像素）"]);
  });
}

test("each picker opens its own file input", () => {
  const view = setupIcons();
  view.get("btn-pick-taskbar-logo").click();
  assert.equal(view.get("taskbar-logo-file").clicked, true);
  assert.equal(view.get("logo-file").clicked, false);
  view.get("btn-pick-logo").click();
  assert.equal(view.get("logo-file").clicked, true);
});

test("restoring toolbar and taskbar checks only its own custom state", () => {
  const toolbarOnly = setupIcons({ has_custom_logo: true, has_custom_taskbar_logo: false });
  toolbarOnly.get("btn-clear-taskbar-logo").click();
  toolbarOnly.get("btn-clear-logo").click();
  assert.deepEqual(toolbarOnly.calls, [["clear_logo", undefined]]);
  assert.deepEqual(toolbarOnly.reactions, ["bad", "cancel"]);

  const taskbarOnly = setupIcons({ has_custom_logo: false, has_custom_taskbar_logo: true });
  taskbarOnly.get("btn-clear-logo").click();
  taskbarOnly.get("btn-clear-taskbar-logo").click();
  assert.deepEqual(taskbarOnly.calls, [["clear_taskbar_logo", undefined]]);
  assert.deepEqual(taskbarOnly.reactions, ["bad", "cancel"]);
});

test("taskbar upload and reset leave toolbar state and commands alone", async () => {
  const status = {
    has_custom_logo: true, logo_revision: "toolbar-original",
    has_custom_taskbar_logo: false, taskbar_logo_revision: "",
  };
  const view = setupIcons(status);
  const bytes = new Uint8Array([137, 80, 78, 71]).buffer;
  view.get("taskbar-logo-file").files = [{ size: bytes.byteLength, arrayBuffer: async () => bytes }];
  await view.get("taskbar-logo-file").change();
  // Simulate the new taskbar draft reported by the backend, then restore it.
  status.has_custom_taskbar_logo = true;
  status.taskbar_logo_revision = "new-taskbar";
  view.get("btn-clear-taskbar-logo").click();
  assert.deepEqual(view.calls.map(([name]) => name), ["import_taskbar_logo", "clear_taskbar_logo"]);
  assert.equal(status.has_custom_logo, true);
  assert.equal(status.logo_revision, "toolbar-original");
});

test("overlapping file reads queue both independent imports instead of dropping one", async () => {
  const view = setupIcons();
  let finishFirst;
  const firstBytes = new Uint8Array([1]).buffer;
  const secondBytes = new Uint8Array([2]).buffer;
  view.get("logo-file").files = [{ size: 1, arrayBuffer: () => new Promise((resolve) => { finishFirst = resolve; }) }];
  view.get("taskbar-logo-file").files = [{ size: 1, arrayBuffer: async () => secondBytes }];
  const first = view.get("logo-file").change();
  const second = view.get("taskbar-logo-file").change();
  await Promise.resolve();
  assert.deepEqual(view.calls, []);
  finishFirst(firstBytes);
  await Promise.all([first, second]);
  assert.deepEqual(view.calls, [["import_logo", firstBytes], ["import_taskbar_logo", secondBytes]]);
});

test("an import waits for an existing install instead of silently becoming that install", async () => {
  const view = setupIcons();
  let finishInstall;
  view.context.inflight = new Promise((resolve) => { finishInstall = resolve; });
  const bytes = new Uint8Array([1]).buffer;
  view.get("taskbar-logo-file").files = [{ size: 1, arrayBuffer: async () => bytes }];
  const upload = view.get("taskbar-logo-file").change();
  await Promise.resolve();
  await Promise.resolve();
  assert.deepEqual(view.calls, []);
  view.context.inflight = null;
  finishInstall();
  await upload;
  assert.deepEqual(view.calls, [["import_taskbar_logo", bytes]]);
});

test("taskbar explanation is exact and both reset controls are keyboard-accessible", () => {
  const markup = readFileSync(new URL("../ui/index.html", import.meta.url), "utf8");
  const styles = readFileSync(new URL("../ui/styles.css", import.meta.url), "utf8");
  assert.match(markup, /id="taskbar-icon-tip-text"[^>]*>修改任务栏图标需重启资源管理器<\/span>/);
  assert.match(markup, /id="taskbar-icon-tip"[^>]*tabindex="0"/);
  for (const id of ["btn-clear-logo", "btn-clear-taskbar-logo"]) {
    const button = markup.match(new RegExp(`<button id="${id}"[^>]*>`))?.[0];
    assert.ok(button);
    assert.doesNotMatch(button, /hidden|tabindex="-1"/);
    assert.match(button, /aria-label="恢复默认/);
  }
  assert.match(styles, /\.icon-action-pop:focus-within \.logo-reset\s*\{[^}]*pointer-events: auto/s);
});

const previewSource = source.slice(source.indexOf("let iconPreviewSeq ="), source.indexOf('$("btn-install").addEventListener'));

function setupPreviews() {
  const elements = new Map();
  const pending = [];
  const created = [];
  const revoked = [];
  const context = {
    window: { __TAURI__: {} }, Blob, Uint8Array,
    $: (id) => {
      if (!elements.has(id)) elements.set(id, { removeAttribute(name) { delete this[name]; } });
      return elements.get(id);
    },
    URL: {
      createObjectURL(blob) { created.push(blob); return `blob:${created.length}`; },
      revokeObjectURL(url) { revoked.push(url); },
    },
    invoke: (name) => {
      assert.equal(name, "get_icon_previews");
      return new Promise((resolve, reject) => pending.push({ resolve, reject }));
    },
  };
  runInNewContext(previewSource, context);
  return { context, pending, elements, created, revoked };
}

test("late icon previews cannot overwrite the newest independent pair", async () => {
  const view = setupPreviews();
  const first = view.context.paintIconPreviews({ logo_revision: "a" });
  const second = view.context.paintIconPreviews({ logo_revision: "b" });
  view.pending[1].resolve({ toolbar: [1], taskbar: [2] });
  await second;
  view.pending[0].resolve({ toolbar: [3], taskbar: [4] });
  await first;
  assert.equal(view.created.length, 2);
  assert.equal(view.elements.get("logo-picker-preview").src, "blob:1");
  assert.equal(view.elements.get("taskbar-logo-picker-preview").src, "blob:2");
  assert.deepEqual([...new Uint8Array(await view.created[0].arrayBuffer())], [1]);
  assert.deepEqual([...new Uint8Array(await view.created[1].arrayBuffer())], [2]);

  const third = view.context.paintIconPreviews({ logo_revision: "a" });
  view.pending[2].resolve({ toolbar: null, taskbar: [5] });
  await third;
  assert.equal(view.elements.get("logo-picker-preview").hidden, true);
  assert.equal(view.elements.get("taskbar-logo-picker-preview").src, "blob:3");
  assert.deepEqual(view.revoked, ["blob:1", "blob:2"]);
});

test("an obsolete preview failure cannot clear the new thumbnails", async () => {
  const view = setupPreviews();
  const first = view.context.paintIconPreviews({});
  const second = view.context.paintIconPreviews({});
  view.pending[1].resolve({ toolbar: [1], taskbar: [2] });
  await second;
  view.pending[0].reject(new Error("obsolete"));
  await first;
  assert.equal(view.elements.get("logo-picker-preview").hidden, false);
  assert.equal(view.elements.get("taskbar-logo-picker-preview").hidden, false);
  assert.deepEqual(view.revoked, []);
});
