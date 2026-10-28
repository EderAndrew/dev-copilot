# Contract fixtures

Sample IPC payloads shared by both sides of the boundary (research R11). Each directory is named
after the DTO it holds; every `*.json` file must:

- deserialize into the Rust DTO and serialize back to the identical JSON
  (`apps/desktop/src-tauri/tests/contract_fixtures.rs`);
- satisfy the TypeScript type in `apps/desktop/src/ipc/types.ts`
  (`apps/desktop/src/ipc/contract.test.ts`).

Fixtures contain synthetic data only — never real meeting content.
