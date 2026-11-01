# Build candidato v0.1.0-rc.N

> Modelo (T011). Preenchido em T057 a cada novo candidato. Um candidato novo (`rc.N+1`) substitui este
> conteúdo; o histórico fica no Git. Formato: `contracts/release-evidence.md`.

- Tag: `v0.1.0-rc.N` · Commit: `<sha completo>` · Workflow: <URL da execução>
- Data do build: `<RFC 3339>`

| Artefato | SHA-256 | Assinatura | Notarização / timestamp | Atestação |
|----------|---------|------------|-------------------------|-----------|
| `Dev-Copilot_0.1.0_aarch64.dmg` | | Developer ID: ✅/❌ | notarizado + staple: ✅/❌ | `gh attestation verify`: ✅/❌ |
| `Dev-Copilot_0.1.0_aarch64.app.tar.gz` (+ `.sig`) | | Developer ID: | — | |
| `Dev-Copilot_0.1.0_x64.dmg` *(só se macOS Intel for declarado)* | | | | |
| `Dev-Copilot_0.1.0_x64-setup.exe` (+ `.sig`) | | Authenticode: | timestamp RFC 3161: | |
| `Dev-Copilot_0.1.0_amd64.AppImage` (+ `.sig`) | | coberto por `SHA256SUMS.asc` | — | |
| `dev-copilot_0.1.0_amd64.deb` | | coberto por `SHA256SUMS.asc` | — | |
| `latest.json` | | — | — | |
| `SHA256SUMS` | | `SHA256SUMS.asc` (OpenPGP, local): ✅/❌ | — | |

## Verificações feitas

- `gpg --verify SHA256SUMS.asc SHA256SUMS`: <saída resumida, impressão digital da chave>
- `shasum -a 256 -c SHA256SUMS`: <resultado>
- `spctl -a -vv` (macOS): <saída>
- Assinatura Authenticode (Windows, `Get-AuthenticodeSignature`): <saída>

Toda evidência em `evidence/` cita o commit curto deste candidato. Evidência produzida com outro candidato é
revalidada, ou justificada como não afetada pela diferença entre os commits.
