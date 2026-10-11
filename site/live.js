// Live previews: the app's own animations, compiled to WebAssembly (web/ in the repo), drawn
// on a canvas the way the native renderers draw them: the character grid, then the shapes.
// Each preview simulates the same scene as its screenshot (same size, scale, theme and seed)
// and scales it to the canvas, so it picks up where the still image left off.

const LEVELS = 16;
const SEED = 42;
/// Seconds simulated before the first frame, as in the screenshots.
const WARM_UP = 20;
const FONT_FAMILY = 'Menlo, Consolas, "Cascadia Code", ui-monospace, monospace';
const TEXT_FAMILY = 'system-ui, -apple-system, "Segoe UI", sans-serif';

let wasm = null;
const encoder = new TextEncoder();
const decoder = new TextDecoder();

/** Load the module once; null if WebAssembly isn't available. */
export async function load(url = "glyphdrift.wasm") {
  if (wasm) return wasm;
  try {
    const response = fetch(url);
    const { instance } = WebAssembly.instantiateStreaming
      ? await WebAssembly.instantiateStreaming(response, {}).catch(async () =>
          WebAssembly.instantiate(await (await fetch(url)).arrayBuffer(), {}))
      : await WebAssembly.instantiate(await (await response).arrayBuffer(), {});
    wasm = instance.exports;
  } catch {
    wasm = null;
  }
  return wasm;
}

/** Call `f(ptr, len)` with `text` copied into the module's memory. */
function withString(text, f) {
  const bytes = encoder.encode(text);
  const ptr = wasm.gd_alloc(bytes.length);
  new Uint8Array(wasm.memory.buffer, ptr, bytes.length).set(bytes);
  try {
    return f(ptr, bytes.length);
  } finally {
    wasm.gd_free(ptr, bytes.length);
  }
}

function palette(theme, warm) {
  const out = wasm.gd_alloc(3 * (LEVELS + 1));
  withString(theme, (p, n) => wasm.gd_palette(p, n, warm, 0, out));
  const b = new Uint8Array(wasm.memory.buffer, out, 3 * (LEVELS + 1)).slice();
  wasm.gd_free(out, 3 * (LEVELS + 1));
  const colors = [];
  for (let i = 0; i < b.length; i += 3) colors.push(`rgb(${b[i]},${b[i + 1]},${b[i + 2]})`);
  return colors; // [background, level 0 .. LEVELS-1]
}

/** Local time and long date, like the app's clock. */
function clockText() {
  const now = new Date();
  const time = now.toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });
  const date = now.toLocaleDateString([], { weekday: "long", year: "numeric", month: "long", day: "numeric" });
  return `${time}\n${date}`;
}

/** One running animation, drawn into `canvas`. */
export class Live {
  /**
   * @param {HTMLCanvasElement} canvas
   * @param {{anim: string, theme: string, width: number, height: number, zoom?: number,
   *          objectZoom?: number, clock?: boolean, speed?: number}} o  `width` x `height`
   *   is the simulated scene in px; the canvas shows all of it at whatever size it is.
   *   `speed` is the app's Speed setting (1 = normal); the warm-up runs at normal speed,
   *   so the first frame still matches the screenshot. `seed` picks the starting point
   *   (the screenshots use 42). `keepClear` ({cx, cy, hw, hh}, px) is a box Topographic
   *   drift flows around, as the header does around the logo.
   */
  constructor(canvas, o) {
    this.canvas = canvas;
    this.ctx = canvas.getContext("2d");
    this.w = o.width;
    this.h = o.height;
    const zoom = o.zoom ?? 1;
    // The grid font follows the overall scale, as in the app (16 px at 100%).
    this.fontSize = Math.round(16 * zoom);
    this.ctx.font = `${this.fontSize}px ${FONT_FAMILY}`;
    const m = this.ctx.measureText("M");
    this.cellW = m.width;
    this.ascent = m.fontBoundingBoxAscent ?? this.fontSize * 0.8;
    this.cellH = Math.ceil(this.ascent + (m.fontBoundingBoxDescent ?? this.fontSize * 0.25));
    this.cols = Math.max(1, Math.floor(this.w / this.cellW));
    this.rows = Math.max(1, Math.floor(this.h / this.cellH));
    this.ptr = withString(o.anim, (p, n) =>
      wasm.gd_new(p, n, this.cols, this.rows, this.w, this.h, zoom, o.objectZoom ?? 1, o.seed ?? SEED));
    if (o.keepClear) {
      const k = o.keepClear;
      wasm.gd_set_keep_clear(this.ptr, k.cx, k.cy, k.hw, k.hh);
    }
    this.fps = Math.min(wasm.gd_fps(this.ptr), 30);
    this.colors = palette(o.theme, wasm.gd_warm(this.ptr));
    this.clock = o.clock ?? false;
    this.speed = o.speed ?? 1;
    for (let t = 0; t < WARM_UP * 30; t++) wasm.gd_update(this.ptr, 1 / 30);
    this.last = 0;
  }

  /** Redraw the current frame in another theme (config name). */
  setTheme(theme) {
    this.colors = palette(theme, wasm.gd_warm(this.ptr));
    this.draw();
  }

  /** Match the canvas's backing store to its displayed size. */
  resize() {
    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    const r = this.canvas.getBoundingClientRect();
    const w = Math.max(1, Math.round(r.width * dpr));
    const h = Math.max(1, Math.round(r.height * dpr));
    if (this.canvas.width !== w || this.canvas.height !== h) {
      this.canvas.width = w;
      this.canvas.height = h;
    }
  }

  /** Advance to `now` (ms) if a frame is due at the animation's own rate, and draw. */
  tick(now) {
    if (now - this.last >= 1000 / this.fps - 2) this.step(now);
  }

  /** Advance to `now` (ms) and draw. */
  step(now) {
    const dt = this.last ? Math.min((now - this.last) / 1000, 0.25) : 0;
    this.last = now;
    wasm.gd_update(this.ptr, dt * this.speed);
    this.draw();
  }

  draw() {
    if (this.clock) withString(clockText(), (p, n) => wasm.gd_set_clock(this.ptr, p, n));
    wasm.gd_frame(this.ptr);
    this.resize();
    const ctx = this.ctx;
    const c = this.colors;
    const s = this.canvas.width / this.w;
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.fillStyle = c[0];
    ctx.fillRect(0, 0, this.canvas.width, this.canvas.height);
    ctx.setTransform(s, 0, 0, s, 0, 0);
    const level = (b) => c[1 + Math.min(LEVELS - 1, (b * LEVELS) >> 8)];

    // Grid: runs of same-colored characters per row, one fillText each.
    const cells = new Uint32Array(wasm.memory.buffer, wasm.gd_cells(this.ptr), this.cols * this.rows);
    ctx.font = `${this.fontSize}px ${FONT_FAMILY}`;
    ctx.textBaseline = "alphabetic";
    ctx.textAlign = "left";
    for (let y = 0; y < this.rows; y++) {
      let run = "", runStart = 0, runColor = null;
      const flush = () => {
        if (run) {
          ctx.fillStyle = runColor;
          ctx.fillText(run, runStart * this.cellW, y * this.cellH + this.ascent);
        }
        run = "";
      };
      for (let x = 0; x < this.cols; x++) {
        const v = cells[y * this.cols + x];
        const ch = v & 0x1fffff, b = v >>> 21;
        if (b === 0 || ch === 32) { flush(); continue; }
        const color = level(b);
        if (color !== runColor || !run) { flush(); runStart = x; runColor = color; }
        run += String.fromCodePoint(ch);
      }
      flush();
    }

    // Shapes, in order; consecutive lines and curves of one style share a path.
    const ops = new Float32Array(wasm.memory.buffer, wasm.gd_ops(this.ptr), wasm.gd_ops_len(this.ptr));
    const text = new Uint8Array(wasm.memory.buffer, wasm.gd_text(this.ptr));
    let i = 0, pathKey = null;
    const stroke = () => { if (pathKey !== null) ctx.stroke(); pathKey = null; };
    while (i < ops.length) {
      const kind = ops[i], b = ops[i + 1];
      if (kind === 0) {
        const [ax, ay, bx, by, w] = ops.subarray(i + 2, i + 7);
        i += 7;
        if (!b) continue;
        const key = `l${w}${level(b)}`;
        if (key !== pathKey) { stroke(); ctx.beginPath(); ctx.strokeStyle = level(b); ctx.lineWidth = w; pathKey = key; }
        ctx.moveTo(ax, ay);
        ctx.lineTo(bx, by);
      } else if (kind === 3) {
        const w = ops[i + 2], closed = ops[i + 3], n = ops[i + 4];
        const p = ops.subarray(i + 5, i + 7 + n * 6);
        i += 7 + n * 6;
        if (!b) continue;
        const key = `c${w}${level(b)}`;
        if (key !== pathKey) { stroke(); ctx.beginPath(); ctx.strokeStyle = level(b); ctx.lineWidth = w; ctx.lineJoin = "round"; pathKey = key; }
        ctx.moveTo(p[0], p[1]);
        for (let k = 2; k < p.length; k += 6) ctx.bezierCurveTo(p[k], p[k + 1], p[k + 2], p[k + 3], p[k + 4], p[k + 5]);
        if (closed) ctx.closePath();
      } else {
        stroke();
        if (kind === 1) {
          const [cx, cy, r, fill] = ops.subarray(i + 2, i + 6);
          i += 6;
          if (!b) continue;
          ctx.beginPath();
          ctx.arc(cx, cy, r, 0, Math.PI * 2);
          if (fill) { ctx.fillStyle = level(b); ctx.fill(); } else { ctx.strokeStyle = level(b); ctx.lineWidth = 1; ctx.stroke(); }
        } else if (kind === 2) {
          const n = ops[i + 2];
          const p = ops.subarray(i + 3, i + 3 + n * 2);
          i += 3 + n * 2;
          if (!b || n < 3) continue;
          ctx.beginPath();
          ctx.moveTo(p[0], p[1]);
          for (let k = 2; k < p.length; k += 2) ctx.lineTo(p[k], p[k + 1]);
          ctx.closePath();
          ctx.fillStyle = level(b);
          ctx.fill();
        } else if (kind === 4) {
          const [cx, cy, size, weight, off, len] = ops.subarray(i + 2, i + 8);
          i += 8;
          if (!b) continue;
          ctx.font = `${weight < 400 ? 300 : 400} ${size}px ${TEXT_FAMILY}`;
          ctx.textAlign = "center";
          ctx.textBaseline = "middle";
          ctx.fillStyle = level(b);
          ctx.fillText(decoder.decode(text.subarray(off, off + len)), cx, cy);
        } else {
          break; // unknown record
        }
      }
    }
    stroke();
  }

  dispose() {
    if (this.ptr) wasm.gd_drop(this.ptr);
    this.ptr = 0;
  }
}

/** Runs the started previews from one animation frame loop, each at its own frame rate. */
export class Player {
  constructor() {
    this.running = new Set();
    this.loop = (now) => {
      for (const live of this.running) live.tick(now);
      this.frame = this.running.size ? requestAnimationFrame(this.loop) : 0;
    };
  }
  start(live) {
    this.running.add(live);
    if (!this.frame) this.frame = requestAnimationFrame(this.loop);
  }
  stop(live) {
    this.running.delete(live);
    live.last = 0;
  }
}
