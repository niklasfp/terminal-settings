// Support script for .github/workflows/upstream-schema.yml.
//
// Subcommands:
//   node editor/tools/upstream-schema.ts detect          # check Rio, Ghostty, and (weekly) Rio themes against recorded versions
//   node editor/tools/upstream-schema.ts bundle          # diff old vs new schema, extract diagnostics, write PR body + artifact bundle
//   node editor/tools/upstream-schema.ts pr              # commit artifact files via GraphQL (signed) and open/update the draft PR
//   node editor/tools/upstream-schema.ts report-failure  # open or comment on a failure issue when detection or regeneration breaks

import { execFileSync, spawnSync } from "node:child_process";
import { appendFileSync, copyFileSync, existsSync, mkdirSync, readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const editorRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const repoRoot = join(editorRoot, "..");

type TargetId = "rio" | "ghostty" | "rio-themes";

type MatrixEntry = {
  target: TargetId;
  display: string;
  os: "ubuntu-latest" | "macos-latest";
  current: string;
  version: string;
  branch: string;
};

type Manifest = {
  target: TargetId;
  display: string;
  branch: string;
  oldVersion: string;
  newVersion: string;
  prTitle: string;
  commitMessage: string;
  labels: string[];
  testsPassed: boolean;
};

const ALLOWED_FILES: Record<TargetId, string[]> = {
  rio: [
    "editor/schema/rio/rio-source.json",
    "editor/tools/rio/schema-dump/Cargo.toml",
    "editor/tools/rio/schema-dump/Cargo.lock",
  ],
  ghostty: ["editor/schema/ghostty/ghostty-source.json"],
  "rio-themes": ["editor/schema/rio/rio-themes.json"],
};

function gh(args: string[], input?: string): string {
  return execFileSync("gh", args, {
    cwd: repoRoot,
    encoding: "utf8",
    input,
    stdio: [input !== undefined ? "pipe" : "ignore", "pipe", "inherit"],
  }).trim();
}

function ghTry(args: string[]): { ok: boolean; stdout: string } {
  const r = spawnSync("gh", args, { cwd: repoRoot, encoding: "utf8" });
  return { ok: r.status === 0, stdout: (r.stdout ?? "").trim() };
}

function appendSummary(markdown: string): void {
  const summaryPath = process.env.GITHUB_STEP_SUMMARY;
  if (summaryPath) appendFileSync(summaryPath, markdown + "\n");
}

function prTitleFor(target: TargetId, version: string): string {
  switch (target) {
    case "rio":
      return `Update the Rio schema to ${version}`;
    case "ghostty":
      return `Update the Ghostty schema to ${version}`;
    case "rio-themes":
      return `Update the Rio theme catalogue to ${version.slice(0, 7)}`;
  }
}

function openPrMatchesTitle(repo: string, branch: string, expectedTitle: string): boolean {
  const raw = gh([
    "pr",
    "list",
    "--repo",
    repo,
    "--head",
    branch,
    "--state",
    "open",
    "--json",
    "number,title",
  ]);
  const prs = JSON.parse(raw || "[]") as { number: number; title: string }[];
  return prs.some((p) => p.title === expectedTitle);
}

// ---------------------------------------------------------------------------
// 1. detect

function cmdDetect(): void {
  const repo = process.env.GH_REPO;
  if (!repo) throw new Error("GH_REPO is required");

  const forceRio = (process.env.FORCE_RIO ?? "").trim();
  const forceGhostty = (process.env.FORCE_GHOSTTY ?? "").trim();
  const forceThemes = (process.env.FORCE_THEMES ?? "").trim() === "true";
  const anyForced = Boolean(forceRio || forceGhostty || forceThemes);

  const include: MatrixEntry[] = [];
  const rows: string[] = [];

  // Rio
  if (!anyForced || forceRio) {
    const rioSrc = JSON.parse(readFileSync(join(editorRoot, "schema", "rio", "rio-source.json"), "utf8")) as {
      rioVersion?: string;
      rioRevision: string;
    };
    const current = rioSrc.rioVersion ?? rioSrc.rioRevision.slice(0, 7);
    let version: string;
    if (forceRio) {
      version = /^\d+\.\d+\.\d+$/.test(forceRio) ? `v${forceRio}` : forceRio;
    } else {
      const releases = JSON.parse(gh(["api", "repos/raphamorim/rio/releases?per_page=20"])) as {
        tag_name: string;
        draft: boolean;
        prerelease: boolean;
      }[];
      const latest = releases.find((r) => !r.draft && !r.prerelease && /^v\d+\.\d+\.\d+$/.test(r.tag_name));
      if (!latest) throw new Error("No stable vX.Y.Z release found on raphamorim/rio");
      version = latest.tag_name;
    }

    const branch = "schema/rio";
    const expectedTitle = prTitleFor("rio", version);
    if (!forceRio && version === current) {
      rows.push(`| Rio | \`${current}\` | \`${version}\` | Up to date |`);
    } else if (!forceRio && openPrMatchesTitle(repo, branch, expectedTitle)) {
      rows.push(`| Rio | \`${current}\` | \`${version}\` | Open PR already at \`${version}\` |`);
    } else {
      include.push({ target: "rio", display: "Rio", os: "ubuntu-latest", current, version, branch });
      rows.push(`| Rio | \`${current}\` | \`${version}\` | **Queued**${forceRio ? " (forced)" : ""} |`);
    }
  }

  // Ghostty
  if (!anyForced || forceGhostty) {
    const ghosttySrc = JSON.parse(readFileSync(join(editorRoot, "schema", "ghostty", "ghostty-source.json"), "utf8")) as {
      ghosttyVersion: string;
    };
    const current = ghosttySrc.ghosttyVersion;
    let version: string;
    if (forceGhostty) {
      version = forceGhostty.replace(/^v(?=\d)/, "");
    } else {
      const tagLines = gh(["api", "--paginate", "repos/ghostty-org/ghostty/tags", "--jq", ".[].name"])
        .split("\n")
        .map((s) => s.trim())
        .filter(Boolean);
      const parsed = tagLines
        .map((t) => {
          const m = /^v(\d+)\.(\d+)\.(\d+)$/.exec(t);
          return m ? { version: `${m[1]}.${m[2]}.${m[3]}`, parts: [Number(m[1]), Number(m[2]), Number(m[3])] } : null;
        })
        .filter((x): x is { version: string; parts: number[] } => x !== null)
        .sort((a, b) => b.parts[0] - a.parts[0] || b.parts[1] - a.parts[1] || b.parts[2] - a.parts[2]);
      if (!parsed.length) throw new Error("No strict vX.Y.Z tags found on ghostty-org/ghostty");
      version = parsed[0].version;
    }

    const branch = "schema/ghostty";
    const expectedTitle = prTitleFor("ghostty", version);
    if (!forceGhostty && version === current) {
      rows.push(`| Ghostty | \`${current}\` | \`${version}\` | Up to date |`);
    } else if (!forceGhostty && openPrMatchesTitle(repo, branch, expectedTitle)) {
      rows.push(`| Ghostty | \`${current}\` | \`${version}\` | Open PR already at \`${version}\` |`);
    } else {
      include.push({ target: "ghostty", display: "Ghostty", os: "macos-latest", current, version, branch });
      rows.push(`| Ghostty | \`${current}\` | \`${version}\` | **Queued**${forceGhostty ? " (forced)" : ""} |`);
    }
  }

  // Rio themes (weekly on Mondays, or when forced)
  const isMonday = new Date().getUTCDay() === 1;
  if (forceThemes || (!anyForced && isMonday)) {
    const themesSrc = JSON.parse(readFileSync(join(editorRoot, "schema", "rio", "rio-themes.json"), "utf8")) as {
      commit: string;
    };
    const current = themesSrc.commit;
    const version = gh([
      "api",
      "repos/mbadolato/iTerm2-Color-Schemes/commits?path=rio&per_page=1",
      "--jq",
      ".[0].sha",
    ]);
    if (!/^[0-9a-f]{40}$/i.test(version)) {
      throw new Error(`Unexpected commit SHA from mbadolato/iTerm2-Color-Schemes: ${version}`);
    }
    const branch = "schema/rio-themes";
    const expectedTitle = prTitleFor("rio-themes", version);
    if (!forceThemes && version === current) {
      rows.push(`| Rio themes | \`${current.slice(0, 7)}\` | \`${version.slice(0, 7)}\` | Up to date |`);
    } else if (!forceThemes && openPrMatchesTitle(repo, branch, expectedTitle)) {
      rows.push(
        `| Rio themes | \`${current.slice(0, 7)}\` | \`${version.slice(0, 7)}\` | Open PR already at \`${version.slice(0, 7)}\` |`,
      );
    } else {
      include.push({
        target: "rio-themes",
        display: "Rio themes",
        os: "ubuntu-latest",
        current: current.slice(0, 7),
        version,
        branch,
      });
      rows.push(
        `| Rio themes | \`${current.slice(0, 7)}\` | \`${version.slice(0, 7)}\` | **Queued**${forceThemes ? " (forced)" : ""} |`,
      );
    }
  }

  const summary = [
    "### Upstream version check",
    "",
    "| Target | Recorded | Upstream | Action |",
    "|---|---|---|---|",
    ...rows,
  ].join("\n");
  console.log(summary);
  appendSummary(summary);

  const outPath = process.env.GITHUB_OUTPUT;
  if (outPath) {
    appendFileSync(
      outPath,
      `has_updates=${include.length > 0 ? "true" : "false"}\nmatrix=${JSON.stringify({ include })}\n`,
    );
  }
}

// ---------------------------------------------------------------------------
// 2. bundle (diff + diagnostics + PR body + artifact staging)

type DiffRow = { change: "Added" | "Removed" | "Changed"; item: string; before: string; after: string };

function fmtCell(value: unknown): string {
  if (value === undefined) return "—";
  const text = typeof value === "string" ? JSON.stringify(value) : JSON.stringify(value);
  const compact = text.length > 80 ? text.slice(0, 77) + "..." : text;
  return "`" + compact.replace(/\|/g, "\\|").replace(/`/g, "'") + "`";
}

function isMarginLike(o: Record<string, unknown>): boolean {
  return "top" in o && "left" in o && "right" in o && "bottom" in o;
}

function flattenRioDefaults(
  obj: Record<string, unknown>,
  prefix = "",
  out = new Map<string, unknown>(),
): Map<string, unknown> {
  for (const [k, v] of Object.entries(obj)) {
    const key = prefix ? `${prefix}.${k}` : k;
    if (key === "platform") continue;
    if (v !== null && typeof v === "object" && !Array.isArray(v)) {
      const rec = v as Record<string, unknown>;
      if (Object.keys(rec).length > 0 && !isMarginLike(rec)) {
        flattenRioDefaults(rec, key, out);
        continue;
      }
    }
    out.set(key, v);
  }
  return out;
}

function diffRio(beforeRaw: string, afterRaw: string): DiffRow[] {
  type RioSource = {
    defaults: Record<string, unknown>;
    enums: Record<string, { renameAll: string | null; variants: { name: string; aliases: string[]; isDefault: boolean }[] }>;
    colorDefaults: Record<string, string | null>;
  };
  const before = JSON.parse(beforeRaw) as RioSource;
  const after = JSON.parse(afterRaw) as RioSource;
  const rows: DiffRow[] = [];

  const oldDefs = flattenRioDefaults(before.defaults ?? {});
  const newDefs = flattenRioDefaults(after.defaults ?? {});
  for (const [k, v] of Object.entries(before.colorDefaults ?? {})) oldDefs.set(`colors.${k}`, v);
  for (const [k, v] of Object.entries(after.colorDefaults ?? {})) newDefs.set(`colors.${k}`, v);

  const allKeys = [...new Set([...oldDefs.keys(), ...newDefs.keys()])].sort();
  for (const key of allKeys) {
    const hasOld = oldDefs.has(key);
    const hasNew = newDefs.has(key);
    const oldVal = oldDefs.get(key);
    const newVal = newDefs.get(key);
    if (!hasOld && hasNew) {
      rows.push({ change: "Added", item: `\`${key}\``, before: "—", after: fmtCell(newVal) });
    } else if (hasOld && !hasNew) {
      rows.push({ change: "Removed", item: `\`${key}\``, before: fmtCell(oldVal), after: "—" });
    } else if (JSON.stringify(oldVal) !== JSON.stringify(newVal)) {
      rows.push({ change: "Changed", item: `\`${key}\``, before: fmtCell(oldVal), after: fmtCell(newVal) });
    }
  }

  const fmtEnum = (e: RioSource["enums"][string] | undefined) =>
    e ? e.variants.map((v) => v.name).join(", ") : "—";
  const allEnums = [...new Set([...Object.keys(before.enums ?? {}), ...Object.keys(after.enums ?? {})])].sort();
  for (const name of allEnums) {
    const oldE = before.enums?.[name];
    const newE = after.enums?.[name];
    if (!oldE && newE) {
      rows.push({ change: "Added", item: `enum \`${name}\``, before: "—", after: fmtCell(fmtEnum(newE)) });
    } else if (oldE && !newE) {
      rows.push({ change: "Removed", item: `enum \`${name}\``, before: fmtCell(fmtEnum(oldE)), after: "—" });
    } else if (JSON.stringify(oldE) !== JSON.stringify(newE)) {
      rows.push({
        change: "Changed",
        item: `enum \`${name}\``,
        before: fmtCell(fmtEnum(oldE)),
        after: fmtCell(fmtEnum(newE)),
      });
    }
  }

  return rows;
}

function diffGhostty(beforeRaw: string, afterRaw: string): DiffRow[] {
  type GhosttyOption = {
    key: string;
    zigType: string | null;
    kind: string;
    optional: boolean;
    repeatable: boolean;
    defaults: string[];
    values: string[] | null;
    flags: { name: string; default: boolean }[] | null;
  };
  type GhosttySource = { options: GhosttyOption[] };
  const before = JSON.parse(beforeRaw) as GhosttySource;
  const after = JSON.parse(afterRaw) as GhosttySource;

  const oldMap = new Map(before.options.map((o) => [o.key, o]));
  const newMap = new Map(after.options.map((o) => [o.key, o]));
  const allKeys = [...new Set([...oldMap.keys(), ...newMap.keys()])];

  const summarize = (o: GhosttyOption) => ({
    type: `${o.zigType ?? o.kind} (${o.kind}${o.repeatable ? ", repeatable" : ""}${o.optional ? ", optional" : ""})`,
    defaults: o.defaults,
    values: o.values,
    flags: o.flags,
  });

  const rows: DiffRow[] = [];
  for (const key of allKeys) {
    const oldOpt = oldMap.get(key);
    const newOpt = newMap.get(key);
    if (!oldOpt && newOpt) {
      rows.push({
        change: "Added",
        item: `\`${key}\``,
        before: "—",
        after: `${fmtCell(newOpt.zigType ?? newOpt.kind)} (default ${fmtCell(newOpt.defaults)})`,
      });
    } else if (oldOpt && !newOpt) {
      rows.push({
        change: "Removed",
        item: `\`${key}\``,
        before: `${fmtCell(oldOpt.zigType ?? oldOpt.kind)} (default ${fmtCell(oldOpt.defaults)})`,
        after: "—",
      });
    } else if (oldOpt && newOpt) {
      const sOld = summarize(oldOpt);
      const sNew = summarize(newOpt);
      if (JSON.stringify(sOld) !== JSON.stringify(sNew)) {
        const partsBefore: string[] = [];
        const partsAfter: string[] = [];
        if (sOld.type !== sNew.type) {
          partsBefore.push(`type ${fmtCell(sOld.type)}`);
          partsAfter.push(`type ${fmtCell(sNew.type)}`);
        }
        if (JSON.stringify(sOld.defaults) !== JSON.stringify(sNew.defaults)) {
          partsBefore.push(`default ${fmtCell(sOld.defaults)}`);
          partsAfter.push(`default ${fmtCell(sNew.defaults)}`);
        }
        if (JSON.stringify(sOld.values) !== JSON.stringify(sNew.values)) {
          partsBefore.push(`values ${fmtCell(sOld.values)}`);
          partsAfter.push(`values ${fmtCell(sNew.values)}`);
        }
        if (JSON.stringify(sOld.flags) !== JSON.stringify(sNew.flags)) {
          partsBefore.push(`flags ${fmtCell(sOld.flags?.map((f) => f.name))}`);
          partsAfter.push(`flags ${fmtCell(sNew.flags?.map((f) => f.name))}`);
        }
        rows.push({
          change: "Changed",
          item: `\`${key}\``,
          before: partsBefore.join(", "),
          after: partsAfter.join(", "),
        });
      }
    }
  }
  return rows;
}

function diffThemes(beforeRaw: string, afterRaw: string): DiffRow[] {
  type Theme = { name: string; bg: string; fg: string; ansi: (string | null)[] };
  type ThemesDoc = { themes: Theme[] };
  const before = JSON.parse(beforeRaw) as ThemesDoc;
  const after = JSON.parse(afterRaw) as ThemesDoc;
  const oldMap = new Map(before.themes.map((t) => [t.name, t]));
  const newMap = new Map(after.themes.map((t) => [t.name, t]));
  const allNames = [...new Set([...oldMap.keys(), ...newMap.keys()])].sort();

  const rows: DiffRow[] = [];
  for (const name of allNames) {
    const o = oldMap.get(name);
    const n = newMap.get(name);
    if (!o && n) {
      rows.push({ change: "Added", item: `\`${name}\``, before: "—", after: fmtCell(`${n.bg} / ${n.fg}`) });
    } else if (o && !n) {
      rows.push({ change: "Removed", item: `\`${name}\``, before: fmtCell(`${o.bg} / ${o.fg}`), after: "—" });
    } else if (o && n && JSON.stringify(o) !== JSON.stringify(n)) {
      rows.push({
        change: "Changed",
        item: `\`${name}\``,
        before: fmtCell(`${o.bg} / ${o.fg}`),
        after: fmtCell(`${n.bg} / ${n.fg}`),
      });
    }
  }
  return rows;
}

function renderDiffTable(rows: DiffRow[], oldVer: string, newVer: string): string {
  if (rows.length === 0) {
    return `_No settings, enums, or defaults changed between \`${oldVer}\` and \`${newVer}\`._`;
  }
  const lines = [
    "| Change | Item | Before | After |",
    "|---|---|---|---|",
    ...rows.map((r) => `| **${r.change}** | ${r.item} | ${r.before} | ${r.after} |`),
  ];
  return lines.join("\n");
}

function rioManualKeys(): string[] {
  const overlay = JSON.parse(readFileSync(join(editorRoot, "schema", "rio", "rio-overlay.json"), "utf8")) as {
    settings?: { key: string; manual?: boolean }[];
  };
  return (overlay.settings ?? []).filter((s) => s.manual).map((s) => `\`${s.key}\``);
}

function cmdBundle(): void {
  const target = process.env.TARGET as TargetId;
  const oldVersion = process.env.OLD_VERSION ?? "unknown";
  const newVersion = process.env.NEW_VERSION ?? "unknown";
  const beforeFile = process.env.BEFORE_FILE ?? "";
  const testExit = Number(process.env.TEST_EXIT ?? "0");
  const testLogPath = process.env.TEST_LOG ?? "";
  const regenLogPath = process.env.REGEN_LOG ?? "";
  const outDir = process.env.OUT_DIR ?? "";

  if (!target || !(target in ALLOWED_FILES)) throw new Error(`Invalid TARGET: ${target}`);
  if (!beforeFile || !existsSync(beforeFile)) throw new Error(`Missing BEFORE_FILE: ${beforeFile}`);
  if (!outDir) throw new Error("Missing OUT_DIR");

  const afterFile = join(repoRoot, ALLOWED_FILES[target][0]);
  const beforeRaw = readFileSync(beforeFile, "utf8");
  const afterRaw = readFileSync(afterFile, "utf8");

  const rows =
    target === "rio"
      ? diffRio(beforeRaw, afterRaw)
      : target === "ghostty"
        ? diffGhostty(beforeRaw, afterRaw)
        : diffThemes(beforeRaw, afterRaw);

  const testLog = testLogPath && existsSync(testLogPath) ? readFileSync(testLogPath, "utf8") : "";
  const regenLog = regenLogPath && existsSync(regenLogPath) ? readFileSync(regenLogPath, "utf8") : "";

  const diagSet = new Set<string>();
  for (const m of testLog.matchAll(/^(?:warning|error) (?:RIO|GHO)\d{3}: .+$/gm)) {
    diagSet.add(m[0].trim());
  }
  for (const m of regenLog.matchAll(/^warning: .+$/gm)) {
    diagSet.add(m[0].trim());
  }
  const diagnostics = [...diagSet];

  const failedTests = [...testLog.matchAll(/^test (\S+) \.\.\. FAILED$/gm)].map((m) => `\`${m[1]}\``);
  const testsPassed = testExit === 0;

  let headerLinks: string;
  let checklist: string[];
  let display: string;
  let branch: string;
  let labels: string[];

  if (target === "rio") {
    display = "Rio";
    branch = "schema/rio";
    labels = ["dependencies", "rust"];
    const afterJson = JSON.parse(afterRaw) as { rioRevision: string };
    const newRevShort = afterJson.rioRevision.slice(0, 7);
    headerLinks = [
      `Updates the generated Rio schema from \`${oldVersion}\` to [\`${newVersion}\`](https://github.com/raphamorim/rio/releases/tag/${newVersion}) (\`${newRevShort}\`).`,
      "",
      `- **Release notes:** https://github.com/raphamorim/rio/releases/tag/${newVersion}`,
      `- **Upstream diff:** https://github.com/raphamorim/rio/compare/${oldVersion}...${newVersion}`,
    ].join("\n");
    const manualList = rioManualKeys().join(", ");
    checklist = [
      "- [ ] Add or update entries in `editor/schema/rio/rio-overlay.json` for any new or changed settings (labels, groups, widget types, ranges)",
      "- [ ] Remove stale overlay entries for any removed settings",
      `- [ ] Verify \`manual: true\` settings in \`rio-overlay.json\` still match upstream Rio (${manualList})`,
      "- [ ] Retake README screenshots (`node editor/tools/screenshots.ts`) if visible defaults changed",
    ];
  } else if (target === "ghostty") {
    display = "Ghostty";
    branch = "schema/ghostty";
    labels = ["dependencies", "enhancement"];
    const oldTag = oldVersion.startsWith("v") ? oldVersion : `v${oldVersion}`;
    const newTag = newVersion.startsWith("v") ? newVersion : `v${newVersion}`;
    const notesSlug = newVersion.replace(/^v/, "").replace(/\./g, "-");
    headerLinks = [
      `Updates the generated Ghostty schema from \`${oldVersion}\` to [\`${newVersion}\`](https://github.com/ghostty-org/ghostty/tree/${newTag}).`,
      "",
      `- **Release notes:** https://ghostty.org/docs/install/release-notes/${notesSlug}`,
      `- **Upstream diff:** https://github.com/ghostty-org/ghostty/compare/${oldTag}...${newTag}`,
    ].join("\n");
    checklist = [
      "- [ ] Add or update entries in `editor/schema/ghostty/ghostty-overlay.json` (`typeMap`, `rules`, `settings`) for any new options or unmapped Zig types",
      "- [ ] Remove stale overlay entries for any removed options",
      "- [ ] Retake README screenshots (`node editor/tools/screenshots.ts`) if visible defaults changed",
    ];
  } else {
    display = "Rio themes";
    branch = "schema/rio-themes";
    labels = ["dependencies", "enhancement"];
    const newShort = newVersion.slice(0, 7);
    headerLinks = [
      `Updates the Rio theme catalogue (\`editor/schema/rio/rio-themes.json\`) from \`${oldVersion}\` to \`${newShort}\`.`,
      "",
      `- **Upstream diff:** https://github.com/mbadolato/iTerm2-Color-Schemes/compare/${oldVersion}...${newShort}`,
    ].join("\n");
    checklist = ["- [ ] Review added, removed, or updated theme palettes"];
  }

  const diagSection =
    diagnostics.length > 0
      ? diagnostics.map((d) => `- \`${d}\``).join("\n")
      : "_None (`0` errors, `0` warnings)._";

  const crateName = target === "ghostty" ? "ghostty-config" : "rio-config";
  let testSection = testsPassed
    ? `Passing (\`cargo test -p ${crateName} --locked\`).`
    : `**Needs overlay work** — \`cargo test -p ${crateName} --locked\` exited with status ${testExit}${
        failedTests.length ? ` (failed: ${failedTests.join(", ")})` : ""
      }.`;

  if (!testsPassed && testLog.trim()) {
    const tail = testLog.trim().split("\n").slice(-60).join("\n");
    testSection += `\n\n<details>\n<summary>Test output</summary>\n\n\`\`\`text\n${tail}\n\`\`\`\n</details>`;
  }

  const body = [
    headerLinks,
    "",
    "## Settings diff",
    "",
    renderDiffTable(rows, oldVersion, target === "rio-themes" ? newVersion.slice(0, 7) : newVersion),
    "",
    "## Merge diagnostics",
    "",
    diagSection,
    "",
    "## Tests",
    "",
    testSection,
    "",
    "## Reviewer checklist",
    "",
    ...checklist,
  ].join("\n");

  const title = prTitleFor(target, newVersion);
  const manifest: Manifest = {
    target,
    display,
    branch,
    oldVersion,
    newVersion,
    prTitle: title,
    commitMessage: title,
    labels,
    testsPassed,
  };

  mkdirSync(outDir, { recursive: true });
  writeFileSync(join(outDir, "manifest.json"), JSON.stringify(manifest, null, 2) + "\n");
  writeFileSync(join(outDir, "pr-body.md"), body + "\n");

  for (const relPath of ALLOWED_FILES[target]) {
    const srcPath = join(repoRoot, relPath);
    const dstPath = join(outDir, "files", relPath);
    mkdirSync(dirname(dstPath), { recursive: true });
    copyFileSync(srcPath, dstPath);
  }

  appendSummary(`## ${title}\n\n${body}\n`);
  console.log(`Bundled ${target} (${oldVersion} -> ${newVersion}) into ${outDir}`);
}

// ---------------------------------------------------------------------------
// 3. pr (runs in the write-enabled job, never executes upstream code)

function cmdPr(): void {
  const repo = process.env.GH_REPO;
  const baseBranch = process.env.BASE_BRANCH || "main";
  const baseSha = process.env.BASE_SHA;
  const bundlesDir = process.env.BUNDLES_DIR;
  if (!repo || !baseSha || !bundlesDir) {
    throw new Error("GH_REPO, BASE_SHA, and BUNDLES_DIR are required");
  }

  if (!existsSync(bundlesDir)) {
    console.log("No bundles directory found; nothing to propose.");
    return;
  }

  const bundleDirs = existsSync(join(bundlesDir, "manifest.json"))
    ? [bundlesDir]
    : readdirSync(bundlesDir)
        .map((d) => join(bundlesDir, d))
        .filter((d) => statSync(d).isDirectory() && existsSync(join(d, "manifest.json")));

  for (const dir of bundleDirs) {
    const manifest = JSON.parse(readFileSync(join(dir, "manifest.json"), "utf8")) as Manifest;
    const target = manifest.target;
    if (!(target in ALLOWED_FILES)) {
      throw new Error(`Refusing unexpected target in artifact manifest: ${String(target)}`);
    }

    const expectedBranch = target === "rio" ? "schema/rio" : target === "ghostty" ? "schema/ghostty" : "schema/rio-themes";
    const allowedFiles = ALLOWED_FILES[target];
    const additions: { path: string; contents: string }[] = [];
    let anyChanged = false;

    for (const relPath of allowedFiles) {
      const candidatePath = join(dir, "files", relPath);
      if (!existsSync(candidatePath)) {
        throw new Error(`Artifact for ${target} is missing expected file ${relPath}`);
      }
      const bytes = readFileSync(candidatePath);
      if (relPath.endsWith(".json")) {
        JSON.parse(bytes.toString("utf8"));
      }
      const currentBytes = existsSync(join(repoRoot, relPath)) ? readFileSync(join(repoRoot, relPath)) : Buffer.alloc(0);
      if (!bytes.equals(currentBytes)) {
        anyChanged = true;
      }
      additions.push({ path: relPath, contents: bytes.toString("base64") });
    }

    if (!anyChanged) {
      console.log(`No file changes for ${target}; skipping PR.`);
      appendSummary(`- **${manifest.display}**: no file changes after regeneration; skipped PR.`);
      continue;
    }

    // Create or force-reset the branch ref to BASE_SHA so createCommitOnBranch makes a clean 1-commit branch.
    const refCheck = ghTry(["api", `repos/${repo}/git/ref/heads/${expectedBranch}`]);
    if (refCheck.ok) {
      gh([
        "api",
        "--method",
        "PATCH",
        `repos/${repo}/git/refs/heads/${expectedBranch}`,
        "-f",
        `sha=${baseSha}`,
        "-F",
        "force=true",
      ]);
    } else {
      gh([
        "api",
        "--method",
        "POST",
        `repos/${repo}/git/refs`,
        "-f",
        `ref=refs/heads/${expectedBranch}`,
        "-f",
        `sha=${baseSha}`,
      ]);
    }

    // Commit through GitHub's GraphQL API so GitHub signs the commit automatically.
    const graphqlPayload = JSON.stringify({
      query: `
        mutation($input: CreateCommitOnBranchInput!) {
          createCommitOnBranch(input: $input) {
            commit { oid url }
          }
        }
      `,
      variables: {
        input: {
          branch: {
            repositoryNameWithOwner: repo,
            branchName: expectedBranch,
          },
          message: {
            headline: manifest.commitMessage,
          },
          expectedHeadOid: baseSha,
          fileChanges: {
            additions,
          },
        },
      },
    });
    const commitRes = JSON.parse(gh(["api", "graphql", "--input", "-"], graphqlPayload)) as {
      data?: { createCommitOnBranch?: { commit?: { oid: string; url: string } } };
      errors?: { message: string }[];
    };
    if (commitRes.errors?.length) {
      throw new Error(`GraphQL createCommitOnBranch failed: ${JSON.stringify(commitRes.errors)}`);
    }
    const commitOid = commitRes.data?.createCommitOnBranch?.commit?.oid ?? "unknown";
    console.log(`Committed ${commitOid} on ${expectedBranch}`);

    const prBodyPath = join(dir, "pr-body.md");
    const existingRaw = gh([
      "pr",
      "list",
      "--repo",
      repo,
      "--head",
      expectedBranch,
      "--base",
      baseBranch,
      "--state",
      "open",
      "--json",
      "number,url",
    ]);
    const existing = JSON.parse(existingRaw || "[]") as { number: number; url: string }[];

    let prUrl: string;
    if (existing.length > 0) {
      const prNum = existing[0].number;
      gh([
        "pr",
        "edit",
        String(prNum),
        "--repo",
        repo,
        "--title",
        manifest.prTitle,
        "--body-file",
        prBodyPath,
      ]);
      prUrl = existing[0].url;
      console.log(`Updated existing draft PR #${prNum}: ${prUrl}`);
      appendSummary(`- **${manifest.display}**: updated PR [#${prNum}](${prUrl}) (\`${commitOid.slice(0, 7)}\`)`);
    } else {
      const labelArgs = manifest.labels.flatMap((l) => ["--label", l]);
      prUrl = gh([
        "pr",
        "create",
        "--repo",
        repo,
        "--draft",
        "--base",
        baseBranch,
        "--head",
        expectedBranch,
        "--title",
        manifest.prTitle,
        "--body-file",
        prBodyPath,
        ...labelArgs,
      ]);
      console.log(`Opened draft PR: ${prUrl}`);
      appendSummary(`- **${manifest.display}**: opened draft PR ${prUrl} (\`${commitOid.slice(0, 7)}\`)`);
    }
  }
}

// ---------------------------------------------------------------------------
// 4. report-failure

function cmdReportFailure(): void {
  const repo = process.env.GH_REPO;
  const runId = process.env.GITHUB_RUN_ID;
  const serverUrl = process.env.GITHUB_SERVER_URL || "https://github.com";
  const matrixRaw = process.env.MATRIX_JSON || "";
  if (!repo || !runId) throw new Error("GH_REPO and GITHUB_RUN_ID are required");

  const runUrl = `${serverUrl}/${repo}/actions/runs/${runId}`;
  const jobsRes = JSON.parse(gh(["api", `repos/${repo}/actions/runs/${runId}/jobs`])) as {
    jobs: { name: string; conclusion: string | null; html_url: string }[];
  };
  const failedJobs = jobsRes.jobs.filter((j) => j.conclusion === "failure");

  let entries: MatrixEntry[] = [];
  if (matrixRaw) {
    try {
      entries = (JSON.parse(matrixRaw) as { include?: MatrixEntry[] }).include ?? [];
    } catch {
      entries = [];
    }
  }

  type FailureReport = { title: string; body: string };
  const reports: FailureReport[] = [];

  for (const entry of entries) {
    const job = failedJobs.find((j) => j.name.includes(entry.display) && j.name.includes(entry.version));
    if (job) {
      const title = `Upstream schema update failed for ${entry.display} ${entry.version}`;
      const body = [
        `Regenerating the **${entry.display}** schema for \`${entry.version}\` (from \`${entry.current}\`) failed.`,
        "",
        `- **Workflow run:** ${runUrl}`,
        `- **Failed job:** ${job.html_url}`,
      ].join("\n");
      reports.push({ title, body });
    }
  }

  if (reports.length === 0) {
    const jobLinks = failedJobs.map((j) => `- [${j.name}](${j.html_url})`).join("\n");
    const title = "Upstream schema update failed during release detection";
    const body = [
      "The upstream schema workflow failed before regeneration finished.",
      "",
      `- **Workflow run:** ${runUrl}`,
      ...(jobLinks ? ["", "**Failed jobs:**", jobLinks] : []),
    ].join("\n");
    reports.push({ title, body });
  }

  for (const report of reports) {
    const openIssues = JSON.parse(
      gh([
        "issue",
        "list",
        "--repo",
        repo,
        "--state",
        "open",
        "--search",
        `"${report.title}" in:title`,
        "--json",
        "number,title",
      ]) || "[]",
    ) as { number: number; title: string }[];
    const match = openIssues.find((i) => i.title === report.title);
    if (match) {
      gh(["issue", "comment", String(match.number), "--repo", repo, "--body", report.body]);
      console.log(`Commented on existing issue #${match.number}: ${report.title}`);
      appendSummary(`- Commented on issue #${match.number}: **${report.title}**`);
    } else {
      const url = gh([
        "issue",
        "create",
        "--repo",
        repo,
        "--title",
        report.title,
        "--label",
        "ci",
        "--body",
        report.body,
      ]);
      console.log(`Created issue: ${url}`);
      appendSummary(`- Opened issue ${url}: **${report.title}**`);
    }
  }
}

// ---------------------------------------------------------------------------
// Dispatch

const subcommand = process.argv[2];
switch (subcommand) {
  case "detect":
    cmdDetect();
    break;
  case "bundle":
    cmdBundle();
    break;
  case "pr":
    cmdPr();
    break;
  case "report-failure":
    cmdReportFailure();
    break;
  default:
    console.error(`Usage: node editor/tools/upstream-schema.ts <detect|bundle|pr|report-failure>`);
    process.exit(1);
}
