// Builds the GPUI editors with the `dist` profile (release + whole-program optimisation, stripped;
// see gpui/Cargo.toml) and wraps each in a macOS .app bundle, so they open from Finder like any
// app (a bare executable opens in a Terminal window instead), then zips them.
//
//   node tools/gpui/package-macos.ts [--app rio-settings|ghostty-settings]
//       → gpui/target/macos/Rio Settings.app, Ghostty Settings.app, terminal-settings-gpui-macos-<arch>.zip
//         (with --app only that one, zipped as <app>-macos-<arch>.zip)
//
//   node tools/gpui/package-macos.ts --exe <path> --name <app name> --id <bundle id> --icon <png or svg>
//       [--version <x.y.z>] [--out <dir>]
//       → wraps one other executable the same way (e.g. a Tauri build without its bundler)
//
// Bundles are ad-hoc signed only (not notarized). Copied to another Mac, Gatekeeper blocks the
// first launch until `xattr -dr com.apple.quarantine <App>.app`, or System Settings → Privacy &
// Security → Open Anyway (right-click → Open no longer works on macOS 15 and later).
//
// Requires cargo and the macOS tools sips, iconutil, codesign and ditto.

import { execFileSync } from "node:child_process";
import { chmodSync, copyFileSync, existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir, arch } from "node:os";
import { basename, dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const editor = join(dirname(fileURLToPath(import.meta.url)), "..", "..");
const workspace = join(editor, "gpui");

type App = {
  exe: string; // path to the built executable
  name: string; // shown in Finder, the Dock and the menu bar
  id: string; // CFBundleIdentifier
  icon: string; // a square PNG or SVG
  version: string;
};

const run = (cmd: string, args: string[], cwd?: string) =>
  execFileSync(cmd, args, { cwd, stdio: ["ignore", "pipe", "inherit"], encoding: "utf8" });

const xml = (s: string) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");

function infoPlist(app: App, executable: string): string {
  const entries: [string, string][] = [
    ["CFBundleName", app.name],
    ["CFBundleDisplayName", app.name],
    ["CFBundleIdentifier", app.id],
    ["CFBundleExecutable", executable],
    ["CFBundleIconFile", "AppIcon"],
    ["CFBundlePackageType", "APPL"],
    ["CFBundleShortVersionString", app.version],
    ["CFBundleVersion", app.version],
    ["CFBundleInfoDictionaryVersion", "6.0"],
    ["LSMinimumSystemVersion", "11.0"],
    ["LSApplicationCategoryType", "public.app-category.developer-tools"],
  ];
  const body = entries.map(([k, v]) => `  <key>${k}</key>\n  <string>${xml(v)}</string>`).join("\n");
  return `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
${body}
  <key>NSHighResolutionCapable</key>
  <true/>
</dict>
</plist>
`;
}

/** An .icns with every standard size, rendered from one square PNG or SVG with sips. */
function icns(source: string, out: string): void {
  const work = mkdtempSync(join(tmpdir(), "icon-"));
  const set = join(work, "AppIcon.iconset");
  mkdirSync(set);
  for (const size of [16, 32, 128, 256, 512]) {
    for (const scale of [1, 2]) {
      const px = String(size * scale);
      const name = scale === 1 ? `icon_${size}x${size}.png` : `icon_${size}x${size}@2x.png`;
      run("sips", ["-s", "format", "png", "-z", px, px, source, "--out", join(set, name)]);
    }
  }
  run("iconutil", ["-c", "icns", set, "-o", out]);
  rmSync(work, { recursive: true, force: true });
}

function bundle(app: App, outDir: string): string {
  const dir = join(outDir, `${app.name}.app`);
  rmSync(dir, { recursive: true, force: true });
  mkdirSync(join(dir, "Contents", "MacOS"), { recursive: true });
  mkdirSync(join(dir, "Contents", "Resources"), { recursive: true });
  const executable = basename(app.exe);
  copyFileSync(app.exe, join(dir, "Contents", "MacOS", executable));
  chmodSync(join(dir, "Contents", "MacOS", executable), 0o755);
  writeFileSync(join(dir, "Contents", "Info.plist"), infoPlist(app, executable));
  icns(app.icon, join(dir, "Contents", "Resources", "AppIcon.icns"));
  // Seals the bundle (the linker's signature covers only the bare executable).
  run("codesign", ["--force", "--sign", "-", "--timestamp=none", dir]);
  return dir;
}

function zip(bundles: string[], out: string): void {
  // ditto keeps the bundle structure and signature intact, unlike plain zip.
  rmSync(out, { force: true });
  const staging = mkdtempSync(join(tmpdir(), "zip-"));
  for (const b of bundles) run("ditto", [b, join(staging, basename(b))]);
  run("ditto", ["-c", "-k", "--norsrc", staging, out]);
  rmSync(staging, { recursive: true, force: true });
}

function arg(name: string): string | undefined {
  const i = process.argv.indexOf(name);
  return i >= 0 ? process.argv[i + 1] : undefined;
}

const machine = arch() === "arm64" ? "arm64" : arch();

if (arg("--exe")) {
  const need = (name: string) => arg(name) ?? (console.error(`${name} is required with --exe`), process.exit(2));
  const out = resolve(arg("--out") ?? ".");
  mkdirSync(out, { recursive: true });
  const app: App = {
    exe: resolve(need("--exe")),
    name: need("--name"),
    id: need("--id"),
    icon: resolve(need("--icon")),
    version: arg("--version") ?? "0.1.0",
  };
  const b = bundle(app, out);
  const z = join(out, `${basename(app.exe)}-macos-${machine}.zip`);
  zip([b], z);
  console.log(`${b}\n${z}`);
} else {
  // --app rio-settings|ghostty-settings: build and bundle just that one (the release workflow).
  const only = arg("--app");
  const packages = only ? [only] : ["rio-settings", "ghostty-settings"];
  run("cargo", ["build", "--locked", "--profile", "dist", ...packages.flatMap((p) => ["-p", p])], workspace);
  const meta = JSON.parse(run("cargo", ["metadata", "--no-deps", "--format-version", "1"], workspace));
  // The release workflow stamps the tag's version into the binaries with SETTINGS_VERSION; match it.
  const version = (pkg: string) =>
    process.env.SETTINGS_VERSION || (meta.packages.find((p: { name: string }) => p.name === pkg)?.version ?? "0.1.0");
  const release = join(workspace, "target", "dist"); // where cargo puts `--profile dist` builds
  const out = join(workspace, "target", "macos");
  mkdirSync(out, { recursive: true });
  const all: App[] = [
    {
      exe: join(release, "rio-settings"),
      name: "Rio Settings",
      id: "dev.niklasfp.rio-settings",
      icon: join(editor, "brand", "rio-settings-icon.svg"),
      version: version("rio-settings"),
    },
    {
      exe: join(release, "ghostty-settings"),
      name: "Ghostty Settings",
      id: "dev.niklasfp.ghostty-settings",
      icon: join(editor, "brand", "ghostty-settings-icon.svg"),
      version: version("ghostty-settings"),
    },
  ];
  const apps = all.filter((a) => packages.includes(basename(a.exe)));
  if (apps.length === 0) throw new Error(`Unknown --app '${only}': use rio-settings or ghostty-settings`);
  for (const a of apps) if (!existsSync(a.exe)) throw new Error(`Missing build output: ${a.exe}`);
  const bundles = apps.map((a) => bundle(a, out));
  const z = join(out, only ? `${only}-macos-${machine}.zip` : `terminal-settings-gpui-macos-${machine}.zip`);
  zip(bundles, z);
  console.log([...bundles, z].join("\n"));
}
