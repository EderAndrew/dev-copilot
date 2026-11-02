#!/usr/bin/env node
// Build matrix of the release workflow, narrowed to the platforms a release declares
// (specs/002 FR-032, tasks.md "Scope decision for 0.1.0").
//
//   node tooling/release/release-platforms.mjs [--select "<ids>"]
//
// Prints `{"include":[…]}` for `strategy.matrix`. `--select` takes the repository variable
// `RELEASE_PLATFORMS`: target ids (`macos-aarch64`, `macos-x64`, `windows-x64`, `linux-amd64`)
// or platform names (`macos`), separated by commas or whitespace. Empty or absent selects
// every target. An unknown name fails, so a typo never ships a smaller release unnoticed.
// A platform left out is not built, so it needs none of its signing credentials, and
// make-manifest.mjs leaves it out of latest.json ("no update" for that platform).

import { parseArgs } from "node:util";
import { pathToFileURL } from "node:url";

/** Every target the pipeline can build, in canonical order. */
export const TARGETS = [
  { os: "macos-15", platform: "macos", target: "aarch64-apple-darwin", arch: "aarch64", bundles: "app,dmg" },
  { os: "macos-15", platform: "macos", target: "x86_64-apple-darwin", arch: "x64", bundles: "app,dmg" },
  { os: "windows-latest", platform: "windows", target: "x86_64-pc-windows-msvc", arch: "x64", bundles: "nsis" },
  { os: "ubuntu-22.04", platform: "linux", target: "x86_64-unknown-linux-gnu", arch: "amd64", bundles: "deb,appimage" },
].map((t) => ({ id: `${t.platform}-${t.arch}`, ...t }));

/** Targets named by `spec` (see the header); every target when `spec` is blank. */
export function selectTargets(spec) {
  if (spec === undefined || spec.trim() === "") {
    return [...TARGETS];
  }
  const names = spec
    .split(/[\s,]+/)
    .map((name) => name.trim().toLowerCase())
    .filter(Boolean);
  if (names.length === 0) {
    throw new Error(`"${spec}" selects no platform`);
  }
  const wanted = new Set();
  for (const name of names) {
    const matches = TARGETS.filter((t) => t.id === name || t.platform === name);
    if (matches.length === 0) {
      const known = TARGETS.map((t) => t.id).join(", ");
      throw new Error(`unknown release platform "${name}" (known: ${known})`);
    }
    for (const t of matches) {
      wanted.add(t.id);
    }
  }
  return TARGETS.filter((t) => wanted.has(t.id));
}

function main() {
  const { values } = parseArgs({ options: { select: { type: "string" } }, strict: true });
  const targets = selectTargets(values.select);
  console.error(`release platforms: ${targets.map((t) => t.id).join(", ")}`);
  console.log(JSON.stringify({ include: targets }));
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  try {
    main();
  } catch (error) {
    console.error(`release-platforms: ${error.message}`);
    process.exit(1);
  }
}
