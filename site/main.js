// Glyphdrift landing page: the theme picker, downloads, image viewer, and the app's own
// animations through live.js (WebAssembly): Topographic drift in the header, flowing around
// the logo box, and the gallery previews.

// [label, background, [dim, mid, bright], config name] — the cool gradients from src/theme.rs.
const THEMES = [
  ["Matrix", "#000000", ["#005522", "#00aa44", "#00ff66"], "matrix"],
  ["Mono", "#000000", ["#3c3c3c", "#9a9a9a", "#ffffff"], "mono"],
  ["Amber CRT", "#0a0600", ["#5a3200", "#c77800", "#ffb000"], "amber"],
  ["Catppuccin Mocha", "#1e1e2e", ["#89b4fa", "#74c7ec", "#89dceb"], "catppuccin-mocha"],
  ["Tokyo Night", "#1a1b26", ["#7aa2f7", "#7dcfff", "#c0caf5"], "tokyo-night"],
  ["Nord", "#2e3440", ["#5e81ac", "#81a1c1", "#88c0d0"], "nord"],
  ["Gruvbox Dark", "#282828", ["#458588", "#83a598", "#8ec07c"], "gruvbox"],
  ["Dracula", "#282a36", ["#6272a4", "#bd93f9", "#8be9fd"], "dracula"],
  ["Rosé Pine", "#191724", ["#31748f", "#9ccfd8", "#e0def4"], "rose-pine"],
  ["Kanagawa", "#1f1f28", ["#7e9cd8", "#7fb4ca", "#dcd7ba"], "kanagawa"],
  ["One Dark", "#282c34", ["#c678dd", "#61afef", "#56b6c2"], "one-dark"],
  ["Solarized Dark", "#002b36", ["#268bd2", "#2aa198", "#93a1a1"], "solarized-dark"],
  ["Everforest", "#2d353b", ["#7fbbb3", "#83c092", "#d3c6aa"], "everforest"],
  ["Monokai", "#272822", ["#ae81ff", "#66d9ef", "#a6e22e"], "monokai"],
  ["Ayu Dark", "#0b0e14", ["#39bae6", "#95e6cb", "#bfbdb6"], "ayu-dark"],
  ["Night Owl", "#011627", ["#82aaff", "#7fdbca", "#d6deeb"], "night-owl"],
  ["Palenight", "#292d3e", ["#c792ea", "#82aaff", "#89ddff"], "palenight"],
  ["GitHub Dark", "#0d1117", ["#58a6ff", "#79c0ff", "#c9d1d9"], "github-dark"],
  ["Oxocarbon", "#161616", ["#78a9ff", "#33b1ff", "#3ddbd9"], "oxocarbon"],
  ["Poimandres", "#1b1e28", ["#5de4c7", "#89ddff", "#add7ff"], "poimandres"],
  ["Synthwave '84", "#262335", ["#b893ce", "#ff7edb", "#36f9f6"], "synthwave-84"],
  ["Claude", "#262624", ["#cc785c", "#d97757", "#f0efea"], "claude"],
];
const DEFAULT_THEME = "Mono";

const rgb = (hex) => [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16));
const mix = (a, b, t) => a.map((v, i) => Math.round(v + (b[i] - v) * t));
const css = (c) => `rgb(${c[0]},${c[1]},${c[2]})`;

const canvas = document.getElementById("terrain");
const markBox = document.querySelector(".mark");
const reduceMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
let theme = THEMES.find((t) => t[0] === DEFAULT_THEME);

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
  recolorLive();
}

for (const t of THEMES) {
  const b = document.createElement("button");
  b.className = "swatch";
  b.type = "button";
  b.dataset.name = t[0];
  b.setAttribute("aria-pressed", "false");
  b.style.background = t[1];
  // Built with the DOM, not innerHTML: the Content-Security-Policy (_headers) blocks inline
  // style attributes, but allows setting styles from script.
  const name = document.createElement("span");
  name.textContent = t[0];
  const bar = document.createElement("span");
  bar.className = "bar";
  bar.style.background = `linear-gradient(90deg, ${[t[1], ...t[2]].join(", ")})`;
  b.append(name, bar);
  b.addEventListener("click", () => applyTheme(t, true));
  swatches.appendChild(b);
}

// ---------------------------------------------------------------------------------------
// Downloads: the visitor's platform first, as the primary button

const isMac = /Mac/i.test(navigator.userAgentData?.platform || navigator.platform || navigator.userAgent);
if (isMac) {
  const downloads = document.getElementById("downloads");
  const mac = downloads.querySelector('[data-platform="mac"]');
  const windows = downloads.querySelector('[data-platform="windows"]');
  mac.classList.add("primary");
  windows.classList.remove("primary");
  downloads.insertBefore(mac, windows);
}

// ---------------------------------------------------------------------------------------
// Navigation: tinted once the page scrolls past the top.

const nav = document.querySelector(".nav");
const tintNav = () => nav.classList.toggle("scrolled", window.scrollY > 24);
window.addEventListener("scroll", tintNav, { passive: true });
tintNav();

// ---------------------------------------------------------------------------------------
// Get started: one platform's steps at a time, starting with the visitor's.

const platforms = document.querySelector(".platforms");
function showPlatform(name) {
  for (const el of document.querySelectorAll(".steps [data-platform]")) el.hidden = el.dataset.platform !== name;
  for (const b of platforms.querySelectorAll("button")) b.setAttribute("aria-pressed", String(b.dataset.show === name));
}
for (const b of platforms.querySelectorAll("button")) b.addEventListener("click", () => showPlatform(b.dataset.show));
platforms.hidden = false;
showPlatform(isMac ? "mac" : "windows");

// ---------------------------------------------------------------------------------------
// Viewer: a larger image in a dialog. Gallery images step with the buttons, arrow keys or
// a swipe; the "in use" image opens on its own. Without JavaScript the links open the image.

const viewer = document.getElementById("viewer");
const frameBox = viewer.querySelector(".viewer-frame");
const big = frameBox.querySelector("img");
const caption = viewer.querySelector("figcaption");
const prev = viewer.querySelector(".viewer-prev");
const next = viewer.querySelector(".viewer-next");
const gallery = [...document.querySelectorAll(".gallery .zoom")];
let shown = null; // the link being viewed
let opener = null; // where focus goes back to

function show(link) {
  shown = link;
  const thumb = link.querySelector("img");
  big.classList.remove("loaded");
  frameBox.style.backgroundImage = `url("${thumb.currentSrc || thumb.src}")`;
  big.alt = thumb.alt;
  big.removeAttribute("srcset");
  if (link.dataset.srcset) {
    big.sizes = "(max-width: 640px) 100vw, 92vw";
    big.srcset = link.dataset.srcset;
  }
  big.src = link.href;
  const card = link.closest("li");
  if (card) {
    caption.replaceChildren(...[...card.querySelector("p").childNodes].map((n) => n.cloneNode(true)));
  } else {
    caption.textContent = link.dataset.caption || "";
  }
  const single = !gallery.includes(link);
  prev.hidden = next.hidden = single;
  showLive(link);
}

function step(by) {
  const i = gallery.indexOf(shown);
  if (i < 0) return;
  show(gallery[(i + by + gallery.length) % gallery.length]);
}

big.addEventListener("load", () => big.classList.add("loaded"));
for (const link of document.querySelectorAll(".zoom")) {
  link.addEventListener("click", (e) => {
    if (e.metaKey || e.ctrlKey || e.shiftKey || e.button !== 0) return; // new tab etc.
    e.preventDefault();
    opener = link;
    show(link);
    viewer.showModal();
    viewer.focus(); // the dialog, not its first button: no focus ring on a tap
  });
}
prev.addEventListener("click", () => step(-1));
next.addEventListener("click", () => step(1));
viewer.querySelector(".viewer-close").addEventListener("click", () => viewer.close());
// A click outside the image and buttons closes it.
viewer.addEventListener("click", (e) => {
  if (e.target === viewer || e.target.tagName === "FIGURE") viewer.close();
});
viewer.addEventListener("keydown", (e) => {
  if (e.key === "ArrowLeft") step(-1);
  if (e.key === "ArrowRight") step(1);
});
viewer.addEventListener("close", () => {
  stopLive();
  big.removeAttribute("src");
  big.removeAttribute("srcset");
  opener?.focus();
});
// Swipe left or right on touch screens.
let touchX = null;
viewer.addEventListener("touchstart", (e) => { touchX = e.touches[0].clientX; }, { passive: true });
viewer.addEventListener("touchend", (e) => {
  if (touchX === null) return;
  const dx = e.changedTouches[0].clientX - touchX;
  touchX = null;
  if (Math.abs(dx) > 50) step(dx < 0 ? 1 : -1);
});

// ---------------------------------------------------------------------------------------
// Live previews: the real animations (live.js, WebAssembly), drawn in the theme picked on
// this page. Cards show a still frame and move while hovered or keyboard-focused; the
// viewer moves while open. With reduced motion they stay still; without WebAssembly the
// screenshots stay.

let live = null; // { Live, player } once loaded
let viewerLive = null;
const viewerCanvas = document.createElement("canvas");
viewerCanvas.className = "live";
viewerCanvas.setAttribute("aria-hidden", "true");
frameBox.append(viewerCanvas);

/// Preview speed, as the app's Speed setting: calmer than the default for a page full of them.
const LIVE_SPEED = 0.5;

/** The scene each screenshot shows: 1200 x 750 px, with its card's scale settings. */
function liveOptions(link) {
  const d = link.dataset;
  return {
    anim: d.anim, theme: theme[3], width: 1200, height: 750,
    zoom: Number(d.zoom), objectZoom: Number(d.objectZoom), clock: d.anim === "topo",
    speed: LIVE_SPEED,
  };
}

/** Replace a card's screenshot with a frame drawn in the current theme. */
function showCard(link) {
  if (!live || link.live) return;
  const canvas = document.createElement("canvas");
  canvas.className = "live";
  canvas.setAttribute("aria-hidden", "true");
  link.append(canvas);
  link.live = new live.Live(canvas, liveOptions(link));
  link.live.draw();
  canvas.classList.add("on");
}

function play(link) {
  if (!live || reduceMotion) return;
  showCard(link);
  live.player.start(link.live);
}

function pause(link) {
  if (link.live) live.player.stop(link.live);
}

for (const link of gallery) {
  link.addEventListener("pointerenter", (e) => { if (e.pointerType !== "touch") play(link); });
  link.addEventListener("pointerleave", () => pause(link));
  // Keyboard focus only: focus returning from the viewer after a click shouldn't play.
  link.addEventListener("focus", () => { if (link.matches(":focus-visible")) play(link); });
  link.addEventListener("blur", () => pause(link));
}

function showLive(link) {
  stopLive();
  for (const card of gallery) pause(card);
  if (!live || !link.dataset.anim) return;
  viewerLive = new live.Live(viewerCanvas, liveOptions(link));
  viewerLive.draw();
  viewerCanvas.classList.add("on");
  if (!reduceMotion) live.player.start(viewerLive);
}

function stopLive() {
  viewerCanvas.classList.remove("on");
  if (!viewerLive) return;
  live.player.stop(viewerLive);
  viewerLive.dispose();
  viewerLive = null;
}

/** Redraw every preview, and the header, in the current theme. */
function recolorLive() {
  if (!live) return;
  for (const link of gallery) link.live?.setTheme(theme[3]);
  viewerLive?.setTheme(theme[3]);
  header?.setTheme(theme[3]);
}

// ---------------------------------------------------------------------------------------
// Header: Topographic drift at the app's own speed, flowing around the logo box, from a
// random starting point each visit. It moves while on screen; one still frame for reduced
// motion. A resize starts it again at the new size.

let header = null;
let headerOnScreen = true;
const headerSeed = Math.floor(Math.random() * 1000);

function startHeader() {
  if (!live) return;
  if (header) {
    live.player.stop(header);
    header.dispose();
  }
  const c = canvas.getBoundingClientRect(), b = markBox.getBoundingClientRect();
  header = new live.Live(canvas, {
    anim: "topo", theme: theme[3], seed: headerSeed,
    width: Math.max(1, Math.round(c.width)), height: Math.max(1, Math.round(c.height)),
    keepClear: {
      cx: b.left - c.left + b.width / 2, cy: b.top - c.top + b.height / 2,
      hw: b.width / 2, hh: b.height / 2,
    },
  });
  header.draw();
  if (!reduceMotion && headerOnScreen) live.player.start(header);
}

new IntersectionObserver(([e]) => {
  headerOnScreen = e.isIntersecting;
  if (!header || reduceMotion) return;
  if (headerOnScreen) live.player.start(header);
  else live.player.stop(header);
}).observe(canvas);

let resizeTimer = 0;
window.addEventListener("resize", () => {
  clearTimeout(resizeTimer);
  resizeTimer = setTimeout(startHeader, 150);
});

if ("IntersectionObserver" in window) {
  import("./live.js").then(async ({ load, Live, Player }) => {
    if (!(await load())) return;
    live = { Live, player: new Player() };
    startHeader();
    // Draw each card's first frame as it comes into view.
    const seen = new IntersectionObserver((entries) => {
      for (const e of entries) if (e.isIntersecting) showCard(e.target);
    }, { rootMargin: "200px" });
    for (const link of gallery) seen.observe(link);
    // The cards already on screen now, rather than at the observer's first report,
    // which browsers hold back until they next render.
    for (const link of gallery) {
      const r = link.getBoundingClientRect();
      if (r.bottom > 0 && r.top < window.innerHeight && r.width > 0) showCard(link);
    }
    if (viewer.open && shown) showLive(shown); // opened while loading
  }).catch(() => { /* the screenshots stay */ });
}

let saved = null;
try { saved = localStorage.getItem("glyphdrift-theme"); } catch { /* storage unavailable */ }
applyTheme(THEMES.find((t) => t[0] === saved) || theme, false);
