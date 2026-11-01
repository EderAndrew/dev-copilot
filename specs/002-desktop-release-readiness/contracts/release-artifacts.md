# Contract: artefatos de release, verificação e manifesto de atualização

Este contrato vale para cada release a partir de `v0.1.0`. É a interface entre o pipeline de release, o
usuário final (que verifica downloads) e o app instalado (que consome o manifesto).

## Artefatos por release (assets do release no GitHub)

| Plataforma | Artefato | Assinatura da plataforma | Atualizável pelo app |
|------------|----------|--------------------------|----------------------|
| macOS Apple Silicon | `Dev-Copilot_<ver>_aarch64.dmg` | Developer ID + notarização + staple | sim, via `Dev-Copilot_<ver>_aarch64.app.tar.gz` + `.sig` |
| macOS Intel *(só se validado)* | `Dev-Copilot_<ver>_x64.dmg` | idem | sim, via `…_x64.app.tar.gz` + `.sig` |
| Windows x64 | `Dev-Copilot_<ver>_x64-setup.exe` | Authenticode com timestamp | sim (o próprio `-setup.exe` + `.sig`) |
| Linux x86_64 | `Dev-Copilot_<ver>_amd64.AppImage` | coberto por `SHA256SUMS.asc` | sim, via `.AppImage` + `.sig` |
| Linux x86_64 (Debian/Ubuntu) | `dev-copilot_<ver>_amd64.deb` | coberto por `SHA256SUMS.asc` | não (atualização manual) |
| Todos | `SHA256SUMS` | — | — |
| Todos | `SHA256SUMS.asc` | OpenPGP, chave de release do projeto (assinatura local, fora do CI) | — |
| Todos | `latest.json` | — (cada pacote referenciado tem `.sig` minisign) | manifesto |

Os nomes exatos podem seguir o padrão do bundler do Tauri. O que é obrigatório é: um `SHA256SUMS` que cubra
**todos** os demais assets, e nenhum asset publicado sem estar coberto.

Cada asset também tem atestação de proveniência do GitHub, que o liga ao commit e ao workflow
(`gh attestation verify <arquivo> --repo EderAndrew/dev-copilot`).

## Procedimento de verificação do usuário (resumo; texto completo em `docs/release/verify-downloads.md`)

1. **Todos os SOs**: baixar `SHA256SUMS` e `SHA256SUMS.asc`; importar a chave pública de release e conferir a
   impressão digital publicada no README; `gpg --verify SHA256SUMS.asc SHA256SUMS`; conferir o hash do
   artefato (`sha256sum -c --ignore-missing SHA256SUMS` / `shasum -a 256 -c` / `Get-FileHash`).
2. **macOS**: além disso, o Gatekeeper abre o app sem aviso (`spctl -a -vv` mostra `Notarized Developer ID`).
3. **Windows**: além disso, as propriedades do arquivo mostram assinatura digital válida do editor do projeto,
   e o UAC mostra o editor verificado.

Nenhum passo pede para desativar Gatekeeper, SmartScreen ou qualquer outro controle (FR-009).

## Manifesto de atualização `latest.json` (formato do `tauri-plugin-updater` v2, arquivo estático)

```json
{
  "version": "0.1.1",
  "notes": "Texto puro, ≤ 4000 caracteres.",
  "pub_date": "2026-12-01T12:00:00Z",
  "platforms": {
    "darwin-aarch64": { "signature": "<conteúdo do .sig>", "url": "https://github.com/EderAndrew/dev-copilot/releases/download/v0.1.1/Dev-Copilot_0.1.1_aarch64.app.tar.gz" },
    "windows-x86_64": { "signature": "…", "url": "https://github.com/EderAndrew/dev-copilot/releases/download/v0.1.1/Dev-Copilot_0.1.1_x64-setup.exe" },
    "linux-x86_64":   { "signature": "…", "url": "https://github.com/EderAndrew/dev-copilot/releases/download/v0.1.1/Dev-Copilot_0.1.1_amd64.AppImage" }
  }
}
```

Regras:

- Todas as `url` usam HTTPS e apontam para assets do mesmo release.
- Cada `signature` é minisign, feita com a chave privada de atualização do projeto. A chave pública fica em
  `plugins.updater.pubkey` e é embutida no app.
- Uma plataforma ausente do manifesto significa "sem atualização" para aquela plataforma. É assim que uma
  plataforma é suspensa no rollback.
- Para suspender uma versão inteira, republica-se o `latest.json` do release anterior no release marcado como
  *latest*. O updater nunca faz downgrade de quem já atualizou (`rollback.md`).

## Segredos e chaves

| Chave | Uso | Onde fica | Quem acessa |
|-------|-----|-----------|-------------|
| Developer ID Application (Apple) | assinar `.app`/`.dmg` | environment `release` do GitHub (certificado `.p12` cifrado + senha) | workflow aprovado pelo responsável |
| Chave de API do App Store Connect | notarizar | environment `release` | idem |
| Identidade Authenticode (provedor do R3) | assinar `.exe` | credenciais do serviço de assinatura no environment `release` | idem |
| Chave privada minisign de atualização | assinar pacotes de atualização | environment `release` + cópia offline cifrada do responsável | idem |
| Chave privada OpenPGP de release | assinar `SHA256SUMS` | só na máquina/token do responsável, nunca no CI | responsável |

Nenhum desses valores aparece em log, artefato ou repositório. O gitleaks roda no pipeline de release, e os
passos de assinatura não ecoam variáveis.
