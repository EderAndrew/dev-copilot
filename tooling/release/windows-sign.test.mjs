// Tests for the fail-closed decision of windows-sign.mjs: `node --test tooling/release/`.

import assert from "node:assert/strict";
import { test } from "node:test";

import { PROVIDERS, plan } from "./windows-sign.mjs";

const FILE = "C:\\build\\Dev Copilot_0.1.0_x64-setup.exe";

function credentials() {
  return Object.fromEntries(PROVIDERS["artifact-signing"].map((name) => [name, `value-of-${name}`]));
}

test("local build without a provider is left unsigned", () => {
  assert.equal(plan({}, FILE).action, "skip");
});

test("CI or a release without a provider fails", () => {
  assert.equal(plan({ GITHUB_ACTIONS: "true" }, FILE).action, "fail");
  assert.equal(plan({ RELEASE_TAG: "v0.1.0" }, FILE).action, "fail");
});

test("a missing credential fails and names it without its value", () => {
  const env = { ...credentials(), WINDOWS_SIGN_PROVIDER: "artifact-signing" };
  delete env.AZURE_CLIENT_SECRET;
  const step = plan(env, FILE);
  assert.equal(step.action, "fail");
  assert.match(step.reason, /AZURE_CLIENT_SECRET/);
  assert.doesNotMatch(step.reason, /value-of-/);
});

test("an unknown provider fails", () => {
  assert.equal(plan({ WINDOWS_SIGN_PROVIDER: "self-signed" }, FILE).action, "fail");
});

test("a missing file argument fails", () => {
  assert.equal(plan({ ...credentials(), WINDOWS_SIGN_PROVIDER: "artifact-signing" }, undefined).action, "fail");
});

test("signs the file with the configured endpoint, secrets stay in the environment", () => {
  const step = plan({ ...credentials(), WINDOWS_SIGN_PROVIDER: "artifact-signing" }, FILE);
  assert.equal(step.action, "sign");
  assert.equal(step.command, "artifact-signing-cli");
  assert.equal(step.args.at(-1), FILE);
  assert.deepEqual(step.args.slice(0, 2), ["-e", "value-of-AZURE_ARTIFACT_SIGNING_ENDPOINT"]);
  assert.ok(!step.args.some((arg) => arg.includes("AZURE_CLIENT_SECRET")));
});
