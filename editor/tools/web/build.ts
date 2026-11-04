// Builds the browser playground (gpui/apps/settings-web) into a static folder for GitHub Pages:
// index.html, settings_web.js (wasm-bindgen's glue) and settings_web_bg.wasm.
//
//   node tools/web/build.ts [--dev] [--out <dir>]
//       → gpui/target/web (or <dir>); serve it with any static server, e.g.
//         `python3 -m http.server -d gpui/target/web` or `npx serve gpui/target/web`
//
// Uses the `web` profile (size-optimised release, see gpui/Cargo.toml); --dev builds the debug
// profile, which is much faster to build and much bigger. Requires the wasm32-unknown-unknown
// target (`rustup target add wasm32-unknown-unknown`) and wasm-bindgen-cli at the same version as
// the `wasm-bindgen` crate in gpui/Cargo.lock (`cargo install wasm-bindgen-cli --version <it>`).

import { execFileSync } from "node:child_process";
import { copyFileSync, mkdirSync, readFileSync, rmSync, statSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { gzipSync } from "node:zlib";

const editor = join(dirname(fileURLToPath(import.meta.url)), "..", "..");
const workspace = join(editor, "gpui");
const args = process.argv.slice(2);
const dev = args.includes("--dev");
const outAt = args.indexOf("--out");
const out = outAt >= 0 ? resolve(args[outAt + 1]) : join(workspace, "target", "web");

const run = (cmd: string, cmdArgs: string[]) => execFileSync(cmd, cmdArgs, { cwd: workspace, stdio: "inherit" });

// The CLI must match the crate exactly, or the glue it writes doesn't fit the module.
const lock = readFileSync(join(workspace, "Cargo.lock"), "utf8");
const wanted = /\[\[package\]\]\nname = "wasm-bindgen"\nversion = "([^"]+)"/.exec(lock)?.[1];
let installed = "";
try {
  installed = execFileSync("wasm-bindgen", ["--version"], { encoding: "utf8" }).trim().split(" ")[1];
} catch {
  console.error(`wasm-bindgen isn't installed. Install it with: cargo install wasm-bindgen-cli --version ${wanted} --locked`);
  process.exit(1);
}
if (installed !== wanted) {
  console.error(`wasm-bindgen ${installed} is installed, but Cargo.lock has ${wanted}. Install the matching one with: cargo install wasm-bindgen-cli --version ${wanted} --locked`);
  process.exit(1);
}

const profile = dev ? "dev" : "web";
run("cargo", ["build", "--locked", "-p", "settings-web", "--target", "wasm32-unknown-unknown", "--profile", profile]);
const wasm = join(workspace, "target", "wasm32-unknown-unknown", dev ? "debug" : "web", "settings_web.wasm");

rmSync(out, { recursive: true, force: true });
mkdirSync(out, { recursive: true });
// The name section only labels stack frames; it nearly doubles the module.
run("wasm-bindgen", ["--target", "web", "--no-typescript", ...(dev ? [] : ["--remove-name-section", "--remove-producers-section"]), "--out-dir", out, wasm]);
copyFileSync(join(workspace, "apps", "settings-web", "static", "index.html"), join(out, "index.html"));

const bg = join(out, "settings_web_bg.wasm");
const mb = (n: number) => `${(n / 1024 / 1024).toFixed(1)} MB`;
console.log(`${out}\n  settings_web_bg.wasm: ${mb(statSync(bg).size)} raw, ${mb(gzipSync(readFileSync(bg), { level: 9 }).length)} gzipped`);
