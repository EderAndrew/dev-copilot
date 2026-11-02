// Tests for make-manifest.mjs (specs/002 T049): `node --test tooling/release/`.

import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, beforeEach, describe, test } from "node:test";

import {
  MAX_NOTES_CHARS,
  makeRelease,
  platformOf,
  verifySums,
} from "./make-manifest.mjs";

const VERSION = "0.1.1";
const TAG = "v0.1.1";
const REPO = "EderAndrew/dev-copilot";
const PUB_DATE = "2026-12-01T12:00:00Z";
const BASE = `https://github.com/${REPO}/releases/download/${TAG}/`;

/** Every asset of a full release, with the updater signatures. */
const FULL = {
  "Dev-Copilot_0.1.1_aarch64.dmg": "dmg-arm",
  "Dev-Copilot_0.1.1_aarch64.app.tar.gz": "tar-arm",
  "Dev-Copilot_0.1.1_aarch64.app.tar.gz.sig": "sig-darwin-arm\n",
  "Dev-Copilot_0.1.1_x64.dmg": "dmg-x64",
  "Dev-Copilot_0.1.1_x64.app.tar.gz": "tar-x64",
  "Dev-Copilot_0.1.1_x64.app.tar.gz.sig": "sig-darwin-x64\n",
  "Dev-Copilot_0.1.1_x64-setup.exe": "exe",
  "Dev-Copilot_0.1.1_x64-setup.exe.sig": "sig-windows\n",
  "Dev-Copilot_0.1.1_amd64.AppImage": "appimage",
  "Dev-Copilot_0.1.1_amd64.AppImage.sig": "sig-linux\n",
  "dev-copilot_0.1.1_amd64.deb": "deb",
};

let dir;

beforeEach(() => {
  dir = mkdtempSync(join(tmpdir(), "make-manifest-"));
});

afterEach(() => {
  rmSync(dir, { recursive: true, force: true });
});

function write(files) {
  for (const [name, content] of Object.entries(files)) {
    writeFileSync(join(dir, name), content);
  }
}

function release(options = {}) {
  return makeRelease({
    dir,
    version: VERSION,
    tag: TAG,
    repo: REPO,
    pubDate: PUB_DATE,
    ...options,
  });
}

function readManifest() {
  return JSON.parse(readFileSync(join(dir, "latest.json"), "utf8"));
}

function sums() {
  return readFileSync(join(dir, "SHA256SUMS"), "utf8");
}

function sha256(content) {
  return createHash("sha256").update(content).digest("hex");
}

describe("latest.json", () => {
  test("follows contracts/release-artifacts.md", () => {
    write(FULL);
    release({ notes: "Correções." });
    assert.deepEqual(readManifest(), {
      version: VERSION,
      notes: "Correções.",
      pub_date: PUB_DATE,
      platforms: {
        "darwin-aarch64": {
          signature: "sig-darwin-arm",
          url: `${BASE}Dev-Copilot_0.1.1_aarch64.app.tar.gz`,
        },
        "darwin-x86_64": {
          signature: "sig-darwin-x64",
          url: `${BASE}Dev-Copilot_0.1.1_x64.app.tar.gz`,
        },
        "windows-x86_64": {
          signature: "sig-windows",
          url: `${BASE}Dev-Copilot_0.1.1_x64-setup.exe`,
        },
        "linux-x86_64": {
          signature: "sig-linux",
          url: `${BASE}Dev-Copilot_0.1.1_amd64.AppImage`,
        },
      },
    });
  });

  test("uses HTTPS URLs only", () => {
    write(FULL);
    release();
    for (const { url } of Object.values(readManifest().platforms)) {
      assert.equal(new URL(url).protocol, "https:");
    }
  });

  test("omits a platform whose .sig is missing", () => {
    const files = { ...FULL };
    delete files["Dev-Copilot_0.1.1_x64.app.tar.gz.sig"];
    write(files);
    release();
    assert.deepEqual(Object.keys(readManifest().platforms).sort(), [
      "darwin-aarch64",
      "linux-x86_64",
      "windows-x86_64",
    ]);
  });

  test("omits a platform whose .sig is empty", () => {
    write({ ...FULL, "Dev-Copilot_0.1.1_x64-setup.exe.sig": " \n" });
    release();
    assert.equal(readManifest().platforms["windows-x86_64"], undefined);
  });

  test("fails without any signed updater package", () => {
    write({ "dev-copilot_0.1.1_amd64.deb": "deb" });
    assert.throws(() => release(), /no signed updater package/);
  });

  test("fails on a package of another version", () => {
    write({ ...FULL, "Dev-Copilot_0.1.0_amd64.AppImage": "old" });
    assert.throws(() => release(), /Dev-Copilot_0\.1\.0_amd64\.AppImage/);
  });

  test("fails on notes longer than the app accepts", () => {
    write(FULL);
    assert.throws(
      () => release({ notes: "n".repeat(MAX_NOTES_CHARS + 1) }),
      /notes/,
    );
    release({ notes: "n".repeat(MAX_NOTES_CHARS) });
  });

  test("fails on a non-HTTPS-safe repository or tag", () => {
    write(FULL);
    assert.throws(() => release({ repo: "owner/../x" }), /repo/);
    assert.throws(() => release({ tag: "v0.1.1/../x" }), /tag/);
  });

  test("fails on asset names with whitespace", () => {
    write({ ...FULL, "Dev Copilot_0.1.1_aarch64.dmg": "x" });
    assert.throws(() => release(), /whitespace/);
  });
});

describe("SHA256SUMS", () => {
  test("covers every asset except SHA256SUMS*", () => {
    write({ ...FULL, "SHA256SUMS.asc": "stale signature" });
    release();
    const lines = sums().trimEnd().split("\n");
    const names = lines.map((line) => line.split("  ")[1]);
    assert.deepEqual(names, [...Object.keys(FULL), "latest.json"].sort());
    for (const line of lines) {
      const [hash, name] = line.split("  ");
      assert.equal(hash, sha256(readFileSync(join(dir, name))), name);
    }
  });

  test("verifySums accepts the generated file", () => {
    write(FULL);
    release();
    verifySums(dir);
  });

  test("verifySums fails when an asset is missing from the sums", () => {
    write(FULL);
    release();
    writeFileSync(join(dir, "extra.zip"), "not covered");
    assert.throws(() => verifySums(dir), /extra\.zip/);
  });

  test("verifySums fails when an asset changed after the sums", () => {
    write(FULL);
    release();
    writeFileSync(join(dir, "dev-copilot_0.1.1_amd64.deb"), "tampered");
    assert.throws(() => verifySums(dir), /dev-copilot_0\.1\.1_amd64\.deb/);
  });

  test("verifySums fails when a listed asset was removed", () => {
    write(FULL);
    release();
    rmSync(join(dir, "Dev-Copilot_0.1.1_x64.dmg"));
    assert.throws(() => verifySums(dir), /Dev-Copilot_0\.1\.1_x64\.dmg/);
  });
});

describe("platformOf", () => {
  test("maps updater packages to updater targets", () => {
    assert.equal(platformOf("Dev-Copilot_0.1.1_aarch64.app.tar.gz", VERSION), "darwin-aarch64");
    assert.equal(platformOf("Dev-Copilot_0.1.1_x64.app.tar.gz", VERSION), "darwin-x86_64");
    assert.equal(platformOf("Dev-Copilot_0.1.1_x64-setup.exe", VERSION), "windows-x86_64");
    assert.equal(platformOf("Dev-Copilot_0.1.1_amd64.AppImage", VERSION), "linux-x86_64");
  });

  test("ignores installers the updater does not use", () => {
    for (const name of [
      "Dev-Copilot_0.1.1_aarch64.dmg",
      "dev-copilot_0.1.1_amd64.deb",
      "Dev-Copilot_0.1.1_x64-setup.exe.sig",
      "latest.json",
    ]) {
      assert.equal(platformOf(name, VERSION), null, name);
    }
  });
});
