#!/usr/bin/env node
// Authenticode signing hook for `bundle.windows.signCommand` (specs/002 T050, research R3).
//
// The Tauri bundler calls `node ../../../tooling/release/windows-sign.mjs <file>` from
// `apps/desktop/src-tauri` for every binary and for the NSIS installer. This script:
//
// 1. Picks the provider from `WINDOWS_SIGN_PROVIDER` (recorded in ADR-005 by T005).
// 2. Signs with an RFC 3161 timestamp.
// 3. Verifies the result with `Get-AuthenticodeSignature`: the signature must be `Valid` and
//    timestamped, or the build fails.
//
// Fail closed: in CI (`GITHUB_ACTIONS=true`) or a release (`RELEASE_TAG` set), a missing
// provider or credential is an error, so no unsigned artifact can be produced (FR-005).
// Only a local developer build without a provider is left unsigned, with a warning.
//
// Credentials are read from the environment by the provider tool and are never printed.

import { spawnSync } from "node:child_process";
import { pathToFileURL } from "node:url";

const DESCRIPTION = "Dev Copilot";

/** Environment variables each provider needs. Values are never logged. */
export const PROVIDERS = {
  // Azure Artifact Signing (formerly Trusted Signing), via `artifact-signing-cli`, which
  // reads the account, profile and service principal from the environment and timestamps
  // with http://timestamp.acs.microsoft.com.
  "artifact-signing": [
    "AZURE_CLIENT_ID",
    "AZURE_CLIENT_SECRET",
    "AZURE_TENANT_ID",
    "AZURE_ARTIFACT_SIGNING_ENDPOINT",
    "AZURE_ARTIFACT_SIGNING_ACCOUNT",
    "AZURE_ARTIFACT_SIGNING_CERTIFICATE_PROFILE",
  ],
};

/**
 * Decides what to do for `file` under `env`, without side effects.
 *
 * @returns {{ action: "skip", reason: string }
 *   | { action: "fail", reason: string }
 *   | { action: "sign", command: string, args: string[] }}
 */
export function plan(env, file) {
  const strict = env.GITHUB_ACTIONS === "true" || Boolean(env.RELEASE_TAG);
  if (!file) {
    return { action: "fail", reason: "no file to sign (expected the %1 argument)" };
  }
  const provider = env.WINDOWS_SIGN_PROVIDER;
  if (!provider) {
    return strict
      ? { action: "fail", reason: "WINDOWS_SIGN_PROVIDER is not set in CI or a release" }
      : {
          action: "skip",
          reason: "WINDOWS_SIGN_PROVIDER is not set: local build left unsigned",
        };
  }
  const required = PROVIDERS[provider];
  if (!required) {
    return { action: "fail", reason: `unknown WINDOWS_SIGN_PROVIDER "${provider}"` };
  }
  const missing = required.filter((name) => !env[name]);
  if (missing.length > 0) {
    return { action: "fail", reason: `missing ${missing.join(", ")}` };
  }
  return {
    action: "sign",
    command: "artifact-signing-cli",
    args: ["-e", env.AZURE_ARTIFACT_SIGNING_ENDPOINT, "-d", DESCRIPTION, file],
  };
}

/** PowerShell check: valid Authenticode signature with a timestamp countersignature. */
const VERIFY = [
  "$s = Get-AuthenticodeSignature -LiteralPath $env:DEVCOPILOT_SIGNED_FILE",
  "if ($s.Status -ne 'Valid') { Write-Error \"signature status: $($s.Status)\"; exit 1 }",
  "if ($null -eq $s.TimeStamperCertificate) { Write-Error 'signature has no timestamp'; exit 1 }",
  "Write-Output \"signed by: $($s.SignerCertificate.Subject)\"",
].join("; ");

function run(command, args, env) {
  const result = spawnSync(command, args, { stdio: "inherit", env, shell: false });
  if (result.error) {
    throw new Error(`${command}: ${result.error.message}`);
  }
  if (result.status !== 0) {
    throw new Error(`${command} exited with ${result.status}`);
  }
}

function main() {
  const file = process.argv[2];
  const step = plan(process.env, file);
  if (step.action === "skip") {
    console.warn(`windows-sign: ${step.reason}: ${file}`);
    return;
  }
  if (step.action === "fail") {
    throw new Error(step.reason);
  }
  run(step.command, step.args, process.env);
  if (process.platform === "win32") {
    run("powershell.exe", ["-NoProfile", "-NonInteractive", "-Command", VERIFY], {
      ...process.env,
      DEVCOPILOT_SIGNED_FILE: file,
    });
  }
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  try {
    main();
  } catch (error) {
    console.error(`windows-sign: ${error.message}`);
    process.exit(1);
  }
}
