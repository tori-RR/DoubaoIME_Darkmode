import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { runInNewContext } from "node:vm";

const source = readFileSync(new URL("../ui/app.js", import.meta.url), "utf8");
const binding = source.slice(
  source.indexOf('$("logo-file").addEventListener("change"'),
  source.indexOf('$("glass-opacity").addEventListener("input"'),
);

async function selectLogo(file) {
  const calls = [];
  const errors = [];
  const input = {
    files: file ? [file] : [],
    value: "selected.png",
    addEventListener(event, handler) { this[event] = handler; },
  };
  const context = {
    $: (id) => {
      assert.equal(id, "logo-file");
      return input;
    },
    MAX_LOGO_BYTES: 8 * 1024 * 1024,
    call: async (name, payload) => { calls.push([name, payload]); },
    showError: (message) => { errors.push(message); },
  };
  runInNewContext(binding, context);
  await input.change();
  return { calls, errors, input };
}

test("logo picker sends the File ArrayBuffer as the top-level IPC payload", async () => {
  const expected = new Uint8Array([137, 80, 78, 71]).buffer;
  const result = await selectLogo({ size: expected.byteLength, arrayBuffer: async () => expected });
  assert.equal(result.calls.length, 1);
  assert.equal(result.calls[0][0], "import_logo");
  assert.equal(result.calls[0][1], expected);
  assert.equal(result.input.value, "");
  assert.deepEqual(result.errors, []);
});

test("oversized logo is rejected before reading or invoking IPC", async () => {
  let read = false;
  const result = await selectLogo({
    size: 8 * 1024 * 1024 + 1,
    arrayBuffer: async () => { read = true; return new ArrayBuffer(0); },
  });
  assert.equal(read, false);
  assert.deepEqual(result.calls, []);
  assert.deepEqual(result.errors, ["图片太大（上限 8MB，边长 4096 像素）"]);
});
