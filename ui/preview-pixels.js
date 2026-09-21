(function (root) {
  const selectors = "#toolbar-logo, #logo-picker-preview, #taskbar-logo-picker-preview";
  const adjusted = new WeakSet();
  let pending = false;
  let resolutionQuery = null;

  // Keep all three 26px previews on the same physical-pixel grid at fractional DPI.
  function geometry(rect, dpr = 1) {
    if (!rect || ![rect.left, rect.top, rect.width, rect.height, dpr].every(Number.isFinite)
      || rect.width <= 0 || rect.height <= 0 || dpr <= 0
      || Math.abs(rect.width - rect.height) > 0.000001) return null;
    const deviceSize = Math.round(rect.width * dpr);
    if (!Number.isFinite(deviceSize) || deviceSize < 1) return null;
    return {
      x: Math.round(rect.left * dpr) / dpr - rect.left,
      y: Math.round(rect.top * dpr) / dpr - rect.top,
      size: deviceSize / dpr,
      deviceSize,
    };
  }

  function paint() {
    pending = false;
    const doc = root.document;
    if (!doc || typeof doc.querySelectorAll !== "function") return;
    const images = [...doc.querySelectorAll(selectors)];
    // Size in layout rather than scaling a rasterized layer, which blurs edges.
    // Restore the nominal CSS geometry first so repeated renders never drift.
    for (const image of images) {
      if (adjusted.has(image)) {
        for (const property of ["width", "height", "flexBasis", "position", "left", "top"]) image.style[property] = "";
        adjusted.delete(image);
      }
    }
    const placements = images.map((image) => {
      if (image.hidden || typeof image.getBoundingClientRect !== "function") return null;
      const rect = image.getBoundingClientRect();
      if (Math.abs(rect.width - 26) > 0.0001 || Math.abs(rect.height - 26) > 0.0001) return null;
      // DOMRect float precision must not turn a nominal 32.5px into 32.499999px.
      return geometry({ left: rect.left, top: rect.top, width: 26, height: 26 }, root.devicePixelRatio === undefined ? 1 : root.devicePixelRatio);
    });
    images.forEach((image, index) => {
      const placement = placements[index];
      if (!placement) return;
      image.style.width = image.style.height = image.style.flexBasis = `${placement.size}px`;
      image.style.position = "relative";
      adjusted.add(image);
    });
    // Grid centering changes after sizing; measure again before snapping origins.
    const dpr = root.devicePixelRatio || 1;
    const origins = images.map((image) => adjusted.has(image) ? image.getBoundingClientRect() : null);
    images.forEach((image, index) => {
      const rect = origins[index];
      if (!rect) return;
      image.style.left = `${Math.round(rect.left * dpr) / dpr - rect.left}px`;
      image.style.top = `${Math.round(rect.top * dpr) / dpr - rect.top}px`;
    });
  }

  function schedule() {
    if (pending || typeof root.requestAnimationFrame !== "function") return;
    pending = true;
    root.requestAnimationFrame(paint);
  }

  function watchResolution() {
    if (resolutionQuery) {
      if (typeof resolutionQuery.removeEventListener === "function") resolutionQuery.removeEventListener("change", resolutionChanged);
      else if (typeof resolutionQuery.removeListener === "function") resolutionQuery.removeListener(resolutionChanged);
    }
    if (typeof root.matchMedia !== "function") return;
    const dpr = Number.isFinite(root.devicePixelRatio) && root.devicePixelRatio > 0 ? root.devicePixelRatio : 1;
    resolutionQuery = root.matchMedia(`(resolution: ${dpr}dppx)`);
    if (typeof resolutionQuery.addEventListener === "function") resolutionQuery.addEventListener("change", resolutionChanged);
    else if (typeof resolutionQuery.addListener === "function") resolutionQuery.addListener(resolutionChanged);
  }

  function resolutionChanged() {
    watchResolution();
    schedule();
  }

  root.PreviewPixels = { geometry, schedule };
  if (typeof root.addEventListener === "function") root.addEventListener("resize", schedule);
  watchResolution();
})(typeof window === "undefined" ? globalThis : window);
