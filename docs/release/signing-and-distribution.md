# Assinatura e distribuição

Como o Dev Copilot é assinado, construído e entregue como release em rascunho, e como o responsável mantém
as identidades e chaves. Decisões em `docs/adr/ADR-005.md`. Formato dos artefatos em
`specs/002-desktop-release-readiness/contracts/release-artifacts.md`.

Este documento tem só **nomes** de segredos, nunca valores. Os identificadores públicos (Team ID, key ID,
impressão digital) ficam no ADR-005.

## Pré-requisitos

| Item | Para quê | Onde se obtém | Task |
|------|----------|---------------|------|
| Conta Apple Developer Program | certificado *Developer ID Application* e chave de API do App Store Connect | developer.apple.com | T005 |
| Provedor Authenticode | assinar o `.exe` e o instalador NSIS no CI | Azure Artifact Signing (antigo Trusted Signing); senão, OV com chave em HSM na nuvem | T005 |
| Par de chaves minisign de atualização | assinar os pacotes que o app baixa | `pnpm --filter appsdesktop tauri signer generate -w <arquivo>` | T006 |
| Chave OpenPGP de release | assinar o `SHA256SUMS` | `gpg --full-generate-key` (ed25519), na máquina ou token do responsável | T007 |
| Environment `release` no GitHub | guardar os segredos e exigir aprovação | Settings → Environments | T008 |
| `gpg`, `gh`, Node ≥ 24, `pnpm` | passos locais do release | gerenciador de pacotes do SO | — |

## Identidades e chaves

| Identidade / chave | Usada por | Onde vive | Cópia de segurança |
|--------------------|-----------|-----------|--------------------|
| Developer ID Application (`.p12`) | job `build` (macOS): `codesign` do `.app` e do `.dmg` | segredo `APPLE_CERTIFICATE` (base64) + `APPLE_CERTIFICATE_PASSWORD` | `.p12` cifrado offline com o responsável; reemissível pela conta Apple |
| Chave de API do App Store Connect (`.p8`) | job `build` (macOS): `notarytool` | segredos `APPLE_API_KEY`, `APPLE_API_ISSUER`, `APPLE_API_PRIVATE_KEY` | revogável e reemissível no App Store Connect |
| Identidade Authenticode | job `build` (Windows): `signCommand` → `tooling/release/windows-sign.mjs` | credenciais do provedor (abaixo) | a chave fica no HSM do provedor e nunca sai dele |
| Chave privada minisign | job `build`: `.sig` de cada pacote de atualização | segredos `TAURI_SIGNING_PRIVATE_KEY` + `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | **obrigatória**: arquivo cifrado offline com o responsável. Sem ela, nenhum app instalado aceita uma atualização |
| Chave pública minisign | o app instalado verifica cada pacote | `plugins.updater.pubkey` em `apps/desktop/src-tauri/tauri.conf.json`; key ID no ADR-005 (`updater-pubkey-id`) | no repositório |
| Chave privada OpenPGP de release | o responsável assina `SHA256SUMS` localmente | só na máquina ou token do responsável. **Nunca no CI** | backup offline cifrado; certificado de revogação gerado junto |
| Chave pública OpenPGP | o usuário verifica `SHA256SUMS.asc` | `docs/release/release-key.asc`; impressão digital no README e no ADR-005 | no repositório |

## Segredos

### Environment `release` (T008)

Configuração em Settings → Environments → `release`:

- **Required reviewers**: o responsável pelo release. Nenhum job de assinatura começa sem essa aprovação.
- **Deployment branches and tags**: só tags com o padrão `v*.*.*`.
- **Prevent self-review**: desligado enquanto houver um único responsável. Revisar ao entrar um segundo
  mantenedor.

Segredos do environment (só nomes):

| Nome | Conteúdo |
|------|----------|
| `APPLE_CERTIFICATE` | `.p12` do Developer ID Application, em base64 (`base64 -i cert.p12`) |
| `APPLE_CERTIFICATE_PASSWORD` | senha do `.p12` |
| `APPLE_SIGNING_IDENTITY` | nome da identidade, por exemplo `Developer ID Application: <nome> (<Team ID>)` |
| `APPLE_API_KEY` | Key ID da chave de API do App Store Connect |
| `APPLE_API_ISSUER` | Issuer ID do App Store Connect |
| `APPLE_API_PRIVATE_KEY` | conteúdo do arquivo `AuthKey_<id>.p8` |
| `TAURI_SIGNING_PRIVATE_KEY` | conteúdo do arquivo da chave privada minisign |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | senha da chave minisign |
| `AZURE_CLIENT_ID`, `AZURE_CLIENT_SECRET`, `AZURE_TENANT_ID` | service principal com o papel *Artifact Signing Certificate Profile Signer* (provedor `artifact-signing`) |

Variáveis do environment (não secretas):

| Nome | Conteúdo |
|------|----------|
| `WINDOWS_SIGN_PROVIDER` | `artifact-signing`. É o único provedor suportado hoje por `windows-sign.mjs`; se a T005 escolher OV com HSM na nuvem, acrescente o provedor ao script e aos testes antes do primeiro release |
| `AZURE_ARTIFACT_SIGNING_ENDPOINT` | endpoint regional, por exemplo `https://eus.codesigning.azure.net` |
| `AZURE_ARTIFACT_SIGNING_ACCOUNT` | nome da conta de assinatura |
| `AZURE_ARTIFACT_SIGNING_CERTIFICATE_PROFILE` | nome do perfil de certificado |

O `GITHUB_TOKEN` do workflow é o único token usado. Nenhum segredo de repositório fica fora do environment.

### Regras

- Segredos só entram nos passos por `env:`, nunca interpolados (`${{ secrets.X }}`) dentro de um script.
- Nenhum passo imprime variáveis. A senha do keychain temporário do macOS é gerada no job e mascarada.
- O gitleaks roda nos gates de cada PR e do release.
- O CI de PR não tem acesso ao environment `release`.

## Cortar uma tag

1. Confirme que `tauri.conf.json`, `apps/desktop/package.json` e o `Cargo.toml` do app têm a versão do
   release (`cargo test -p devcopilot-desktop --test version_consistency`).
2. Opcional: escreva as notas de atualização em texto puro (até 4000 caracteres) em
   `docs/release/v<versão>/update-notes.txt`. O app mostra esse texto ao oferecer a atualização.
3. Crie e envie a tag a partir do commit validado:

   ```sh
   git tag -s v0.1.0-rc.1 -m "Dev Copilot 0.1.0-rc.1"
   git push origin v0.1.0-rc.1
   ```

   Tags aceitas: `v<versão>` e `v<versão>-rc.N` (N ≥ 1). Uma tag `-rc.N` gera um rascunho marcado como
   pré-release.
4. Aprove o environment `release` quando o GitHub pedir (aparece depois dos gates).
5. Ao fim do workflow, siga "Conferir o rascunho e assinar o `SHA256SUMS`".

## O que cada job faz (`.github/workflows/release.yml`)

| Job | Roda em | O que faz | Falha quando |
|-----|---------|-----------|--------------|
| `gates` | `ci.yml` reutilizado | fmt, clippy, testes Rust e frontend, `cargo audit`/`deny`, gitleaks, `pnpm audit`, testes de `tooling/release/`, `dist/` sem estilo inline | qualquer gate da CI falha |
| `release-config` | `ubuntu-22.04` | `version_consistency`, `updater_config` e `bundle_config` com `RELEASE_TAG` | a tag não bate com a versão, a chave pública do updater ainda é o marcador ou não bate com o ADR-005, ou a configuração de bundle mudou |
| `build` (×4) | `macos-15` aarch64 e x86_64, `windows-latest`, `ubuntu-22.04`, no environment `release` | confere os segredos antes do build; prepara a assinatura; `tauri build --target <triple> --bundles <formatos>`; verifica as assinaturas; copia os artefatos com nomes canônicos (`collect-bundle.mjs`) | falta segredo, a assinatura, a notarização ou o staple falham, ou falta um artefato ou seu `.sig` |
| `draft` | `ubuntu-latest` | junta os artefatos, gera `latest.json` e `SHA256SUMS` (`make-manifest.mjs`), atesta a proveniência de cada asset e cria o release **em rascunho** | o manifesto ficaria vazio, um asset fica fora do `SHA256SUMS`, ou já existe release para a tag |

Detalhes por plataforma no job `build`:

- **macOS**: importa o `.p12` num keychain temporário com senha aleatória. O bundler assina o `.app` com
  runtime reforçado e `Entitlements.plist` (só microfone), notariza e faz staple. Depois o workflow assina,
  notariza e faz staple do `.dmg`, e confere com `codesign --verify --deep --strict`, `stapler validate` e
  `spctl -a -vv` (`source=Notarized Developer ID`) o `.app`, o `.dmg` e o `.app` dentro do `.app.tar.gz`
  do updater. O keychain e a chave `.p8` são apagados no fim, mesmo se o job falhar.
- **Windows**: instala `artifact-signing-cli` 0.12.0 (`cargo install --locked`). O bundler chama
  `windows-sign.mjs` para cada binário e para o instalador. O script assina com timestamp RFC 3161
  (`timestamp.acs.microsoft.com`) e confere com `Get-AuthenticodeSignature` (status `Valid` e com
  timestamp). No CI, a falta de provedor ou de credencial é erro; só um build local fica sem assinatura,
  com aviso. O workflow confere de novo o instalador e o executável.
- **Linux**: AppImage e `.deb` em `ubuntu-22.04` (glibc mais antiga). Não há assinatura de plataforma: os
  dois ficam cobertos pelo `SHA256SUMS.asc`.
- **Todos**: o `.sig` minisign de cada pacote de atualização (`.app.tar.gz`, `-setup.exe`, `.AppImage`) é
  gerado com `TAURI_SIGNING_PRIVATE_KEY`.

O release não restaura cache de build (Rust nem pnpm), para que nada produzido num branch não confiável
chegue a um artefato publicado. Por isso o release demora mais que a CI.

## Conferir o rascunho e assinar o `SHA256SUMS`

Feito pelo responsável, na própria máquina, com a chave OpenPGP de release:

```sh
mkdir -p ~/release/v0.1.0-rc.1 && cd ~/release/v0.1.0-rc.1
gh release download v0.1.0-rc.1 --repo EderAndrew/dev-copilot

# Integridade e proveniência
shasum -a 256 -c SHA256SUMS
for f in $(cut -d' ' -f3 SHA256SUMS); do
  gh attestation verify "$f" --repo EderAndrew/dev-copilot
done

# Assinatura destacada, em ASCII
gpg --local-user <impressão digital da chave de release> --detach-sign --armor SHA256SUMS
gpg --verify SHA256SUMS.asc SHA256SUMS

gh release upload v0.1.0-rc.1 SHA256SUMS.asc --repo EderAndrew/dev-copilot
```

Depois disso, registre o candidato em `docs/release/v<versão>/candidate.md` (commit, URL do workflow,
SHA-256, estado de assinatura, notarização, timestamp e atestação).

### Build Intel não validado

O build macOS x86_64 só é publicado se validado em hardware Intel real (FR-032). Se não for, antes de
assinar:

```sh
gh release delete-asset v0.1.0 Dev-Copilot_0.1.0_x64.dmg --repo EderAndrew/dev-copilot -y
gh release delete-asset v0.1.0 Dev-Copilot_0.1.0_x64.app.tar.gz --repo EderAndrew/dev-copilot -y
gh release delete-asset v0.1.0 Dev-Copilot_0.1.0_x64.app.tar.gz.sig --repo EderAndrew/dev-copilot -y
rm Dev-Copilot_0.1.0_x64.*
node <repo>/tooling/release/make-manifest.mjs --dir . --version 0.1.0 --tag v0.1.0 \
  --repo EderAndrew/dev-copilot --notes-file <repo>/docs/release/v0.1.0/update-notes.txt
gh release upload v0.1.0 latest.json SHA256SUMS --clobber --repo EderAndrew/dev-copilot
```

O `latest.json` regenerado não tem `darwin-x86_64`, então nenhum Mac Intel recebe essa atualização. A
atestação do `latest.json` e do `SHA256SUMS` regenerados deixa de valer, porque eles foram feitos fora do
CI. Registre isso no `candidate.md` e como limitação.

## Publicar

Só com o checklist em 12/12 (`docs/release/checklist.md`) e a decisão registrada em
`docs/release/v<versão>/decision.md`. Edite o rascunho no GitHub, desmarque *pre-release* na versão final e
publique. O endpoint `releases/latest/download/latest.json` passa a servir o novo manifesto. Rascunhos e
pré-releases nunca são *latest*, então um candidato nunca é oferecido pelo updater.

## Ensaio do pipeline (quickstart §3, T055)

Num fork, com identidades de teste (certificado Apple de teste ou de desenvolvimento, chave minisign e
OpenPGP descartáveis):

1. No fork, ajuste a versão dos três arquivos para `0.0.1`, coloque a chave pública minisign de teste em
   `plugins.updater.pubkey` e a linha `updater-pubkey-id` correspondente no ADR-005.
2. Use a tag `v0.0.1-rc.1`. O gate de versão só aceita `v<versão>` e `-rc.N`, então a tag `v0.0.1-test.1`
   do quickstart seria recusada pelo `release-config`.
3. Confirme que os jobs `build` esperam a aprovação do environment.
4. Remova um segredo de assinatura e repita com `v0.0.1-rc.2`: o job falha antes do build, e nenhum
   rascunho é criado.
5. Registre os resultados em `docs/release/v0.1.0/pipeline-rehearsal.md`.

## Renovação de certificados

| Item | Validade típica | Antes de vencer | Efeito de deixar vencer |
|------|-----------------|-----------------|-------------------------|
| Developer ID Application | 5 anos | gerar um novo certificado **na mesma conta e Team ID**, atualizar `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD` e `APPLE_SIGNING_IDENTITY` | apps já notarizados continuam abrindo; não há como assinar versões novas. Trocar de Team ID quebra o acesso ao Keychain de quem já usa o app (ADR-005) |
| Chave de API do App Store Connect | sem expiração | revogar e emitir outra se vazar | — |
| Authenticode (Artifact Signing) | certificados de curta duração renovados pelo serviço | manter a conta e a validação de identidade ativas | assinaturas antigas continuam válidas por causa do timestamp |
| Authenticode (OV em HSM) | 1–3 anos | renovar com o emissor e atualizar as credenciais | idem |
| Chave OpenPGP | a data definida na T007 | estender a validade (`gpg --quick-set-expire`), reexportar `release-key.asc` e publicar | `gpg --verify` passa a avisar de chave expirada |

Registre cada renovação no ADR-005 (tabela "Identidades e chaves") com a nova data de validade.

## Rotação de chaves

- **minisign (atualização)**: publique uma versão N assinada com a chave **antiga** que já embute a chave
  pública **nova** em `plugins.updater.pubkey` (e o novo `updater-pubkey-id` no ADR-005). A partir da N+1,
  assine com a nova. Quem pular a versão N só atualiza pela N reinstalando. Se a chave antiga vazar ou se
  perder sem rotação, os usuários precisam reinstalar manualmente pelo instalador assinado
  (`docs/release/rollback.md`).
- **OpenPGP (release)**: gere a nova chave, assine a nova chave pública com a antiga, publique as duas
  impressões digitais no README e no ADR-005 durante uma transição e revogue a antiga com o certificado de
  revogação se ela tiver sido comprometida.
- **Apple / Authenticode**: revogue no portal do emissor se vazar. Revogar um certificado Apple pode
  invalidar a notarização das versões assinadas com ele: publique imediatamente uma versão assinada com o
  certificado novo.
- **Segredos do environment**: depois de qualquer rotação, substitua o valor no environment `release` e
  apague arquivos locais temporários.
