# Updater test fixtures (002 T029)

Throwaway minisign keys used only by `tests/updater_adapter.rs`. They are **not** the release
key (T006, ADR-005) and must never be configured in `tauri.conf.json`.

| File | Content |
|------|---------|
| `test.key.pub` | Public key trusted by the mock app (key id `5F8BE192457A18B8`) |
| `other.key.pub` | Public key of a second, untrusted key |
| `package.bin` | Stand-in update package |
| `package.sig` | `package.bin` signed by the test key for version `0.2.0` |
| `package.other-key.sig` | `package.bin` signed by the other key for version `0.2.0` |
| `package.v0.1.5.sig` | `package.bin` signed by the test key for version `0.1.5` (version-binding check) |

The private keys were generated with `pnpm tauri signer generate -p ""`, used once with
`pnpm tauri signer sign --app-version <v>`, and deleted. To change a fixture, generate new keys
the same way and re-sign every file above.
