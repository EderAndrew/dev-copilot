#!/usr/bin/env node
// Copies one build's release assets out of the Tauri bundle directory under their canonical
// names (contracts/release-artifacts.md), for make-manifest.mjs.
//
//   node tooling/release/collect-bundle.mjs --bundle-dir <target>/release/bundle \
//     --out-dir release-assets --platform macos|windows|linux --arch aarch64|x64|amd64 \
//     --version 0.1.0
//
// The bundler names files after `productName` ("Dev Copilot", with a space) and gives the
// macOS updater tarball no version or architecture, so two macOS builds would collide. Every
// expected file must exist exactly once, and every updater package must have its `.sig`;
// otherwise the build fails instead of publishing an incomplete or unverifiable release.

import { copyFileSync, existsSync, mkdirSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { parseArgs } from "node:util";
import { pathToFileURL } from "node:url";

/** Expected artifacts per platform: bundle subdirectory, file suffix, canonical name. */
const LAYOUT = {
  macos: {
    arches: ["aarch64", "x64"],
    files: (version, arch) => [
      { subdir: "dmg", suffix: `_${version}_${arch}.dmg`, name: `Dev-Copilot_${version}_${arch}.dmg` },
      {
        subdir: "macos",
        suffix: ".app.tar.gz",
        name: `Dev-Copilot_${version}_${arch}.app.tar.gz`,
        signed: true,
      },
    ],
  },
  windows: {
    arches: ["x64"],
    files: (version) => [
      {
        subdir: "nsis",
        suffix: `_${version}_x64-setup.exe`,
        name: `Dev-Copilot_${version}_x64-setup.exe`,
        signed: true,
      },
    ],
  },
  linux: {
    arches: ["amd64"],
    files: (version) => [
      {
        subdir: "appimage",
        suffix: `_${version}_amd64.AppImage`,
        name: `Dev-Copilot_${version}_amd64.AppImage`,
        signed: true,
      },
      { subdir: "deb", suffix: `_${version}_amd64.deb`, name: `dev-copilot_${version}_amd64.deb` },
    ],
  },
};

function findOne(dir, suffix) {
  const matches = existsSync(dir) ? readdirSync(dir).filter((name) => name.endsWith(suffix)) : [];
  if (matches.length === 0) {
    throw new Error(`no *${suffix} in ${dir}`);
  }
  if (matches.length > 1) {
    throw new Error(`more than one *${suffix} in ${dir}: ${matches.join(", ")}`);
  }
  return matches[0];
}

/** Copies the expected artifacts into `outDir`; returns the canonical names written. */
export function collect({ bundleDir, outDir, platform, arch, version }) {
  const layout = LAYOUT[platform];
  if (!layout) {
    throw new Error(`unknown platform "${platform}"`);
  }
  if (!layout.arches.includes(arch)) {
    throw new Error(`unsupported architecture "${arch}" for ${platform}`);
  }
  // Resolve everything before copying, so a failure leaves nothing half-collected.
  const copies = [];
  for (const file of layout.files(version, arch)) {
    const dir = join(bundleDir, file.subdir);
    const found = findOne(dir, file.suffix);
    copies.push([join(dir, found), file.name]);
    if (file.signed) {
      const sig = join(dir, `${found}.sig`);
      if (!existsSync(sig)) {
        throw new Error(`${found} has no updater signature (${found}.sig)`);
      }
      copies.push([sig, `${file.name}.sig`]);
    }
  }
  mkdirSync(outDir, { recursive: true });
  for (const [from, name] of copies) {
    copyFileSync(from, join(outDir, name));
  }
  return copies.map(([, name]) => name);
}

function main() {
  const { values } = parseArgs({
    options: {
      "bundle-dir": { type: "string" },
      "out-dir": { type: "string" },
      platform: { type: "string" },
      arch: { type: "string" },
      version: { type: "string" },
    },
    strict: true,
  });
  for (const option of ["bundle-dir", "out-dir", "platform", "arch", "version"]) {
    if (!values[option]) {
      throw new Error(`--${option} is required`);
    }
  }
  const names = collect({
    bundleDir: values["bundle-dir"],
    outDir: values["out-dir"],
    platform: values.platform,
    arch: values.arch,
    version: values.version,
  });
  console.log(names.join("\n"));
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  try {
    main();
  } catch (error) {
    console.error(`collect-bundle: ${error.message}`);
    process.exit(1);
  }
}
