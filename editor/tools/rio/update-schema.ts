// Regenerates schema/rio-source.json from Rio's source code.
//
//   node tools/rio/update-schema.ts            # use the Rio revision pinned in tools/rio/schema-dump/Cargo.lock
//   node tools/rio/update-schema.ts --update   # bump to Rio's latest main first (cargo update)
//
// What it collects:
//   defaults      Config::default() serialised by tools/schema-dump (Rust, links rio-backend)
//   enums         `pub enum` variants + serde aliases scraped from the Rio checkout
//   colorDefaults default hex value per `[colors]` key, scraped from rio-vt
//
// Requires cargo. Builds third-party code (Rio) on first run.

import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync, readdirSync, statSync, existsSync } from "node:fs";
import { join, dirname } from "node:path";
import { homedir } from "node:os";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..", "..");
const dumpDir = join(root, "tools", "rio", "schema-dump");

if (process.argv.includes("--update")) {
  execFileSync("cargo", ["update", "-p", "rio-backend"], { cwd: dumpDir, stdio: "inherit" });
}

const defaultsJson = execFileSync("cargo", ["run", "-q", "--release"], {
  cwd: dumpDir,
  encoding: "utf8",
  stdio: ["ignore", "pipe", "inherit"],
});
const defaults = JSON.parse(defaultsJson);

const lock = readFileSync(join(dumpDir, "Cargo.lock"), "utf8");
const rev = /name = "rio-backend"\s+version = "[^"]+"\s+source = "git\+[^#]+#([0-9a-f]+)"/.exec(lock)?.[1];
if (!rev) throw new Error("Could not find the rio-backend git revision in Cargo.lock");

const checkoutsRoot = join(process.env.CARGO_HOME ?? join(homedir(), ".cargo"), "git", "checkouts");
const checkout = readdirSync(checkoutsRoot)
  .filter((d) => d.startsWith("rio-"))
  .map((d) => join(checkoutsRoot, d, rev.slice(0, 7)))
  .find((p) => existsSync(p));
if (!checkout) throw new Error(`Rio checkout for ${rev} not found under ${checkoutsRoot}`);

function rustFiles(dir: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    const p = join(dir, name);
    if (statSync(p).isDirectory()) return name === "tests" ? [] : rustFiles(p);
    return name.endsWith(".rs") ? [p] : [];
  });
}

type EnumVariant = { name: string; aliases: string[]; isDefault: boolean };
const enums: Record<string, { file: string; renameAll: string | null; variants: EnumVariant[] }> = {};

const enumSources = [
  "rio-backend/src/config",
  "rio-vt/src/ansi",
  "rio-vt/src/config",
  "sugarloaf/src/font",
].map((p) => join(checkout, p));

for (const file of enumSources.flatMap(rustFiles)) {
  const src = readFileSync(file, "utf8");
  const enumRx = /((?:#\[[^\]]*\]\s*)*)pub enum (\w+)\s*\{/g;
  for (let m; (m = enumRx.exec(src)); ) {
    const attrs = m[1];
    const name = m[2];
    // Only serde-visible enums are config values.
    if (!/Deserialize/.test(attrs)) continue;
    const bodyStart = enumRx.lastIndex;
    let depth = 1, i = bodyStart;
    while (depth > 0 && i < src.length) {
      if (src[i] === "{") depth++;
      else if (src[i] === "}") depth--;
      i++;
    }
    const body = src.slice(bodyStart, i - 1).replace(/\/\/.*$/gm, "");
    const variants: EnumVariant[] = [];
    const varRx = /((?:#\[[^\]]*\]\s*)*)([A-Z]\w*)\s*(?:\([^)]*\)|\{[^}]*\})?\s*(?:=\s*\d+)?\s*,?/g;
    for (let v; (v = varRx.exec(body)); ) {
      const vAttrs = v[1];
      const aliases = [...vAttrs.matchAll(/alias\s*=\s*"([^"]+)"/g)].map((a) => a[1]);
      const rename = /rename\s*=\s*"([^"]+)"/.exec(vAttrs)?.[1];
      variants.push({
        name: rename ?? v[2],
        aliases,
        isDefault: /#\[default\]/.test(vAttrs),
      });
    }
    const renameAll = /rename_all\s*=\s*"([^"]+)"/.exec(attrs)?.[1] ?? null;
    enums[name] = { file: file.slice(checkout.length + 1), renameAll, variants };
  }
}

// [colors] defaults: field (+ rename) -> `defaults::fn` -> hex in defaults.rs
const colorsDir = join(checkout, "rio-vt", "src", "config", "colors");
const defaultsRs = readFileSync(join(colorsDir, "defaults.rs"), "utf8");
const hexByFn: Record<string, string> = {};
const toHex = (n: number) => Math.round(n * 255).toString(16).padStart(2, "0");
for (const m of defaultsRs.matchAll(/pub fn (\w+)\(\)[^{]*\{([\s\S]*?)\n\}/g)) {
  const hex = /from_hex\(String::from\("(#[0-9A-Fa-f]{6,8})"\)/.exec(m[2])?.[1];
  // Some defaults are literal float arrays: [r, g, b, a] in 0..1.
  const arr = /\[\s*([\d.]+)\s*,\s*([\d.]+)\s*,\s*([\d.]+)\s*,\s*([\d.]+)\s*\]/.exec(m[2]);
  if (hex) hexByFn[m[1]] = hex.toLowerCase();
  else if (arr) hexByFn[m[1]] = "#" + [arr[1], arr[2], arr[3]].map((x) => toHex(parseFloat(x))).join("");
}
const colorsRs = readFileSync(join(colorsDir, "mod.rs"), "utf8");
const colorsStruct = (/pub struct Colors \{([\s\S]*?)\n\}/.exec(colorsRs)?.[1] ?? "") + "\n";
const colorDefaults: Record<string, string | null> = {};
// Each field's attributes are the text between the previous field and this one.
let prevEnd = 0;
for (const m of colorsStruct.matchAll(/pub (\w+):[^\n]*\n/g)) {
  const attrs = colorsStruct.slice(prevEnd, m.index);
  prevEnd = m.index! + m[0].length;
  const key = /rename\s*=\s*"([^"]+)"/.exec(attrs)?.[1] ?? m[1];
  const fn = /default\s*=\s*"(?:[\w:]*::)?(\w+)"/.exec(attrs)?.[1];
  colorDefaults[key] = fn ? hexByFn[fn] ?? null : null;
}

const out = {
  $comment: "GENERATED by tools/update-schema.ts. Do not edit; edit schema/rio/rio-overlay.json instead.",
  rioRevision: rev,
  defaults,
  enums,
  colorDefaults,
};
writeFileSync(join(root, "schema", "rio", "rio-source.json"), JSON.stringify(out, null, 2) + "\n");
console.log(
  `rio-source.json: rio@${rev.slice(0, 7)}, ${Object.keys(enums).length} enums, ` +
    `${Object.keys(colorDefaults).length} colors`,
);
