// Writes the app icons of the GPUI editors: brand/rio-settings-icon.svg and
// brand/ghostty-settings-icon.svg. They're our own drawings, not the terminals' logos (which would
// make the editors look like the terminals): each keeps its terminal's colors and one motif, and
// shows settings.
//
//   Rio Settings      Rio's gradient frame and black screen; its two "eyes" became the knobs of
//                     three settings sliders.
//   Ghostty Settings  Ghostty's bezel and blue screen; the glowing ghost wears a gear instead of
//                     a prompt.
//
//   node tools/gpui/make-icons.ts
//
// 1024×1024 on Apple's icon grid (an 824 px rounded square). Plain shapes and gradients only, so
// macOS's own SVG renderer (sips) draws them; tools/gpui/package-macos.ts turns them into .icns.

import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const brand = join(dirname(fileURLToPath(import.meta.url)), "..", "..", "brand");
const r1 = (n: number) => Math.round(n * 10) / 10;

/** A gear outline: `teeth` trapezoid teeth between radius `root` and `tip`. */
function gear(cx: number, cy: number, tip: number, root: number, teeth: number): string {
  const step = (2 * Math.PI) / teeth;
  const at = (r: number, a: number) => `${r1(cx + r * Math.cos(a))} ${r1(cy + r * Math.sin(a))}`;
  const parts: string[] = [];
  for (let i = 0; i < teeth; i++) {
    const a = i * step - Math.PI / 2;
    const base = step * 0.27; // half the tooth width at the root
    const top = step * 0.17; // half the tooth width at the tip
    parts.push(`${i === 0 ? "M" : "L"} ${at(root, a - step / 2 + 0.001)}`);
    parts.push(`A ${root} ${root} 0 0 1 ${at(root, a - base)}`);
    parts.push(`L ${at(tip, a - top)}`, `A ${tip} ${tip} 0 0 1 ${at(tip, a + top)}`, `L ${at(root, a + base)}`);
    parts.push(`A ${root} ${root} 0 0 1 ${at(root, a + step / 2)}`);
  }
  return parts.join(" ") + " Z";
}

/** A circle with an evenodd hole, as one path. */
const ring = (cx: number, cy: number, r: number) =>
  `M ${cx - r} ${cy} a ${r} ${r} 0 1 0 ${2 * r} 0 a ${r} ${r} 0 1 0 ${-2 * r} 0 Z`;

/** Rio's eye: a disc with a wedge cut out, opening up and to the right. */
function eye(cx: number, cy: number, r: number): string {
  const a1 = -Math.PI / 4.2; // upper edge of the cut
  const a2 = Math.PI / 9; // lower edge of the cut
  const p = (a: number) => `${r1(cx + r * Math.cos(a))} ${r1(cy + r * Math.sin(a))}`;
  return `M ${cx} ${cy} L ${p(a2)} A ${r} ${r} 0 1 1 ${p(a1)} Z`;
}

function rio(): string {
  const rows = [
    { y: 352, knob: 640, from: "#fb43fe", to: "#cf91f6" },
    { y: 512, knob: 420, from: "#cf91f6", to: "#73c9ea" },
    { y: 672, knob: 572, from: "#73c9ea", to: "#00dbe5" },
  ];
  const left = 262;
  const right = 762;
  const sliders = rows
    .map(
      (r, i) => `
  <line x1="${r.knob + 84}" y1="${r.y}" x2="${right}" y2="${r.y}" stroke="#3f3f46" stroke-width="30" stroke-linecap="round"/>
  <line x1="${left}" y1="${r.y}" x2="${r.knob - 84}" y2="${r.y}" stroke="url(#fill${i})" stroke-width="30" stroke-linecap="round"/>
  <path d="${eye(r.knob, r.y, 66)}" fill="#fefefe"/>`,
    )
    .join("");
  const fills = rows
    .map(
      (r, i) => `
    <linearGradient id="fill${i}" x1="${left}" x2="${r.knob - 84}" y1="0" y2="0" gradientUnits="userSpaceOnUse">
      <stop stop-color="${r.from}"/><stop offset="1" stop-color="${r.to}"/>
    </linearGradient>`,
    )
    .join("");
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024" width="1024" height="1024">
  <defs>
    <linearGradient id="frame" x1="512" x2="512" y1="100" y2="924" gradientUnits="userSpaceOnUse">
      <stop stop-color="#fb43fe"/>
      <stop offset=".484" stop-color="#cf91f6"/>
      <stop offset=".813" stop-color="#73c9ea"/>
      <stop offset="1" stop-color="#00dbe5"/>
    </linearGradient>${fills}
  </defs>
  <rect x="100" y="100" width="824" height="824" rx="185" fill="url(#frame)"/>
  <rect x="174" y="174" width="676" height="676" rx="128" fill="#000"/>${sliders}
</svg>
`;
}

function ghostty(): string {
  // The ghost: a round head, straight sides and a wavy hem, centred on the screen.
  const cx = 512;
  const w = 380;
  const head = 330;
  const hem = 700;
  const x0 = cx - w / 2;
  const x1 = cx + w / 2;
  const waves = 4;
  const ww = w / waves;
  let hemPath = "";
  for (let i = 0; i < waves; i++) {
    const xa = x1 - i * ww;
    hemPath += ` Q ${r1(xa - ww * 0.25)} ${hem + 46} ${r1(xa - ww * 0.5)} ${hem} Q ${r1(xa - ww * 0.75)} ${hem - 46} ${r1(xa - ww)} ${hem}`;
  }
  const ghost = `M ${x0} ${hem} L ${x0} ${head + w / 2} A ${w / 2} ${w / 2} 0 0 1 ${x1} ${head + w / 2} L ${x1} ${hem}${hemPath} Z`;
  const glow = [44, 30, 18]
    .map((s, i) => `<path d="${ghost}" fill="none" stroke="#7d93ff" stroke-width="${s}" stroke-opacity="${[0.12, 0.18, 0.28][i]}" stroke-linejoin="round"/>`)
    .join("\n  ");
  const gy = 548;
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024" width="1024" height="1024">
  <defs>
    <linearGradient id="bezel" x1="512" x2="512" y1="100" y2="924" gradientUnits="userSpaceOnUse">
      <stop stop-color="#f4f4f6"/><stop offset=".5" stop-color="#b9bcc4"/><stop offset="1" stop-color="#6e717a"/>
    </linearGradient>
    <linearGradient id="screen" x1="512" x2="512" y1="168" y2="856" gradientUnits="userSpaceOnUse">
      <stop stop-color="#3551f3"/><stop offset=".55" stop-color="#2236b8"/><stop offset="1" stop-color="#101a6e"/>
    </linearGradient>
    <linearGradient id="body" x1="512" x2="512" y1="${head}" y2="${hem}" gradientUnits="userSpaceOnUse">
      <stop stop-color="#ffffff"/><stop offset="1" stop-color="#cdd7ff"/>
    </linearGradient>
  </defs>
  <rect x="100" y="100" width="824" height="824" rx="185" fill="url(#bezel)"/>
  <rect x="160" y="160" width="704" height="704" rx="140" fill="#1b1d24"/>
  <rect x="176" y="176" width="672" height="672" rx="126" fill="url(#screen)"/>
  ${glow}
  <path d="${ghost}" fill="url(#body)"/>
  <path d="${gear(cx, gy, 124, 96, 8)} ${ring(cx, gy, 44)}" fill="#3551f3" fill-rule="evenodd"/>
</svg>
`;
}

/** The single app that edits both terminals: Rio's pink running into Ghostty's blue, a gear and a prompt. */
function both(): string {
  const cx = 512;
  const cy = 500;
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024" width="1024" height="1024">
  <defs>
    <linearGradient id="frame" x1="100" x2="924" y1="100" y2="924" gradientUnits="userSpaceOnUse">
      <stop stop-color="#fb43fe"/><stop offset=".45" stop-color="#cf91f6"/><stop offset="1" stop-color="#3551f3"/>
    </linearGradient>
  </defs>
  <rect x="100" y="100" width="824" height="824" rx="185" fill="url(#frame)"/>
  <rect x="174" y="174" width="676" height="676" rx="128" fill="#0b0b10"/>
  <path d="${gear(cx, cy, 210, 168, 10)} ${ring(cx, cy, 104)}" fill="#fefefe" fill-rule="evenodd"/>
  <path d="M 452 452 L 500 500 L 452 548" fill="none" stroke="url(#frame)" stroke-width="26" stroke-linecap="round" stroke-linejoin="round"/>
  <line x1="516" y1="552" x2="572" y2="552" stroke="url(#frame)" stroke-width="26" stroke-linecap="round"/>
</svg>
`;
}

/**
 * A Windows .ico with PNG images at the usual sizes, rendered from an SVG with macOS's sips. The
 * .ico files are committed, so building on Windows needs no image tools (see apps/<app>/build.rs).
 */
function ico(svg: string, out: string): void {
  const work = mkdtempSync(join(tmpdir(), "ico-"));
  const sizes = [16, 20, 24, 32, 40, 48, 64, 128, 256];
  const images = sizes.map((px) => {
    const png = join(work, `${px}.png`);
    execFileSync("sips", ["-s", "format", "png", "-z", String(px), String(px), svg, "--out", png], { stdio: "ignore" });
    return readFileSync(png);
  });
  rmSync(work, { recursive: true, force: true });
  // ICONDIR (6 bytes), one ICONDIRENTRY (16 bytes) per image, then the PNG data.
  const header = Buffer.alloc(6 + 16 * images.length);
  header.writeUInt16LE(0, 0); // reserved
  header.writeUInt16LE(1, 2); // 1 = icon
  header.writeUInt16LE(images.length, 4);
  let offset = header.length;
  images.forEach((png, i) => {
    const entry = 6 + 16 * i;
    const px = sizes[i];
    header.writeUInt8(px >= 256 ? 0 : px, entry); // 0 means 256
    header.writeUInt8(px >= 256 ? 0 : px, entry + 1);
    header.writeUInt8(0, entry + 2); // no palette
    header.writeUInt8(0, entry + 3); // reserved
    header.writeUInt16LE(1, entry + 4); // colour planes
    header.writeUInt16LE(32, entry + 6); // bits per pixel
    header.writeUInt32LE(png.length, entry + 8);
    header.writeUInt32LE(offset, entry + 12);
    offset += png.length;
  });
  writeFileSync(out, Buffer.concat([header, ...images]));
}

writeFileSync(join(brand, "rio-settings-icon.svg"), rio());
writeFileSync(join(brand, "ghostty-settings-icon.svg"), ghostty());
writeFileSync(join(brand, "terminal-settings-icon.svg"), both());
ico(join(brand, "rio-settings-icon.svg"), join(brand, "rio-settings-icon.ico"));
ico(join(brand, "ghostty-settings-icon.svg"), join(brand, "ghostty-settings-icon.ico"));
console.log("wrote brand/{rio,ghostty,terminal}-settings-icon.svg and {rio,ghostty}-settings-icon.ico");
