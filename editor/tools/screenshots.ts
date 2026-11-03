// Renders the README screenshots: both native GPUI editors against demo configs, in light and dark,
// at 2x Retina on macOS, plus the composite hero and social-preview images.
//
//   node tools/screenshots.ts            # builds the GPUI binaries and writes ../docs/images/*.png
//
// Needs macOS (cargo, swift, screencapture). Uses throwaway demo configs under a temporary HOME
// so it never touches your real config files or appearance preference.

import { execFileSync, spawn, type ChildProcess } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const workspace = join(root, "gpui");
const outDir = join(root, "..", "docs", "images");
mkdirSync(outDir, { recursive: true });

const work = mkdtempSync(join(tmpdir(), "settings-shots-"));
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
const children: ChildProcess[] = [];

execFileSync("cargo", ["build", "--locked", "--bins"], { cwd: workspace, stdio: "inherit" });
const binDir = join(workspace, "target", "debug");

type Point = [number, number];

type Shot = {
  name: string;
  app: "rio-settings" | "ghostty-settings";
  config: string;
  scheme: "light" | "dark";
  clicks: Point[]; // relative to the top-left of the 1180x853 window
  waitAfterLastClickMs?: number;
};

function runSwift(code: string): string {
  return execFileSync("swift", ["-e", code], { encoding: "utf8" }).trim();
}

async function capture(shot: Shot): Promise<void> {
  const pref = join(work, ".config", shot.app, "appearance");
  mkdirSync(dirname(pref), { recursive: true });
  writeFileSync(pref, `${shot.scheme}\n`);

  const child = spawn(join(binDir, shot.app), ["--config", shot.config], {
    env: { ...process.env, HOME: work, XDG_STATE_HOME: "", XDG_CONFIG_HOME: "" },
    stdio: "ignore",
  });
  children.push(child);

  try {
    // Wait for the window to appear and fonts/themes lists to settle.
    await sleep(2200);
    const clickList = shot.clicks.map(([dx, dy]) => `(${dx}, ${dy})`).join(", ");
    const wid = runSwift(`
      import AppKit
      import CoreGraphics
      import Foundation

      let pid: pid_t = ${child.pid}
      NSRunningApplication(processIdentifier: pid)?.activate()
      usleep(250_000)

      let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]] ?? []
      guard let win = list.first(where: {
        ($0[kCGWindowOwnerPID as String] as? Int) == Int(pid) &&
        ($0[kCGWindowLayer as String] as? Int) == 0
      }),
      let wid = win[kCGWindowNumber as String] as? Int,
      let bounds = win[kCGWindowBounds as String] as? [String: Double],
      let wx = bounds["X"], let wy = bounds["Y"] else {
        fputs("window not found for pid \\(pid)\\n", stderr)
        exit(1)
      }

      func click(_ dx: Double, _ dy: Double) {
        let pt = CGPoint(x: wx + dx, y: wy + dy)
        CGEvent(mouseEventSource: nil, mouseType: .leftMouseDown, mouseCursorPosition: pt, mouseButton: .left)?.post(tap: .cghidEventTap)
        usleep(50_000)
        CGEvent(mouseEventSource: nil, mouseType: .leftMouseUp, mouseCursorPosition: pt, mouseButton: .left)?.post(tap: .cghidEventTap)
        usleep(350_000)
      }

      let clicks: [(Double, Double)] = [${clickList}]
      for (dx, dy) in clicks {
        click(dx, dy)
      }
      print(wid)
    `);

    await sleep(shot.waitAfterLastClickMs ?? 500);
    const outPath = join(outDir, `${shot.name}.png`);
    execFileSync("screencapture", ["-l", wid, "-o", "-x", outPath]);
    console.log("wrote", `${shot.name}.png`);
  } finally {
    child.kill();
    await sleep(300);
  }
}

function renderHtml(htmlPath: string, outPath: string, width: number, height: number, scale = 1): void {
  runSwift(`
    import AppKit
    import WebKit

    let app = NSApplication.shared
    let webView = WKWebView(frame: NSRect(x: 0, y: 0, width: ${width}, height: ${height}))
    let url = URL(fileURLWithPath: ${JSON.stringify(htmlPath)})
    webView.loadFileURL(url, allowingReadAccessTo: URL(fileURLWithPath: "/"))

    let deadline = Date().addingTimeInterval(2.0)
    while Date() < deadline {
      RunLoop.current.run(mode: .default, before: Date().addingTimeInterval(0.05))
    }

    var done = false
    let config = WKSnapshotConfiguration()
    config.rect = CGRect(x: 0, y: 0, width: ${width}, height: ${height})
    config.snapshotWidth = NSNumber(value: ${Math.round((width * scale) / 2)})
    webView.takeSnapshot(with: config) { image, error in
      guard let image = image,
            let tiff = image.tiffRepresentation,
            let rep = NSBitmapImageRep(data: tiff),
            let png = rep.representation(using: .png, properties: [:]) else {
        fputs("snapshot failed: \\(String(describing: error))\\n", stderr)
        exit(1)
      }
      try! png.write(to: URL(fileURLWithPath: ${JSON.stringify(outPath)}))
      done = true
    }
    while !done {
      RunLoop.current.run(mode: .default, before: Date().addingTimeInterval(0.05))
    }
  `);
}

const rioConfig = join(work, ".config", "rio", "config.toml");
const ghosttyConfig = join(work, ".config", "ghostty", "config");
mkdirSync(dirname(rioConfig), { recursive: true });
mkdirSync(dirname(ghosttyConfig), { recursive: true });
writeFileSync(
  rioConfig,
  `# My Rio setup\n\n[window]\nopacity = 0.95\nblur = true\n\n[fonts]\nsize = 15.0\n\n[colors]\nbackground = "#1e1e2e"\nforeground = "#cdd6f4"\ncursor = "#f5e0dc"\n`,
);
writeFileSync(
  ghosttyConfig,
  `# My Ghostty setup\nfont-size = 14\ncursor-style = bar\nwindow-padding-x = 8\nshell-integration-features = cursor,title,ssh-env,path\n`,
);

// Sidebar item i (0-indexed) sits at (90, 158 + i * 36).
const sidebarItem = (index: number): Point => [90, 158 + index * 36];

try {
  const shots: Shot[] = [
    {
      name: "rio-dark",
      app: "rio-settings",
      config: rioConfig,
      scheme: "dark",
      // Group 4: Colors (with live terminal preview).
      clicks: [sidebarItem(4)],
    },
    {
      name: "rio-light",
      app: "rio-settings",
      config: rioConfig,
      scheme: "light",
      // Group 1: Window.
      clicks: [sidebarItem(1)],
    },
    {
      name: "ghostty-light",
      app: "ghostty-settings",
      config: ghosttyConfig,
      scheme: "light",
      // Group 2: Theme & colors, then open the Theme dropdown.
      clicks: [sidebarItem(2), [800, 266]],
    },
    {
      name: "ghostty-dark",
      app: "ghostty-settings",
      config: ghosttyConfig,
      scheme: "dark",
      // Group 5: Cursor, set cursor-opacity on the slider, then click Save to open the diff dialog.
      clicks: [sidebarItem(5), [835, 391], [1105, 65]],
      waitAfterLastClickMs: 1100,
    },
  ];

  for (const s of shots) {
    await capture(s);
  }

  // Hero: two native windows overlapping on a radial gradient, rendered at 2x (3200x1800).
  const heroHtml = join(work, "hero.html");
  writeFileSync(
    heroHtml,
    `<!doctype html><html><body style="margin:0;width:1600px;height:900px;overflow:hidden;
    background: radial-gradient(circle at 15% 20%, #e879f9 0, transparent 45%), radial-gradient(circle at 85% 80%, #3551f3 0, transparent 45%), #0f0f11;
    font-family: -apple-system, system-ui, sans-serif;">
    <img src="file://${join(outDir, "rio-dark.png")}" style="position:absolute;left:70px;top:150px;width:900px;border-radius:14px;box-shadow:0 30px 80px #000a;transform:rotate(-2.5deg)">
    <img src="file://${join(outDir, "ghostty-light.png")}" style="position:absolute;right:70px;top:120px;width:900px;border-radius:14px;box-shadow:0 30px 80px #000a;transform:rotate(2deg)">
    <div style="position:absolute;left:0;right:0;top:36px;text-align:center;color:#fff;font-size:44px;font-weight:700;letter-spacing:-.02em;text-shadow:0 2px 20px #0008">
      Settings for Rio &amp; Ghostty</div></body></html>`,
  );
  renderHtml(heroHtml, join(outDir, "hero.png"), 1600, 900, 2);
  console.log("wrote hero.png");

  // Social preview: 1280x640 repository card with native windows.
  const socialHtml = join(work, "social-preview.html");
  const brandDir = join(root, "brand");
  writeFileSync(
    socialHtml,
    `<!doctype html><html><body style="margin:0;width:1280px;height:640px;overflow:hidden;
    background: radial-gradient(circle at 15% 20%, #e879f933 0, transparent 45%), radial-gradient(circle at 85% 80%, #3551f344 0, transparent 50%), #0f0f11;
    font-family: -apple-system, system-ui, sans-serif;color:#f4f4f5;">
    <div style="position:absolute;left:64px;top:120px;width:500px;">
      <div style="display:flex;gap:16px;align-items:center;margin-bottom:24px;">
        <img src="file://${join(brandDir, "rio-settings-icon.svg")}" style="width:56px;height:56px;border-radius:12px;">
        <img src="file://${join(brandDir, "ghostty-settings-icon.svg")}" style="width:56px;height:56px;border-radius:12px;">
      </div>
      <div style="font-size:46px;font-weight:800;letter-spacing:-.03em;margin-bottom:16px;">terminal-settings</div>
      <div style="font-size:28px;font-weight:600;line-height:1.25;color:#e4e4e7;margin-bottom:28px;">A real settings window for<br>Rio and Ghostty.</div>
      <div style="font-size:18px;line-height:1.6;color:#a1a1aa;">
        <div>Native desktop apps &bull; macOS, Linux &amp; Windows</div>
        <div>Saves straight to your config file</div>
      </div>
    </div>
    <img src="file://${join(outDir, "rio-dark.png")}" style="position:absolute;left:560px;top:52px;width:680px;border-radius:12px;box-shadow:0 24px 60px #000b;transform:rotate(-2deg);">
    <img src="file://${join(outDir, "ghostty-light.png")}" style="position:absolute;left:660px;top:210px;width:680px;border-radius:12px;box-shadow:0 24px 60px #000b;transform:rotate(1.5deg);">
    </body></html>`,
  );
  renderHtml(socialHtml, join(outDir, "social-preview.png"), 1280, 640, 1);
  console.log("wrote social-preview.png");
} finally {
  for (const c of children) c.kill();
  rmSync(work, { recursive: true, force: true, maxRetries: 5, retryDelay: 300 });
}
