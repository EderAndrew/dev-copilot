# Contract fixtures (002)

Sample IPC payloads for the commands added by this feature (`contracts/ipc-updates.md`). Same rules as
`specs/001-meeting-sessions/contracts/fixtures/README.md`: each directory is named after its DTO, and every
`*.json` file must

- deserialize into the Rust DTO and serialize back to the identical JSON
  (`apps/desktop/src-tauri/tests/contract_fixtures.rs`);
- satisfy the TypeScript type in `apps/desktop/src/ipc/types.ts`
  (`apps/desktop/src/ipc/contract.test.ts`).

Fixtures contain synthetic data only — never real meeting content.
