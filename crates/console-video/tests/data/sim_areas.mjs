// Runs the design page's own present() ("areas" way) on frames made from
// a fixed seed, and prints the rectangles it sends, one line per step.
// The Rust test tests/areas.rs makes the same frames and must print the
// same lines. Re-make the file with:
//   node tests/data/sim_areas.mjs > tests/data/sim_areas.txt
// The lines between COPIED START and COPIED END are copied unchanged from
// the Sim of the design page (lines 404-408, 420-427, 444-454, 525-577;
// the page is kept outside the repository).
// COPIED START
const W = 160, H = 120, K = 2, PW = W * K, PH = H * K;
const CLEAR = 255;            // "no pixel" in a sprite
const WINDOW_BYTES = 11;      // column, row and write commands for one area
const MAX_AREAS = 16;          // more areas than this in one frame are joined
const WINDOW_COST_PX = 24;    // what opening one more area is worth, in logical pixels
const fb = new Uint8Array(W * H);
const prev = new Uint8Array(W * H);
const changed = new Uint8Array(W * H);     // for the "what changed" picture only
const pal565 = new Uint16Array(256);
const palRGBA = new Uint32Array(256);
const panel = new Uint32Array(PW * PH);
let palDirty = true;

function cls(c) { fb.fill(c); }
function rect(x, y, w, h, c) {
  x |= 0; y |= 0; w |= 0; h |= 0;
  let x1 = Math.min(W, x + w), y1 = Math.min(H, y + h);
  x = Math.max(0, x); y = Math.max(0, y);
  for (let j = y; j < y1; j++) fb.fill(c, j * W + x, j * W + Math.max(x, x1));
}
function frame(x, y, w, h, c, t = 1) {
  rect(x, y, w, t, c); rect(x, y + h - t, w, t, c); rect(x, y, t, h, c); rect(x + w - t, y, t, h, c);
}
function px(x, y, c) { x |= 0; y |= 0; if (x >= 0 && y >= 0 && x < W && y < H) fb[y * W + x] = c; }
let mode = 'areas';
function present() {
  const rects = [];
  let changedPx = 0;
  const full = palDirty || mode === 'stream';
  // a palette change alters the colour of pixels whose bytes did not change,
  // so it always costs a full screen
  palDirty = false;
  let cur = null;
  for (let y = 0; y < H; y++) {
    const o = y * W;
    let a = -1, b = -1;
    for (let x = 0; x < W; x++) {
      const d = fb[o + x] !== prev[o + x];
      changed[o + x] = d || (full && mode !== 'stream') ? 1 : 0;
      if (d) { if (a < 0) a = x; b = x; changedPx++; }
    }
    if (full) continue;
    if (a < 0) { if (cur) { rects.push(cur); cur = null; } continue; }
    if (mode === 'rows') { a = 0; b = W - 1; }
    if (cur) {
      const x0 = Math.min(cur.x, a), x1 = Math.max(cur.x + cur.w - 1, b);
      if ((x1 - x0 + 1) * (cur.h + 1) <= cur.w * cur.h + (b - a + 1) + WINDOW_COST_PX) { cur.x = x0; cur.w = x1 - x0 + 1; cur.h++; continue; }
      rects.push(cur);
    }
    cur = { x: a, y, w: b - a + 1, h: 1 };
  }
  if (cur) rects.push(cur);
  // never more than MAX_AREAS in a frame: join the two neighbours whose joining wastes least
  while (rects.length > MAX_AREAS) {
    let best = 0, bestWaste = Infinity;
    for (let i = 0; i + 1 < rects.length; i++) {
      const p = rects[i], q = rects[i + 1];
      const x0 = Math.min(p.x, q.x), x1 = Math.max(p.x + p.w, q.x + q.w);
      const waste = (x1 - x0) * (q.y + q.h - p.y) - p.w * p.h - q.w * q.h;
      if (waste < bestWaste) { bestWaste = waste; best = i; }
    }
    const p = rects[best], q = rects[best + 1];
    const x0 = Math.min(p.x, q.x), x1 = Math.max(p.x + p.w, q.x + q.w);
    rects.splice(best, 2, { x: x0, y: p.y, w: x1 - x0, h: q.y + q.h - p.y });
  }
  if (full) rects.push({ x: 0, y: 0, w: W, h: H });
  let bytes = 0;
  for (const r of rects) {
    for (let y = r.y; y < r.y + r.h; y++) for (let x = r.x; x < r.x + r.w; x++) {
      const v = palRGBA[fb[y * W + x]], p = (y * K) * PW + x * K;
      panel[p] = v; panel[p + 1] = v; panel[p + PW] = v; panel[p + PW + 1] = v;
    }
    bytes += r.w * K * r.h * K * 2 + WINDOW_BYTES;
  }
  prev.set(fb);
  return { rects, bytes, changedPx, full };
}
// COPIED END

// xorshift32: the same numbers as the Rust test.
let seed = 0x2545f491;
function next() { seed ^= seed << 13; seed >>>= 0; seed ^= seed >>> 17; seed ^= seed << 5; seed >>>= 0; return seed; }
function rnd(n) { return next() % n; }

const STEPS = 400;
for (let step = 0; step < STEPS; step++) {
  const kind = rnd(6);
  if (kind === 0) {                       // a few sprites: random rectangles
    const m = 1 + rnd(20);
    for (let i = 0; i < m; i++) rect(rnd(180) - 10, rnd(140) - 10, 1 + rnd(40), 1 + rnd(30), rnd(256));
  } else if (kind === 1) {                // scattered single pixels
    const m = 1 + rnd(60);
    for (let i = 0; i < m; i++) px(rnd(W), rnd(H), rnd(256));
  } else if (kind === 2) {                // a diagonal line
    const x0 = rnd(W), y0 = rnd(H), len = 1 + rnd(100), c = rnd(256), up = rnd(2);
    for (let i = 0; i < len; i++) px(x0 + i, up ? y0 - i : y0 + i, c);
  } else if (kind === 3) {                // scroll up by one row
    fb.copyWithin(0, W);
    for (let x = 0; x < W; x++) fb[(H - 1) * W + x] = rnd(256);
  } else if (kind === 4) {                // many thin rows far apart
    const m = 1 + rnd(40);
    for (let i = 0; i < m; i++) rect(rnd(W), rnd(H), 1 + rnd(8), 1, rnd(256));
  }                                       // kind 5: nothing changes
  const { rects } = present();
  console.log(step + ':' + rects.map((r) => ` ${r.x},${r.y},${r.w},${r.h}`).join(''));
}
