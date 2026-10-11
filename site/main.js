// Glyphdrift landing page: a live port of the app's Topographic drift (contour lines of
// drifting value noise, warped around the clock box), the clock itself, and the theme picker.

// [label, background, [dim, mid, bright]] — the cool gradients from src/theme.rs.
const THEMES = [
  ["Matrix", "#000000", ["#005522", "#00aa44", "#00ff66"]],
  ["Mono", "#000000", ["#3c3c3c", "#9a9a9a", "#ffffff"]],
  ["Amber CRT", "#0a0600", ["#5a3200", "#c77800", "#ffb000"]],
  ["Catppuccin Mocha", "#1e1e2e", ["#89b4fa", "#74c7ec", "#89dceb"]],
  ["Tokyo Night", "#1a1b26", ["#7aa2f7", "#7dcfff", "#c0caf5"]],
  ["Nord", "#2e3440", ["#5e81ac", "#81a1c1", "#88c0d0"]],
  ["Gruvbox Dark", "#282828", ["#458588", "#83a598", "#8ec07c"]],
  ["Dracula", "#282a36", ["#6272a4", "#bd93f9", "#8be9fd"]],
  ["Rosé Pine", "#191724", ["#31748f", "#9ccfd8", "#e0def4"]],
  ["Kanagawa", "#1f1f28", ["#7e9cd8", "#7fb4ca", "#dcd7ba"]],
  ["One Dark", "#282c34", ["#c678dd", "#61afef", "#56b6c2"]],
  ["Solarized Dark", "#002b36", ["#268bd2", "#2aa198", "#93a1a1"]],
  ["Everforest", "#2d353b", ["#7fbbb3", "#83c092", "#d3c6aa"]],
  ["Monokai", "#272822", ["#ae81ff", "#66d9ef", "#a6e22e"]],
  ["Ayu Dark", "#0b0e14", ["#39bae6", "#95e6cb", "#bfbdb6"]],
  ["Night Owl", "#011627", ["#82aaff", "#7fdbca", "#d6deeb"]],
  ["Palenight", "#292d3e", ["#c792ea", "#82aaff", "#89ddff"]],
  ["GitHub Dark", "#0d1117", ["#58a6ff", "#79c0ff", "#c9d1d9"]],
  ["Oxocarbon", "#161616", ["#78a9ff", "#33b1ff", "#3ddbd9"]],
  ["Poimandres", "#1b1e28", ["#5de4c7", "#89ddff", "#add7ff"]],
  ["Synthwave '84", "#262335", ["#b893ce", "#ff7edb", "#36f9f6"]],
  ["Claude", "#262624", ["#cc785c", "#d97757", "#f0efea"]],
];
const DEFAULT_THEME = "Mono";

const rgb = (hex) => [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16));
const mix = (a, b, t) => a.map((v, i) => Math.round(v + (b[i] - v) * t));
const css = (c) => `rgb(${c[0]},${c[1]},${c[2]})`;

/** Color at brightness t (0..1): background, then through the three stops, as the app does. */
function shade(theme, t) {
  const pts = [rgb(theme[1]), ...theme[2].map(rgb)];
  const x = Math.min(Math.max(t, 0), 1) * 3;
  const i = Math.min(Math.floor(x), 2);
  return mix(pts[i], pts[i + 1], x - i);
}

// ---------------------------------------------------------------------------------------
// Value noise (same construction as src/anim/contour.rs, with a 32-bit hash)

function corner(x, y, z) {
  let h = Math.imul(x, 0x27d4eb2d) ^ Math.imul(y, 0x165667b1) ^ Math.imul(z, 0x9e3779b1);
  h = Math.imul(h ^ (h >>> 15), 0x85ebca6b);
  h = Math.imul(h ^ (h >>> 13), 0xc2b2ae35);
  h ^= h >>> 16;
  return (h & 0xffff) / 32767.5 - 1;
}

function noise3(x, y, z) {
  const xf = Math.floor(x), yf = Math.floor(y), zf = Math.floor(z);
  const s = (t) => t * t * (3 - 2 * t);
  const u = s(x - xf), v = s(y - yf), w = s(z - zf);
  const lerp = (a, b, t) => a + (b - a) * t;
  const plane = (dz) =>
    lerp(
      lerp(corner(xf, yf, zf + dz), corner(xf + 1, yf, zf + dz), u),
      lerp(corner(xf, yf + 1, zf + dz), corner(xf + 1, yf + 1, zf + dz), u),
      v
    );
  return lerp(plane(0), plane(1), w);
}

function fbm3(x, y, z) {
  return (
    (noise3(x, y, z) + 0.5 * noise3(2 * x + 17.3, 2 * y, z * 1.3) + 0.25 * noise3(4 * x, 4 * y + 31.7, z * 1.7)) /
    1.75
  );
}

// ---------------------------------------------------------------------------------------
// Terrain

const canvas = document.getElementById("terrain");
const ctx = canvas.getContext("2d");
const clockBox = document.querySelector(".clock");
const reduceMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;

const LEVELS = 16; // contour interval: 1/LEVELS of the height range per side
const CELL = 12; // CSS px between field samples
let theme = THEMES.find((t) => t[0] === DEFAULT_THEME);
let W = 0, H = 0, unit = 1, box = { cx: 0, cy: 0, hw: 1, hh: 1 };
let t0 = performance.now() / 1000 - 400 - Math.random() * 400;

function resize() {
  const dpr = Math.min(window.devicePixelRatio || 1, 2);
  W = canvas.clientWidth;
  H = canvas.clientHeight;
  canvas.width = Math.round(W * dpr);
  canvas.height = Math.round(H * dpr);
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  unit = Math.max(Math.min(W, H), 320);
  const c = canvas.getBoundingClientRect(), b = clockBox.getBoundingClientRect();
  box = { cx: b.left - c.left + b.width / 2, cy: b.top - c.top + b.height / 2, hw: b.width / 2, hh: b.height / 2 };
}

/** Where terrain point (x, y) samples from: pulled toward the box center near the box
 *  (fully at its rounded edge, fading with distance); null inside it. */
function warp(x, y) {
  const qx = x - box.cx, qy = y - box.cy;
  const ux = qx / box.hw, uy = qy / box.hh;
  const rho = Math.sqrt(Math.sqrt(ux ** 4 + uy ** 4));
  if (rho <= 1) return null;
  const k = (rho - 1 / (rho * rho)) / rho;
  return [box.cx + qx * k, box.cy + qy * k];
}

// Marching-squares segments per case (edges: 0 top, 1 right, 2 bottom, 3 left).
// Corner bits: top-left 8, top-right 4, bottom-right 2, bottom-left 1. Saddles (5, 10) are
// resolved by the cell center below.
const CASES = [[], [[3, 2]], [[2, 1]], [[3, 1]], [[0, 1]], null, [[0, 2]], [[3, 0]],
  [[3, 0]], [[0, 2]], null, [[0, 1]], [[3, 1]], [[2, 1]], [[3, 2]], []];

function draw(now) {
  const t = now / 1000 - t0;
  const scale = 1 / (unit * 0.45), drift = t * 0.05, morph = t * 0.12;
  const height = (x, y) => fbm3(x * scale + drift, y * scale, morph);
  const nx = Math.ceil(W / CELL) + 1, ny = Math.ceil(H / CELL) + 1;
  const field = new Float32Array(nx * ny);
  const inside = height(box.cx, box.cy); // flat inside the box: no lines cross it
  for (let j = 0; j < ny; j++) {
    for (let i = 0; i < nx; i++) {
      const p = warp(i * CELL, j * CELL);
      field[j * nx + i] = p ? height(p[0], p[1]) : inside;
    }
  }

  ctx.fillStyle = theme[1];
  ctx.fillRect(0, 0, W, H);
  ctx.lineCap = "round";
  const hair = Math.max(unit / 1440, 0.75), heavy = hair * 2.2;
  for (let k = -LEVELS; k <= LEVELS; k++) {
    const level = (k / LEVELS) * 0.8;
    const index = k % 5 === 0; // heavier index contour, as on a survey map
    const h = (level / 0.8 + 1) / 2; // 0 low .. 1 high
    ctx.strokeStyle = css(shade(theme, (40 + 150 * h + (index ? 40 : 0)) / 255));
    ctx.lineWidth = index ? heavy : hair;
    ctx.beginPath();
    for (let j = 0; j < ny - 1; j++) {
      for (let i = 0; i < nx - 1; i++) {
        const tl = field[j * nx + i], tr = field[j * nx + i + 1];
        const br = field[(j + 1) * nx + i + 1], bl = field[(j + 1) * nx + i];
        const c = (tl > level) * 8 + (tr > level) * 4 + (br > level) * 2 + (bl > level);
        if (c === 0 || c === 15) continue;
        let segs = CASES[c];
        if (!segs) {
          const high = (tl + tr + br + bl) / 4 > level;
          segs = c === 5 ? (high ? [[3, 0], [2, 1]] : [[3, 2], [0, 1]])
            : (high ? [[0, 1], [3, 2]] : [[3, 0], [2, 1]]);
        }
        const x = i * CELL, y = j * CELL;
        const cross = (a, b) => Math.min(Math.max((level - a) / (b - a), 0), 1);
        const edge = (e) =>
          e === 0 ? [x + CELL * cross(tl, tr), y]
            : e === 1 ? [x + CELL, y + CELL * cross(tr, br)]
              : e === 2 ? [x + CELL * cross(bl, br), y + CELL]
                : [x, y + CELL * cross(tl, bl)];
        for (const [ea, eb] of segs) {
          const a = edge(ea), b = edge(eb);
          ctx.moveTo(a[0], a[1]);
          ctx.lineTo(b[0], b[1]);
        }
      }
    }
    ctx.stroke();
  }
}

// ~30 fps while the hero is on screen; one still frame for reduced motion.
let visible = true, last = 0;
function frame(now) {
  if (visible && now - last >= 33) {
    last = now;
    draw(now);
  }
  if (!reduceMotion) requestAnimationFrame(frame);
}
new IntersectionObserver(([e]) => (visible = e.isIntersecting)).observe(canvas);
window.addEventListener("resize", () => { resize(); draw(performance.now()); });

// ---------------------------------------------------------------------------------------
// Clock

const timeEl = document.getElementById("time"), dateEl = document.getElementById("date");
function tick() {
  const now = new Date();
  timeEl.textContent = now.toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });
  dateEl.textContent = now.toLocaleDateString([], { weekday: "long", year: "numeric", month: "long", day: "numeric" });
}
tick();
setInterval(tick, 1000);

// ---------------------------------------------------------------------------------------
// Themes

const root = document.documentElement.style;
const swatches = document.getElementById("swatches");

function applyTheme(t, save) {
  theme = t;
  const bg = rgb(t[1]), white = [255, 255, 255];
  root.setProperty("--bg", t[1]);
  root.setProperty("--surface", css(mix(bg, white, 0.04)));
  root.setProperty("--line", css(mix(bg, white, 0.13)));
  root.setProperty("--accent", t[2][2]);
  root.setProperty("--accent-dim", t[2][0]);
  document.querySelector('meta[name="theme-color"]').content = t[1];
  for (const b of swatches.children) b.setAttribute("aria-pressed", String(b.dataset.name === t[0]));
  if (save) {
    try { localStorage.setItem("glyphdrift-theme", t[0]); } catch { /* storage unavailable */ }
  }
  draw(performance.now());
}

for (const t of THEMES) {
  const b = document.createElement("button");
  b.className = "swatch";
  b.type = "button";
  b.dataset.name = t[0];
  b.setAttribute("aria-pressed", "false");
  b.style.background = t[1];
  const stops = [t[1], ...t[2]].join(", ");
  b.innerHTML = `<span></span><span class="bar" style="background: linear-gradient(90deg, ${stops})"></span>`;
  b.firstChild.textContent = t[0];
  b.addEventListener("click", () => applyTheme(t, true));
  swatches.appendChild(b);
}

let saved = null;
try { saved = localStorage.getItem("glyphdrift-theme"); } catch { /* storage unavailable */ }
resize();
applyTheme(THEMES.find((t) => t[0] === saved) || theme, false);
requestAnimationFrame(frame);
