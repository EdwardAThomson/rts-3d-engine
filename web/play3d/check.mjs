// Opens the browser viewer in headless Chromium and checks it draws the game and takes keys and the mouse: once
// with WebGPU and once with WebGL2 (WebGPU switched off), each on Chromium's software GPU. Saves each frame as a PNG to look at.
// Follows the Classic engine's check for its browser player (rts-engine, web/play/check.mjs).
//
//   cargo build --release --target wasm32-unknown-unknown -p render3d --bin play3d
//   wasm-bindgen --target web --no-typescript --out-dir web/play3d/pkg target/wasm32-unknown-unknown/release/play3d.wasm
//   python3 -m http.server 8000 &
//   node web/play3d/check.mjs [http://localhost:8000] [folder for the PNGs]
//
// Needs the `playwright` package and its Chromium (npm i playwright && npx playwright install chromium).

import { writeFileSync } from "node:fs";
import { chromium } from "playwright";

const base = process.argv[2] ?? "http://localhost:8000";
const out = process.argv[3] ?? ".";
const software = ["--use-angle=swiftshader", "--enable-unsafe-swiftshader"];
const runs = [
  { name: "webgpu", args: ["--enable-unsafe-webgpu", "--enable-features=Vulkan", ...software], backend: "BrowserWebGpu" },
  { name: "webgl2", args: ["--disable-features=WebGPU", ...software], backend: "Gl" },
];

// Headless Chromium draws WebGPU canvases but never composites them, so a screenshot comes out blank. Read the
// frame back on the GPU instead: let the canvas texture be copied, and after the next submit once `wantFrame` is
// set, copy the frame to a buffer.
function readWebGpuFrames() {
  if (!globalThis.GPUCanvasContext) return;
  const configure = GPUCanvasContext.prototype.configure;
  GPUCanvasContext.prototype.configure = function (c) {
    this.device = c.device;
    const usage = (c.usage ?? GPUTextureUsage.RENDER_ATTACHMENT) | GPUTextureUsage.COPY_SRC;
    return configure.call(this, { ...c, usage });
  };
  const current = GPUCanvasContext.prototype.getCurrentTexture;
  GPUCanvasContext.prototype.getCurrentTexture = function () {
    const texture = current.call(this);
    window.frameTexture = { texture, device: this.device };
    return texture;
  };
  const submit = GPUQueue.prototype.submit;
  GPUQueue.prototype.submit = function (commands) {
    submit.call(this, commands);
    const f = window.frameTexture;
    if (!f || !window.wantFrame) return;
    window.wantFrame = false;
    const { texture: t, device } = f;
    const row = Math.ceil((t.width * 4) / 256) * 256;
    const buffer = device.createBuffer({ size: row * t.height, usage: GPUBufferUsage.COPY_DST | GPUBufferUsage.MAP_READ });
    const enc = device.createCommandEncoder();
    enc.copyTextureToBuffer({ texture: t }, { buffer, bytesPerRow: row }, [t.width, t.height]);
    submit.call(this, [enc.finish()]);
    buffer.mapAsync(GPUMapMode.READ).then(() => {
      const src = new Uint8Array(buffer.getMappedRange());
      const bgra = t.format.startsWith("bgra");
      const rgba = new Uint8ClampedArray(t.width * t.height * 4);
      for (let y = 0; y < t.height; y++) {
        for (let x = 0; x < t.width; x++) {
          const i = y * row + x * 4, o = (y * t.width + x) * 4;
          rgba[o] = src[i + (bgra ? 2 : 0)];
          rgba[o + 1] = src[i + 1];
          rgba[o + 2] = src[i + (bgra ? 0 : 2)];
          rgba[o + 3] = 255;
        }
      }
      window.webGpuFrame = { width: t.width, height: t.height, rgba };
      buffer.unmap();
    });
  };
}

// The frame as a PNG, the number of distinct colours in it (lit terrain has hundreds, a blank canvas one) and a
// fingerprint to tell two frames apart.
async function frame(page, run) {
  const png = run.name === "webgpu"
    ? await page.evaluate(async () => {
        window.webGpuFrame = null;
        window.wantFrame = true;
        for (let i = 0; i < 100 && !window.webGpuFrame; i++) await new Promise((r) => setTimeout(r, 100));
        const f = window.webGpuFrame;
        if (!f) return null;
        const c = new OffscreenCanvas(f.width, f.height);
        c.getContext("2d").putImageData(new ImageData(f.rgba, f.width, f.height), 0, 0);
        const bytes = new Uint8Array(await (await c.convertToBlob()).arrayBuffer());
        let s = "";
        for (const b of bytes) s += String.fromCharCode(b);
        return btoa(s);
      }).then((b64) => b64 && Buffer.from(b64, "base64"))
    : await page.locator("#game").screenshot();
  if (!png) return { png: null, colours: 0, print: "" };
  const { colours, print } = await page.evaluate(async (b64) => {
    const img = new Image();
    img.src = `data:image/png;base64,${b64}`;
    await img.decode();
    const c = new OffscreenCanvas(img.width, img.height);
    const g = c.getContext("2d");
    g.drawImage(img, 0, 0);
    const px = g.getImageData(0, 0, img.width, img.height).data;
    const seen = new Set();
    let sum = 0;
    for (let i = 0; i < px.length; i += 4) {
      seen.add((px[i] << 16) | (px[i + 1] << 8) | px[i + 2]);
      sum = (sum * 31 + px[i] + px[i + 1] * 7 + px[i + 2] * 13) % 1000000007;
    }
    return { colours: seen.size, print: String(sum) };
  }, png.toString("base64"));
  return { png, colours, print };
}

let failed = false;
for (const run of runs) {
  const browser = await chromium.launch({ args: run.args });
  const page = await browser.newPage({ viewport: { width: 960, height: 600 } });
  await page.addInitScript(readWebGpuFrames);
  const errors = [];
  let drawing = "";
  page.on("pageerror", (e) => {
    // winit hands control back to the browser by throwing; that one is expected.
    if (!String(e).includes("Using exceptions for control flow")) errors.push(String(e));
  });
  page.on("console", (m) => {
    if (m.text().startsWith("drawing with")) drawing = m.text();
    if (m.type() === "error") errors.push(m.text());
  });
  await page.goto(`${base}/web/play3d/?seed=1&speed=8`);
  const titled = (re) => page.waitForFunction((s) => new RegExp(s).test(document.title), re.source, { timeout: 30_000 })
    .then(() => true, () => false);
  const ticking = await titled(/tick ([2-9]\d|\d\d\d)/);
  // Space pauses: the key reaches the game through the canvas.
  await page.locator("#game").focus();
  await page.keyboard.press("Space");
  const paused = await titled(/paused/);
  // Dragging a box over the whole map selects your one builder; a right-click then orders it without the browser's
  // own menu opening.
  await page.mouse.move(5, 5);
  await page.mouse.down();
  await page.mouse.move(955, 595, { steps: 5 });
  await page.mouse.up();
  const selected = await titled(/, 1 selected/);
  await page.evaluate(() => window.addEventListener("contextmenu", (e) => (window.menuBlocked = e.defaultPrevented)));
  await page.mouse.click(480, 300, { button: "right" });
  const noMenu = await page.evaluate(() => window.menuBlocked === true);
  const shot = await frame(page, run);
  if (shot.png) writeFileSync(`${out}/play3d-${run.name}.png`, shot.png);
  // The wheel zooms in at the cursor: with the game paused, the next frame differs.
  await page.mouse.move(480, 300);
  for (let i = 0; i < 5; i++) await page.mouse.wheel(0, -120);
  await page.waitForTimeout(300);
  const zoomed = await frame(page, run);
  if (zoomed.png) writeFileSync(`${out}/play3d-${run.name}-zoomed.png`, zoomed.png);
  const checks = {
    [`drew with ${run.backend}`]: drawing.includes(`(${run.backend},`),
    "the game ticks": ticking,
    "the frame shows the map": shot.colours >= 64,
    "Space pauses": paused,
    "a drag selects": selected,
    "right-click gives no menu": noMenu,
    "the wheel zooms": zoomed.colours >= 64 && zoomed.print !== shot.print,
    "no errors": errors.length === 0,
  };
  const bad = Object.entries(checks).filter(([, ok]) => !ok).map(([name]) => name);
  failed ||= bad.length > 0;
  console.log(`${bad.length ? "FAIL" : "ok  "} ${run.name}: ${drawing}; ${shot.colours} colours; ${await page.title()}`);
  for (const b of bad) console.log(`     failed: ${b}`);
  for (const e of errors) console.log(`     error: ${e}`);
  await browser.close();
}
process.exit(failed ? 1 : 0);
