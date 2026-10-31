// Regenerates schema/ghostty/ghostty-source.json from the installed Ghostty.
//
//   node tools/ghostty/update-schema.ts
//
// What it collects:
//   options   key order, defaults and docs from `ghostty +show-config --default --docs`
//   types     field types, enums and flag sets from src/config/Config.zig at the
//             git tag matching the installed version (falls back to main)
//   values    enum value lists, probed via `ghostty +validate-config` with an
//             invalid value (authoritative; covers enums defined outside Config.zig)
//
// Requires Ghostty, network access to GitHub, and optionally `gh` for the tag check.

import { execFileSync, spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync, existsSync } from "node:fs";
import { join, dirname } from "node:path";
import { tmpdir } from "node:os";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..", "..");
const outFile = join(root, "schema", "ghostty", "ghostty-source.json");

const ghostty =
  process.env.GHOSTTY_BIN ??
  [
    "/Applications/Ghostty.app/Contents/MacOS/ghostty",
    ...(process.env.PATH ?? "").split(":").map((p) => join(p, "ghostty")),
  ].find((p) => existsSync(p));
if (!ghostty) {
  console.error(
    "ghostty not found. Install Ghostty, or set GHOSTTY_BIN to the ghostty binary.",
  );
  process.exit(1);
}

function run(args: string[]): string {
  return execFileSync(ghostty!, args, { encoding: "utf8", maxBuffer: 64 * 1024 * 1024 });
}

// ---------------------------------------------------------------------------
// Version and Config.zig

const version = /^\s*-\s*version:\s*(\S+)/m.exec(run(["--version"]))?.[1];
if (!version) throw new Error("Could not parse `ghostty --version` output");

function tagExists(tag: string): boolean {
  const r = spawnSync("gh", ["api", `repos/ghostty-org/ghostty/git/refs/tags/${tag}`], {
    encoding: "utf8",
  });
  if (r.error) return false; // gh not available; the raw fetch below decides
  return r.status === 0;
}

async function fetchConfigZig(ref: string): Promise<string | null> {
  const url = `https://raw.githubusercontent.com/ghostty-org/ghostty/${ref}/src/config/Config.zig`;
  const res = await fetch(url);
  if (res.status === 404) return null;
  if (!res.ok) throw new Error(`GET ${url} failed: ${res.status} ${res.statusText}`);
  return res.text();
}

let configZigRef = `v${version}`;
let zig = tagExists(configZigRef) ? await fetchConfigZig(configZigRef) : null;
if (zig === null) {
  console.warn(`warning: tag ${configZigRef} not found; using Config.zig from main`);
  configZigRef = "main";
  zig = await fetchConfigZig("main");
  if (zig === null) throw new Error("Config.zig not found on main");
}
const zigSrc: string = zig;

// ---------------------------------------------------------------------------
// +show-config parsing

type ShowOption = { key: string; defaults: string[]; lines: number; doc: string; docFrom: string | null };

function parseShowConfig(text: string): ShowOption[] {
  const out = new Map<string, ShowOption>();
  let pending: string[] = [];
  let lastDoc = "";
  let lastDocKey: string | null = null;
  for (const line of text.split("\n")) {
    if (line.startsWith("#")) {
      pending.push(line.replace(/^# ?/, ""));
      continue;
    }
    if (line.trim() === "") continue;
    const m = /^([a-z0-9-]+) =(?: (.*))?$/.exec(line);
    if (!m) throw new Error(`Unexpected +show-config line: ${JSON.stringify(line)}`);
    const [, key, value = ""] = m;
    let opt = out.get(key);
    if (!opt) {
      let doc: string;
      let docFrom: string | null = null;
      if (pending.length) {
        doc = pending.join("\n").replace(/^\s+|\s+$/g, "");
        lastDoc = doc;
        lastDocKey = key;
      } else {
        // Grouped fields (font-family-bold after font-family) share the previous doc block.
        doc = lastDoc;
        docFrom = lastDocKey;
      }
      opt = { key, defaults: [], lines: 0, doc, docFrom };
      out.set(key, opt);
    }
    pending = [];
    opt.lines++;
    if (value !== "") opt.defaults.push(value);
  }
  return [...out.values()];
}

const show = parseShowConfig(run(["+show-config", "--default", "--docs"]));

// ---------------------------------------------------------------------------
// Config.zig parsing

/** Top-level fields at column 0: `name: Type = default,` or `@"name": Type = ...`. */
function parseZigFields(src: string): Map<string, string> {
  const fields = new Map<string, string>();
  for (const line of src.split("\n")) {
    const m = /^(?:@"([^"]+)"|([a-z_][a-z0-9_]*)): (.+)$/.exec(line);
    if (!m) continue;
    const name = m[1] ?? m[2];
    const rest = m[3];
    let depth = 0;
    let end = rest.length;
    for (let i = 0; i < rest.length; i++) {
      const c = rest[i];
      if ("([{".includes(c)) depth++;
      else if (")]}".includes(c)) depth--;
      else if (depth === 0 && (c === "," || (c === "=" && rest[i - 1] === " "))) {
        end = i;
        break;
      }
    }
    fields.set(name, rest.slice(0, end).trim());
  }
  return fields;
}

type TypeDecl = { kind: "enum" | "packed" | "other"; body: string };

/** Finds `const Name = enum {...}` / `packed struct {...}` anywhere in the file. */
function findDecl(src: string, name: string): TypeDecl | null {
  const re = new RegExp(
    `^\\s*(?:pub )?const ${name} = (enum|packed struct|struct|union|extern struct)\\b[^{;]*\\{`,
    "m",
  );
  const m = re.exec(src);
  if (!m) return null;
  const start = m.index + m[0].length;
  let depth = 1;
  let i = start;
  for (; i < src.length && depth > 0; i++) {
    const c = src[i];
    if (c === "/" && src[i + 1] === "/") {
      i = src.indexOf("\n", i);
      if (i < 0) break;
    } else if (c === '"') {
      i++;
      while (i < src.length && src[i] !== '"') i += src[i] === "\\" ? 2 : 1;
    } else if (c === "{") depth++;
    else if (c === "}") depth--;
  }
  const body = src.slice(start, i - 1);
  const kind = m[1] === "enum" ? "enum" : m[1] === "packed struct" ? "packed" : "other";
  return { kind, body };
}

/** Top-level lines of a container body (nested blocks removed, comments stripped). */
function topLevel(body: string): string {
  let out = "";
  let depth = 0;
  for (const raw of body.split("\n")) {
    const line = raw.replace(/\/\/.*$/, "");
    if (depth === 0) out += line + "\n";
    for (const c of line) {
      if (c === "{") depth++;
      else if (c === "}") depth--;
    }
  }
  return out;
}

const unquote = (s: string) => s.replace(/^@"(.*)"$/, "$1");

function enumMembers(body: string): string[] {
  const top = topLevel(body);
  const cut = top.search(/\b(pub|fn|const|var|comptime|test)\b/);
  return (cut >= 0 ? top.slice(0, cut) : top)
    .split(",")
    .map((s) => s.replace(/=.*$/s, "").trim())
    .filter((s) => s && s !== "_")
    .map(unquote);
}

function flagFields(body: string): { name: string; default: boolean }[] | null {
  const flags: { name: string; default: boolean }[] = [];
  for (const line of topLevel(body).split("\n")) {
    const m = /^\s*(@"[^"]+"|[a-z_][a-z0-9_]*)\s*:\s*([^=,]+?)\s*(?:=\s*([^,]+?))?\s*,\s*$/.exec(line);
    if (!m) continue;
    if (m[2] !== "bool" || (m[3] !== "true" && m[3] !== "false")) return null;
    flags.push({ name: unquote(m[1]), default: m[3] === "true" });
  }
  return flags.length ? flags : null;
}

const zigFields = parseZigFields(zigSrc);

// ---------------------------------------------------------------------------
// Kind mapping

type Kind =
  | "bool" | "int" | "float" | "string" | "enum" | "flags" | "color" | "duration"
  | "repeatable-string" | "palette" | "keybind" | "other";

const DIRECT: Record<string, Kind> = {
  bool: "bool",
  f32: "float",
  f64: "float",
  Color: "color",
  Duration: "duration",
  RepeatableString: "repeatable-string",
  Palette: "palette",
  Keybinds: "keybind",
};

type Resolved = { kind: Kind; optional: boolean; base: string; decl: TypeDecl | null };

function resolve(zigType: string): Resolved {
  const optional = zigType.startsWith("?");
  const base = optional ? zigType.slice(1) : zigType;
  if (DIRECT[base]) return { kind: DIRECT[base], optional, base, decl: null };
  if (/^(?:[ui]\d+|usize|isize|c_int|c_uint)$/.test(base)) return { kind: "int", optional, base, decl: null };
  if (/^\[(?::0)?\](?:const )?u8$/.test(base)) return { kind: "string", optional, base, decl: null };
  const decl = /^[A-Z][A-Za-z0-9_]*$/.test(base) ? findDecl(zigSrc, base) : null;
  if (decl?.kind === "enum") return { kind: "enum", optional, base, decl };
  if (decl?.kind === "packed" && flagFields(decl.body)) return { kind: "flags", optional, base, decl };
  return { kind: "other", optional, base, decl };
}

// Probe candidates: Config.zig enums, plus named types not defined in Config.zig
// (e.g. terminal.CursorStyle, inputpkg.OptionAsAlt), which are usually enums elsewhere.
const resolved = new Map<string, Resolved>();
for (const o of show) {
  const t = zigFields.get(o.key);
  if (t) resolved.set(o.key, resolve(t));
}
const probeKeys = show
  .map((o) => o.key)
  .filter((k) => {
    const r = resolved.get(k);
    if (!r) return false;
    if (r.kind === "enum") return true;
    return r.kind === "other" && r.decl === null && /^[A-Za-z_][\w.]*$/.test(r.base) && !/Repeatable|Path|Command/.test(r.base);
  });

function probeEnums(keys: string[]): Map<string, string[]> {
  const dir = mkdtempSync(join(tmpdir(), "ghostty-schema-"));
  try {
    const file = join(dir, "probe.conf");
    writeFileSync(file, keys.map((k) => `${k} = __invalid__`).join("\n") + "\n");
    const r = spawnSync(ghostty!, ["+validate-config", `--config-file=${file}`], { encoding: "utf8" });
    const values = new Map<string, string[]>();
    const re = /:(\d+):([a-z0-9-]+): invalid value "__invalid__", valid values are: (.*)$/;
    for (const line of (r.stdout + r.stderr).split("\n")) {
      const m = re.exec(line);
      if (!m) continue;
      const [, lineNo, key, list] = m;
      if (keys[Number(lineNo) - 1] !== key) throw new Error(`Probe line mismatch: ${line}`);
      values.set(key, list.split(",").map((s) => s.trim()).filter(Boolean));
    }
    return values;
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

const probed = probeEnums(probeKeys);

// ---------------------------------------------------------------------------
// Doc-derived metadata

function platforms(doc: string): string[] | null {
  // Only unindented prose: notes inside value lists ("`non-native` - (macOS only) ...")
  // describe a single value, not the option.
  const prose = doc
    .split("\n")
    .filter((l) => !/^\s|^[*-] /.test(l))
    .join(" ")
    .replace(/\s+/g, " ");
  const p: string[] = [];
  if (/\bmacOS only\b|only supported (?:currently )?on macOS(?! and Linux)/i.test(prose)) p.push("macos");
  if (/\b(?:GTK|Linux)(?: Wayland)? only\b|only supported (?:currently )?(?:on|in the) (?:GTK|Linux)\b/i.test(prose))
    p.push("linux");
  return p.length ? p : null;
}

function since(doc: string): string | null {
  return /Available since:?\s*(?:Ghostty\s+)?v?(\d+\.\d+\.\d+)/i.exec(doc)?.[1] ?? null;
}

function reload(doc: string): "restart" | "new-terminals" | "runtime" {
  const flat = doc.replace(/\s+/g, " ");
  if (/cannot be reloaded at runtime|requires (?:a )?(?:full )?(?:application )?restart|requires restarting/i.test(flat))
    return "restart";
  if (
    /only (?:affect|affects|apply to|applies to) new|only new terminals will use|not affect existing (?:terminals|surfaces)/i.test(flat)
  )
    return "new-terminals";
  return "runtime";
}

// ---------------------------------------------------------------------------
// Assemble

const warnings: string[] = [];
const REPEATABLE_TYPE = /^\??(?:Repeatable\w*|Palette|Keybinds)$/;

const options = show.map((o) => {
  const zigType = zigFields.get(o.key) ?? null;
  const r = resolved.get(o.key) ?? null;
  let kind: Kind = r?.kind ?? "other";
  let values: string[] | null = probed.get(o.key) ?? null;
  if (values && kind !== "enum") kind = "enum";
  if (kind === "enum" && !values && r?.decl) {
    values = enumMembers(r.decl.body);
    if (!r.optional) warnings.push(`${o.key}: enum ${r.base} not confirmed by probe; using Config.zig members`);
  }
  if (kind === "enum" && r?.decl && values) {
    const zigVals = enumMembers(r.decl.body);
    if (zigVals.join(",") !== values.join(","))
      warnings.push(`${o.key}: probe values [${values}] differ from Config.zig [${zigVals}]`);
  }
  const flags = kind === "flags" && r?.decl ? flagFields(r.decl.body) : null;
  if (flags) {
    const fromShow = o.defaults[0]?.split(",") ?? [];
    const fromZig = flags.map((f) => (f.default ? f.name : `no-${f.name}`));
    if ([...fromShow].sort().join(",") !== [...fromZig].sort().join(","))
      warnings.push(`${o.key}: show-config default "${o.defaults[0]}" differs from Config.zig flags`);
  }
  return {
    key: o.key,
    zigType,
    kind,
    optional: r?.optional ?? false,
    repeatable: o.lines > 1 || (zigType !== null && REPEATABLE_TYPE.test(zigType)),
    defaults: o.defaults,
    doc: o.doc,
    ...(o.docFrom ? { docFrom: o.docFrom } : {}),
    platforms: platforms(o.doc),
    since: since(o.doc),
    reload: reload(o.doc),
    values,
    flags,
  };
});

const showKeys = new Set(show.map((o) => o.key));
const onlyInZig = [...zigFields.keys()].filter((k) => !k.startsWith("_") && !showKeys.has(k));
const onlyInShow = show.map((o) => o.key).filter((k) => !zigFields.has(k));

mkdirSync(dirname(outFile), { recursive: true });
writeFileSync(
  outFile,
  JSON.stringify(
    {
      $comment:
        "GENERATED by tools/ghostty/update-schema.ts. Do not edit; edit schema/ghostty/ghostty-overlay.json instead.",
      ghosttyVersion: version,
      configZigRef,
      options,
    },
    null,
    2,
  ) + "\n",
);

for (const w of warnings) console.warn(`warning: ${w}`);
if (onlyInZig.length) console.warn(`warning: in Config.zig but not +show-config: ${onlyInZig.join(", ")}`);
if (onlyInShow.length) console.warn(`warning: in +show-config but not Config.zig: ${onlyInShow.join(", ")}`);

const counts: Record<string, number> = {};
for (const o of options) counts[o.kind] = (counts[o.kind] ?? 0) + 1;
const kinds = Object.entries(counts)
  .sort((a, b) => b[1] - a[1])
  .map(([k, n]) => `${k}=${n}`)
  .join(" ");
console.log(
  `ghostty ${version} (${configZigRef}): ${options.length} options [${kinds}], ${counts.other ?? 0} unresolved other -> ${outFile.slice(root.length + 1)}`,
);
