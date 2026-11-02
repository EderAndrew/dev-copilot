// Tests for collect-bundle.mjs: `node --test tooling/release/`.

import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { afterEach, beforeEach, test } from "node:test";

import { collect } from "./collect-bundle.mjs";

let root;
let bundle;
let out;

beforeEach(() => {
  root = mkdtempSync(join(tmpdir(), "collect-bundle-"));
  bundle = join(root, "bundle");
  out = join(root, "out");
});

afterEach(() => {
  rmSync(root, { recursive: true, force: true });
});

function write(...paths) {
  for (const path of paths) {
    mkdirSync(dirname(join(bundle, path)), { recursive: true });
    writeFileSync(join(bundle, path), path);
  }
}

function collected() {
  return readdirSync(out).sort();
}

test("macOS: renames the DMG and the per-arch updater tarball", () => {
  write(
    "dmg/Dev Copilot_0.1.0_aarch64.dmg",
    "macos/Dev Copilot.app.tar.gz",
    "macos/Dev Copilot.app.tar.gz.sig",
    "macos/Dev Copilot.app/Contents/Info.plist",
  );
  collect({ bundleDir: bundle, outDir: out, platform: "macos", arch: "aarch64", version: "0.1.0" });
  assert.deepEqual(collected(), [
    "Dev-Copilot_0.1.0_aarch64.app.tar.gz",
    "Dev-Copilot_0.1.0_aarch64.app.tar.gz.sig",
    "Dev-Copilot_0.1.0_aarch64.dmg",
  ]);
});

test("Windows: only the NSIS installer and its signature", () => {
  write("nsis/Dev Copilot_0.1.0_x64-setup.exe", "nsis/Dev Copilot_0.1.0_x64-setup.exe.sig");
  collect({ bundleDir: bundle, outDir: out, platform: "windows", arch: "x64", version: "0.1.0" });
  assert.deepEqual(collected(), [
    "Dev-Copilot_0.1.0_x64-setup.exe",
    "Dev-Copilot_0.1.0_x64-setup.exe.sig",
  ]);
});

test("Linux: AppImage with signature and the lowercase .deb", () => {
  write(
    "appimage/Dev Copilot_0.1.0_amd64.AppImage",
    "appimage/Dev Copilot_0.1.0_amd64.AppImage.sig",
    "deb/Dev Copilot_0.1.0_amd64.deb",
  );
  collect({ bundleDir: bundle, outDir: out, platform: "linux", arch: "amd64", version: "0.1.0" });
  assert.deepEqual(collected(), [
    "Dev-Copilot_0.1.0_amd64.AppImage",
    "Dev-Copilot_0.1.0_amd64.AppImage.sig",
    "dev-copilot_0.1.0_amd64.deb",
  ]);
});

test("fails closed when the updater signature is missing", () => {
  write("nsis/Dev Copilot_0.1.0_x64-setup.exe");
  assert.throws(
    () => collect({ bundleDir: bundle, outDir: out, platform: "windows", arch: "x64", version: "0.1.0" }),
    /\.sig/,
  );
});

test("fails on a missing or ambiguous artifact", () => {
  write("appimage/Dev Copilot_0.1.0_amd64.AppImage", "appimage/Dev Copilot_0.1.0_amd64.AppImage.sig");
  assert.throws(
    () => collect({ bundleDir: bundle, outDir: out, platform: "linux", arch: "amd64", version: "0.1.0" }),
    /deb/,
  );
  write("deb/a_0.1.0_amd64.deb", "deb/b_0.1.0_amd64.deb");
  assert.throws(
    () => collect({ bundleDir: bundle, outDir: out, platform: "linux", arch: "amd64", version: "0.1.0" }),
    /more than one/,
  );
});

test("fails on an artifact of another version", () => {
  write("nsis/Dev Copilot_0.0.9_x64-setup.exe", "nsis/Dev Copilot_0.0.9_x64-setup.exe.sig");
  assert.throws(
    () => collect({ bundleDir: bundle, outDir: out, platform: "windows", arch: "x64", version: "0.1.0" }),
    /0\.1\.0/,
  );
});

test("rejects an unknown platform or architecture", () => {
  assert.throws(() => collect({ bundleDir: bundle, outDir: out, platform: "bsd", arch: "x64", version: "0.1.0" }));
  assert.throws(() => collect({ bundleDir: bundle, outDir: out, platform: "macos", arch: "ppc", version: "0.1.0" }));
});
