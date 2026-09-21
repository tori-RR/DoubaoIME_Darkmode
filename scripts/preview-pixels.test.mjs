import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import vm from "node:vm";

const source = readFileSync(new URL("../ui/preview-pixels.js", import.meta.url), "utf8");

function harness(images = [], dpr = 1.25) {
  const frames = [];
  const queries = [];
  const events = {};
  const window = {
    devicePixelRatio: dpr,
    document: { querySelectorAll: () => images },
    requestAnimationFrame: (fn) => frames.push(fn),
    addEventListener: (name, fn) => { events[name] = fn; },
    matchMedia: (query) => {
      const media = {
        query,
        addEventListener: (_, fn) => { media.listener = fn; },
        removeEventListener: (_, fn) => { if (media.listener === fn) media.listener = null; },
      };
      queries.push(media);
      return media;
    },
  };
  vm.runInNewContext(source, { window });
  return { window, frames, queries, events, flush: () => frames.splice(0).forEach((fn) => fn()) };
}

function image(left, top, centered = false) {
  return {
    hidden: false,
    style: {},
    rect: { left, top, width: 26, height: 26 },
    measurements: [],
    getBoundingClientRect() {
      assert.equal(this.style.transform, undefined, "do not resample the image using a transform");
      const width = parseFloat(this.style.width) || this.rect.width;
      const height = parseFloat(this.style.height) || this.rect.height;
      const result = {
        left: this.rect.left + (centered ? (this.rect.width - width) / 2 : 0) + (parseFloat(this.style.left) || 0),
        top: this.rect.top + (centered ? (this.rect.height - height) / 2 : 0) + (parseFloat(this.style.top) || 0),
        width,
        height,
      };
      this.measurements.push({ ...result, position: this.style.position });
      return result;
    },
  };
}

function nearly(actual, expected) {
  assert.ok(Math.abs(actual - expected) < 0.0000001, `${actual} should equal ${expected}`);
}

test("equal square previews snap to physical pixels at 100, 125, 150, 175 and 200 percent DPI", () => {
  const { geometry } = harness().window.PreviewPixels;
  for (const dpr of [1, 1.25, 1.5, 1.75, 2]) {
    let commonSize;
    for (const [left, top] of [[53.8, 169.8], [58, 393.6], [0.1, 0.9], [-3.4, -5.7]]) {
      const rect = { left, top, width: 26, height: 26 };
      const placed = geometry(rect, dpr);
      const pxLeft = (left + placed.x) * dpr;
      const pxTop = (top + placed.y) * dpr;
      nearly(pxLeft, Math.round(pxLeft));
      nearly(pxTop, Math.round(pxTop));
      nearly(placed.size * dpr, placed.deviceSize);
      commonSize ??= placed.deviceSize;
      assert.equal(placed.deviceSize, commonSize);
    }
  }
});

test("geometry defaults to DPR 1 and rejects invalid or nonsquare rectangles", () => {
  const { geometry } = harness().window.PreviewPixels;
  const rect = { left: 1, top: 2, width: 26, height: 26 };
  assert.equal(geometry(rect).size, 26);
  for (const dpr of [0, -1, NaN, Infinity, "1.25", null]) assert.equal(geometry(rect, dpr), null);
  for (const invalid of [null, {}, { ...rect, width: 0 }, { ...rect, height: -1 }, { ...rect, width: 25 }, { ...rect, left: NaN }, { ...rect, top: Infinity }]) {
    assert.equal(geometry(invalid), null);
  }
});

test("schedules coalesce and repeated passes reset real dimensions and offsets without drift", () => {
  const images = [image(53.8, 169.8), image(58, 393.6, true), image(178.2, 393.6, true)];
  const h = harness(images);
  h.window.PreviewPixels.schedule();
  h.window.PreviewPixels.schedule();
  assert.equal(h.frames.length, 1);
  h.flush();
  const first = images.map((entry) => ({ ...entry.style }));
  for (let i = 0; i < 4; i++) {
    h.window.PreviewPixels.schedule();
    h.flush();
    assert.deepEqual(images.map((entry) => ({ ...entry.style })), first);
  }
  for (const entry of images) {
    assert.equal(entry.style.width, "26.4px");
    assert.equal(entry.style.height, "26.4px");
    assert.equal(entry.style.flexBasis, "26.4px");
    assert.equal(entry.style.position, "relative");
    assert.equal(entry.style.transform, undefined);
    for (let i = 0; i < 10; i += 2) {
      assert.equal(entry.measurements[i].width, 26, "reset to nominal size before each pass");
      assert.equal(entry.measurements[i + 1].width, 26.4, "remeasure grid centering after sizing");
    }
    const final = entry.getBoundingClientRect();
    nearly(final.left * 1.25, Math.round(final.left * 1.25));
    nearly(final.top * 1.25, Math.round(final.top * 1.25));
  }
});

test("DOMRect float noise keeps nominal 26px sizes and never rounds half-pixels down", () => {
  for (const dpr of [1.25, 1.75]) {
    const images = [image(53.8, 169.8), image(58, 393.6, true), image(178.2, 393.6, true)];
    images[0].rect.width = images[0].rect.height = 25.99999923706055;
    images[1].rect.width = 26.000001907348633;
    images[1].rect.height = 26;
    images[2].rect.width = 26;
    images[2].rect.height = 25.999996185302734;
    const h = harness(images, dpr);
    h.window.PreviewPixels.schedule();
    h.flush();
    for (const entry of images) {
      assert.equal(entry.style.width, `${Math.round(26 * dpr) / dpr}px`);
      assert.equal(entry.style.width, entry.style.height);
      nearly(parseFloat(entry.style.width) * dpr, Math.round(26 * dpr));
    }
  }
});

test("hidden, zero-size and non-26px images lose stale layout corrections and are skipped", () => {
  const images = [image(1, 2), image(3, 4), image(5, 6)];
  const h = harness(images);
  h.window.PreviewPixels.schedule();
  h.flush();
  images[0].hidden = true;
  images[1].rect.width = 0;
  images[1].rect.height = 0;
  images[2].rect.width = 30;
  images[2].rect.height = 30;
  h.window.PreviewPixels.schedule();
  h.flush();
  for (const entry of images) {
    for (const property of ["width", "height", "flexBasis", "position", "left", "top"]) assert.equal(entry.style[property], "");
    assert.equal(entry.style.transform, undefined);
  }
});

test("resize reschedules and DPR changes replace the resolution listener", () => {
  const h = harness([image(53.8, 169.8)]);
  assert.equal(h.queries[0].query, "(resolution: 1.25dppx)");
  h.events.resize();
  assert.equal(h.frames.length, 1);
  h.flush();
  h.window.devicePixelRatio = 1.5;
  h.queries[0].listener();
  assert.equal(h.queries[0].listener, null);
  assert.equal(h.queries[1].query, "(resolution: 1.5dppx)");
  assert.equal(h.frames.length, 1);
  h.flush();
  assert.equal(h.window.document.querySelectorAll()[0].style.width, "26px");
});

test("safe without a browser, document or requestAnimationFrame", () => {
  const plain = {};
  vm.runInNewContext(source, plain);
  assert.doesNotThrow(() => plain.PreviewPixels.schedule());
  const window = {};
  vm.runInNewContext(source, { window });
  assert.doesNotThrow(() => window.PreviewPixels.schedule());
  const h = harness();
  delete h.window.document;
  h.window.PreviewPixels.schedule();
  assert.doesNotThrow(h.flush);
});
