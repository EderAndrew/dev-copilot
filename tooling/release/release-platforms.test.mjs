// Tests for release-platforms.mjs: `node --test tooling/release/`.

import assert from "node:assert/strict";
import { test } from "node:test";

import { TARGETS, selectTargets } from "./release-platforms.mjs";

const ids = (targets) => targets.map((t) => t.id);

test("no selection builds every target, in canonical order", () => {
  const all = ["macos-aarch64", "macos-x64", "windows-x64", "linux-amd64"];
  assert.deepEqual(ids(TARGETS), all);
  for (const spec of [undefined, "", "   ", "\n"]) {
    assert.deepEqual(ids(selectTargets(spec)), all);
  }
});

test("selects only the named targets (0.1.0: macOS Apple Silicon)", () => {
  assert.deepEqual(ids(selectTargets("macos-aarch64")), ["macos-aarch64"]);
});

test("accepts commas and whitespace, ignores case and duplicates, keeps canonical order", () => {
  assert.deepEqual(ids(selectTargets(" Linux-AMD64,macos-aarch64  macos-aarch64\n")), [
    "macos-aarch64",
    "linux-amd64",
  ]);
});

test("a platform name alone selects all of its architectures", () => {
  assert.deepEqual(ids(selectTargets("macos")), ["macos-aarch64", "macos-x64"]);
});

test("an unknown name fails instead of silently building less", () => {
  assert.throws(() => selectTargets("macos-aarch64,windows-arm64"), /unknown release platform "windows-arm64"/);
  assert.throws(() => selectTargets("mac"), /unknown release platform "mac"/);
});

test("a selection of only separators fails", () => {
  assert.throws(() => selectTargets(",,"), /selects no platform/);
});

test("each target carries what the build job needs", () => {
  for (const target of TARGETS) {
    for (const key of ["id", "os", "platform", "target", "arch", "bundles"]) {
      assert.equal(typeof target[key], "string", `${target.id ?? "?"}.${key}`);
      assert.notEqual(target[key], "");
    }
    assert.equal(target.id, `${target.platform}-${target.arch}`);
  }
});
