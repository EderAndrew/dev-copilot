#!/usr/bin/env node
// Builds `latest.json` and `SHA256SUMS` for a release (specs/002 T052,
// contracts/release-artifacts.md).
//
//   node tooling/release/make-manifest.mjs --dir <assets> --version 0.1.1 --tag v0.1.1 \
//     --repo EderAndrew/dev-copilot [--notes-file notes.txt] [--pub-date 2026-12-01T12:00:00Z]
//   node tooling/release/make-manifest.mjs --dir <assets> --check
//
// `<assets>` holds the release assets under their canonical names (collect-bundle.mjs). The
// updater manifest lists each platform whose package has a non-empty `.sig`; a platform without
// one is omitted, which the app reads as "no update". `SHA256SUMS` covers every asset except
// `SHA256SUMS*`, `latest.json` included. `--check` re-verifies an existing `SHA256SUMS`, for
// example after assets were removed from a draft (then regenerate instead).

import { createHash } from "node:crypto";
import { existsSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { parseArgs } from "node:util";
import { pathToFileURL } from "node:url";

/** Same bound as `MAX_UPDATE_NOTES_CHARS` in the app; the app would truncate longer notes. */
export const MAX_NOTES_CHARS = 4000;

const SUMS = "SHA256SUMS";
const MANIFEST = "latest.json";
const REPO_PATTERN = /^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/;
const TAG_PATTERN = /^v\d+\.\d+\.\d+(-[0-9A-Za-z.]+)?$/;
const VERSION_PATTERN = /^\d+\.\d+\.\d+$/;

/** Updater package suffix → updater target, per architecture tag of the file name. */
const PACKAGES = [
  ["_aarch64.app.tar.gz", "darwin-aarch64"],
  ["_x64.app.tar.gz", "darwin-x86_64"],
  ["_x64-setup.exe", "windows-x86_64"],
  ["_amd64.AppImage", "linux-x86_64"],
];

const PACKAGE_SUFFIX = /(\.app\.tar\.gz|-setup\.exe|\.AppImage)$/;

/** Updater target of the package `name` for `version`, or `null` if it is not one. */
export function platformOf(name, version) {
  for (const [suffix, platform] of PACKAGES) {
    if (name === `Dev-Copilot_${version}${suffix}`) {
      return platform;
    }
  }
  return null;
}

function isSumsFile(name) {
  return name === SUMS || name.startsWith(`${SUMS}.`);
}

/** Regular files of `dir`, sorted, except `SHA256SUMS*`. */
function assetNames(dir) {
  return readdirSync(dir)
    .filter((name) => !isSumsFile(name) && statSync(join(dir, name)).isFile())
    .sort();
}

function sha256(path) {
  return createHash("sha256").update(readFileSync(path)).digest("hex");
}

/** Manifest object for the assets in `dir`. */
export function buildManifest({ dir, version, tag, repo, notes, pubDate }) {
  const platforms = {};
  for (const name of assetNames(dir)) {
    const platform = platformOf(name, version);
    if (platform === null) {
      if (PACKAGE_SUFFIX.test(name)) {
        throw new Error(`${name} is not an updater package of version ${version}`);
      }
      continue;
    }
    const sigPath = join(dir, `${name}.sig`);
    const signature = existsSync(sigPath) ? readFileSync(sigPath, "utf8").trim() : "";
    if (!signature) {
      console.warn(`make-manifest: ${name} has no signature; ${platform} gets no update`);
      continue;
    }
    platforms[platform] = {
      signature,
      url: `https://github.com/${repo}/releases/download/${tag}/${encodeURIComponent(name)}`,
    };
  }
  if (Object.keys(platforms).length === 0) {
    throw new Error("no signed updater package: latest.json would offer nothing");
  }
  return { version, notes, pub_date: pubDate, platforms };
}

/** Writes `latest.json` and `SHA256SUMS` into `dir`, then verifies the sums. */
export function makeRelease({
  dir,
  version,
  tag,
  repo,
  notes = "",
  pubDate = new Date().toISOString().replace(/\.\d{3}Z$/, "Z"),
}) {
  if (!VERSION_PATTERN.test(version ?? "")) {
    throw new Error(`invalid version "${version}"`);
  }
  if (!TAG_PATTERN.test(tag ?? "")) {
    throw new Error(`invalid tag "${tag}"`);
  }
  if (!REPO_PATTERN.test(repo ?? "") || repo.split("/").some((part) => /^\.+$/.test(part))) {
    throw new Error(`invalid repo "${repo}"`);
  }
  if ([...notes].length > MAX_NOTES_CHARS) {
    throw new Error(`notes exceed ${MAX_NOTES_CHARS} characters`);
  }
  if (Number.isNaN(Date.parse(pubDate))) {
    throw new Error(`invalid pub date "${pubDate}"`);
  }
  for (const name of readdirSync(dir)) {
    if (/\s/.test(name)) {
      throw new Error(`asset name with whitespace: "${name}"`);
    }
  }

  const manifest = buildManifest({ dir, version, tag, repo, notes, pubDate });
  writeFileSync(join(dir, MANIFEST), `${JSON.stringify(manifest, null, 2)}\n`);

  // A signature of the previous sums would not verify; it is redone locally (T057).
  for (const name of readdirSync(dir)) {
    if (isSumsFile(name)) {
      rmSync(join(dir, name));
    }
  }
  const lines = assetNames(dir).map((name) => `${sha256(join(dir, name))}  ${name}\n`);
  writeFileSync(join(dir, SUMS), lines.join(""));
  verifySums(dir);
  return manifest;
}

/**
 * Checks that `SHA256SUMS` in `dir` lists every asset except `SHA256SUMS*`, with matching
 * hashes, and nothing else. Throws listing every problem.
 */
export function verifySums(dir) {
  const listed = new Map();
  const problems = [];
  for (const line of readFileSync(join(dir, SUMS), "utf8").split("\n")) {
    if (!line) continue;
    const match = /^([0-9a-f]{64}) [ *](.+)$/.exec(line);
    if (!match) {
      problems.push(`malformed line: ${line}`);
      continue;
    }
    listed.set(match[2], match[1]);
  }
  for (const name of assetNames(dir)) {
    const hash = listed.get(name);
    if (hash === undefined) {
      problems.push(`${name} is not covered by ${SUMS}`);
    } else if (hash !== sha256(join(dir, name))) {
      problems.push(`${name} does not match its hash in ${SUMS}`);
    }
    listed.delete(name);
  }
  for (const name of listed.keys()) {
    problems.push(`${name} is listed in ${SUMS} but missing`);
  }
  if (problems.length > 0) {
    throw new Error(problems.join("\n"));
  }
}

function main() {
  const { values } = parseArgs({
    options: {
      dir: { type: "string" },
      version: { type: "string" },
      tag: { type: "string" },
      repo: { type: "string" },
      "notes-file": { type: "string" },
      "pub-date": { type: "string" },
      check: { type: "boolean", default: false },
    },
    strict: true,
  });
  if (!values.dir) {
    throw new Error("--dir is required");
  }
  if (values.check) {
    verifySums(values.dir);
    console.log(`${SUMS} covers every asset of ${values.dir}`);
    return;
  }
  const notes = values["notes-file"] ? readFileSync(values["notes-file"], "utf8").trim() : "";
  const manifest = makeRelease({
    dir: values.dir,
    version: values.version,
    tag: values.tag,
    repo: values.repo,
    notes,
    ...(values["pub-date"] ? { pubDate: values["pub-date"] } : {}),
  });
  console.log(`latest.json: ${Object.keys(manifest.platforms).join(", ")}`);
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  try {
    main();
  } catch (error) {
    console.error(`make-manifest: ${error.message}`);
    process.exit(1);
  }
}
