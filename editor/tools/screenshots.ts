// Renders the README screenshots: both editors against demo configs, in light and dark,
// at 2x, driven through headless Chrome's DevTools protocol (Node's built-in WebSocket, no deps).
//
//   dotnet publish src/RioSettings -c Release -o out && dotnet publish src/GhosttySettings -c Release -o out
//   node tools/screenshots.ts            # writes ../docs/images/*.png
//
// Needs Google Chrome. Uses throwaway demo configs; never touches your real config files.

import { spawn, type ChildProcess } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const outDir = join(root, "..", "docs", "images");
mkdirSync(outDir, { recursive: true });
const chromeBin = process.env.CHROME ?? "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
const work = mkdtempSync(join(tmpdir(), "settings-shots-"));
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
const children: ChildProcess[] = [];

/** Starts an editor on a demo config and returns its tokenised URL. */
async function startEditor(exe: string, config: string, port: number): Promise<string> {
  const p = spawn(join(root, "out", exe), ["--config", config, "--port", String(port), "--no-open", "--stay"]);
  children.push(p);
  return await new Promise((resolve, reject) => {
    let buf = "";
    p.stdout!.on("data", (d) => {
      buf += d;
      const m = /Open (http:\/\/\S+)/.exec(buf);
      if (m) resolve(m[1]);
    });
    p.on("exit", (code) => reject(new Error(`${exe} exited ${code}`)));
  });
}

/** Minimal CDP client over one page target. */
async function openPage(debugPort: number) {
  const target = await (await fetch(`http://127.0.0.1:${debugPort}/json/new?about:blank`, { method: "PUT" })).json();
  const ws = new WebSocket(target.webSocketDebuggerUrl);
  await new Promise((r) => ws.addEventListener("open", r, { once: true }));
  let id = 0;
  const pending = new Map<number, (v: any) => void>();
  ws.addEventListener("message", (e) => {
    const msg = JSON.parse(String(e.data));
    if (msg.id && pending.has(msg.id)) { pending.get(msg.id)!(msg); pending.delete(msg.id); }
  });
  const send = (method: string, params: object = {}) =>
    new Promise<any>((resolve) => { const n = ++id; pending.set(n, resolve); ws.send(JSON.stringify({ id: n, method, params })); });
  return { send, close: () => ws.close() };
}

type Shot = { name: string; url: string; scheme: "light" | "dark"; script: string; width?: number; height?: number };

async function capture(page: Awaited<ReturnType<typeof openPage>>, shot: Shot) {
  await page.send("Emulation.setDeviceMetricsOverride", { width: shot.width ?? 1440, height: shot.height ?? 900, deviceScaleFactor: 2, mobile: false });
  await page.send("Emulation.setEmulatedMedia", { features: [{ name: "prefers-color-scheme", value: shot.scheme }] });
  await page.send("Page.navigate", { url: shot.url });
  await sleep(2500);
  await page.send("Runtime.evaluate", { expression: `(async () => { ${shot.script} })()`, awaitPromise: true });
  await sleep(800);
  // Tidy demo path in the header (set right before capture; the page refreshes it every few seconds).
  await page.send("Runtime.evaluate", { expression: `{ const p = document.getElementById("config-path"); if (p) p.textContent = p.textContent.includes("ghostty") ? "~/.config/ghostty/config" : "~/.config/rio/config.toml"; }` });
  const { result } = await page.send("Page.captureScreenshot", { format: "png" });
  writeFileSync(join(outDir, shot.name + ".png"), Buffer.from(result.data, "base64"));
  console.log("wrote", shot.name + ".png");
}

const rioConfig = join(work, "rio", "config.toml");
const ghosttyConfig = join(work, "ghostty", "config");
mkdirSync(dirname(rioConfig), { recursive: true });
mkdirSync(dirname(ghosttyConfig), { recursive: true });
writeFileSync(rioConfig, `# My Rio setup\n\n[window]\nopacity = 0.95\nblur = true\n\n[fonts]\nsize = 15.0\n\n[colors]\nbackground = "#1e1e2e"\nforeground = "#cdd6f4"\ncursor = "#f5e0dc"\n`);
writeFileSync(ghosttyConfig, `# My Ghostty setup\nfont-size = 14\ncursor-style = bar\nwindow-padding-x = 8\nshell-integration-features = cursor,title,ssh-env,path\n`);

try {
  const rioUrl = await startEditor("rio-settings", rioConfig, 5511);
  const ghosttyUrl = await startEditor("ghostty-settings", ghosttyConfig, 5512);

  const chrome = spawn(chromeBin, ["--headless=new", "--disable-gpu", "--hide-scrollbars", "--remote-debugging-port=9339",
    `--user-data-dir=${join(work, "chrome")}`, "about:blank"], { stdio: "ignore" });
  children.push(chrome);
  await sleep(2000);
  const page = await openPage(9339);

  const scrollTo = (id: string, extra = "") =>
    `document.getElementById(${JSON.stringify(id)}).scrollIntoView(); window.scrollBy(0, -16); ${extra}`;
  const edit = (inputId: string, value: string) =>
    `{ const i = document.getElementById(${JSON.stringify(inputId)}); i.value = ${JSON.stringify(value)}; i.dispatchEvent(new Event("input")); }`;
  const openPicker = (inputId: string) =>
    `{ const i = document.getElementById(${JSON.stringify(inputId)}); i.scrollIntoView({ block: "start" }); window.scrollBy(0, -140); i.click(); }`;

  const shots: Shot[] = [
    { name: "rio-dark", url: rioUrl, scheme: "dark",
      script: scrollTo("g-colors", edit("c-colors-cursor", "#cba6f7")) },
    { name: "rio-light", url: rioUrl, scheme: "light", script: scrollTo("g-window") },
    { name: "ghostty-light", url: ghosttyUrl, scheme: "light", script: openPicker("c-theme") },
    { name: "ghostty-dark", url: ghosttyUrl, scheme: "dark",
      script: scrollTo("g-cursor", edit("c-cursor-opacity", "0.85") + `document.getElementById("save").click(); await new Promise(r => setTimeout(r, 900));`) },
  ];
  for (const s of shots) await capture(page, s);

  // Hero: two editors overlapping on a gradient.
  const hero = join(work, "hero.html");
  writeFileSync(hero, `<!doctype html><html><body style="margin:0;width:1600px;height:900px;overflow:hidden;
    background: radial-gradient(circle at 15% 20%, #e879f9 0, transparent 45%), radial-gradient(circle at 85% 80%, #3551f3 0, transparent 45%), #0f0f11;
    font-family: -apple-system, system-ui, sans-serif;">
    <img src="file://${join(outDir, "rio-dark.png")}" style="position:absolute;left:70px;top:150px;width:900px;border-radius:14px;box-shadow:0 30px 80px #000a;transform:rotate(-2.5deg)">
    <img src="file://${join(outDir, "ghostty-light.png")}" style="position:absolute;right:70px;top:120px;width:900px;border-radius:14px;box-shadow:0 30px 80px #000a;transform:rotate(2deg)">
    <div style="position:absolute;left:0;right:0;top:36px;text-align:center;color:#fff;font-size:44px;font-weight:700;letter-spacing:-.02em;text-shadow:0 2px 20px #0008">
      Settings for Rio &amp; Ghostty, in your browser</div></body></html>`);
  await capture(page, { name: "hero", url: "file://" + hero, scheme: "dark", script: "", width: 1600, height: 900 });
  page.close();
} finally {
  for (const c of children) c.kill();
  await sleep(1000); // let Chrome finish writing its profile before removing it
  rmSync(work, { recursive: true, force: true, maxRetries: 5, retryDelay: 300 });
}
