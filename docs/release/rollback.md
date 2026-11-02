# Plano de rollback

O que fazer quando um release publicado precisa ser retirado, suspenso ou substituído. Vale para todos os
releases. Requisitos: spec FR-008, FR-028 e SC-013; limites técnicos em `docs/adr/ADR-005.md` ("Limites do
rollback de atualizações"); formato do manifesto em
`specs/002-desktop-release-readiness/contracts/release-artifacts.md`.

Nomes usados aqui:

- **N**: a versão publicada com problema (tag `vN`).
- **N−1**: a última versão boa publicada antes dela. No primeiro release (0.1.0) não existe N−1.
- **N+1**: a versão de correção.
- **T0**: o momento do acionamento. O relógio do SC-013 começa aqui.

## Princípios

1. **Os dados locais do usuário nunca são apagados nem migrados pelo rollback.** Nenhum passo, nenhum aviso
   e nenhuma nota pede para apagar `devcopilot.db*`, a entrada do cofre do SO ou a pasta de dados.
2. **Corrigir para a frente.** O updater não faz downgrade e o `requireSignedVersion` impede anunciar uma
   versão antiga como nova. Quem já instalou N só sai dela com N+1.
3. **Nenhum controle de segurança é contornado.** Os avisos nunca pedem para desativar Gatekeeper,
   SmartScreen, a verificação de assinatura ou o updater por um caminho não documentado. Para baixar de novo,
   aponte para `docs/release/verify-downloads.md`.
4. **A tag e o histórico ficam.** Não se apaga nem se move uma tag publicada. O release fica marcado e
   explicado, e as evidências continuam rastreáveis.
5. **Meta de tempo (SC-013):** em até **2 horas** depois de T0, os artefatos defeituosos deixam de ser
   oferecidos (download e updater) e o aviso público está publicado.

## Papéis

Hoje o projeto tem um único mantenedor, que acumula os três papéis. Registre quem exerceu cada papel no
registro do incidente.

| Papel | Responsabilidade | Hoje |
|-------|------------------|------|
| Responsável pelo release | Decide acionar, executa a suspensão e a retirada, aprova N+1 | o mantenedor |
| Guardião das chaves | Rotação e revogação de identidades e chaves; único que tem a chave OpenPGP e a cópia offline da minisign | o mantenedor |
| Comunicação | Publica e atualiza o aviso público e responde nas issues | o mantenedor |

Qualquer pessoa do projeto pode **pedir** o acionamento abrindo uma issue com `severity:critical`. Só o
responsável pelo release aciona. Se ele estiver indisponível por mais de 2 horas num incidente de chave ou
de segurança, quem tiver acesso de administrador ao repositório executa ao menos os passos 1 e 2 (suspender
e retirar), que não exigem nenhuma chave.

## Gatilhos

| Gatilho | Exemplos | Ações |
|---------|----------|-------|
| **Defeito crítico** | Qualquer bloqueador da FR-030 que aparece depois da publicação: captura sem início explícito ou após negativa; perda ou corrupção de sessão; falha na recuperação; conteúdo ou credenciais em logs; estado de captura não perceptível por leitor de tela ou teclado; app que não abre numa plataforma declarada | 1, 2, 4, 5, 6 |
| **Defeito grave restrito a uma plataforma** | Instalador ou atualização quebrada só no Windows; falha só em Wayland | 3 (só aquela plataforma), 4, 5, 6 |
| **Incidente de segurança** | Vulnerabilidade explorável na versão publicada; artefato publicado que não confere com o `SHA256SUMS` assinado; atestação que não verifica | 1, 2, 4, 5, 6, e §Chaves se houver chave envolvida |
| **Comprometimento de chave ou identidade** | Vazamento ou suspeita sobre a chave minisign, a chave OpenPGP, o certificado Apple, a identidade Authenticode, a conta do GitHub ou um segredo do environment `release` | §Chaves, mais 1 e 2 se algum artefato puder ter sido assinado por terceiros |
| **Perda de chave** | Chave minisign ou OpenPGP perdida sem cópia | §Chaves (perda) |

Defeito alto (`severity:high`) com contorno não aciona o rollback. Ele vai para as notas e para
`limitations.md`, e a correção sai em N+1 pelo caminho normal.

## Passos

### 0. Acionar e registrar (T0)

1. Abra ou reaproveite a issue do problema com `severity:critical`, `release-blocker`, `release-<N>` e o
   `platform:*` afetado. Anote nela a hora de T0 (UTC).
2. Copie `docs/release/rollback-record.md` (modelo) para `docs/release/v<N>/rollback-<AAAA-MM-DD>.md` e
   preencha o cabeçalho. Anote a hora de cada passo seguinte.
3. Não discuta detalhes exploráveis de um incidente de segurança na issue pública. Use um *security
   advisory* privado do GitHub e deixe na issue só o necessário para o aviso.

### 1. Suspender a oferta da atualização (versão inteira)

O app consulta `https://github.com/EderAndrew/dev-copilot/releases/latest/download/latest.json`. Esse
endereço segue o release marcado como *latest*. Devolver a marca para N−1 faz o endpoint servir o
`latest.json` de N−1, que anuncia N−1. Os apps em N−1 deixam de ver N, e nenhum arquivo é editado.

```sh
REPO=EderAndrew/dev-copilot
# N vira pré-release (pré-release nunca é "latest") e N−1 volta a ser "latest"
gh release edit vN   --repo $REPO --prerelease --latest=false
gh release edit vN-1 --repo $REPO --latest

# Conferir o que o endpoint serve agora (pode levar alguns minutos por causa do cache)
curl -sSL https://github.com/$REPO/releases/latest/download/latest.json | jq -r .version   # → N−1
```

**Primeiro release (sem N−1):** volte o release para rascunho com `gh release edit v0.1.0 --repo $REPO
--draft`. O endpoint passa a responder 404. O app trata isso como falha de verificação, mantém a
versão instalada e não oferece nada. Ninguém tem uma versão anterior para onde voltar, então o passo 4 é o
que importa.

Quem já instalou N continua em N. Isso não é revertido e se resolve no passo 5.

### 2. Retirar os artefatos defeituosos

O passo 1 não basta para defeito crítico ou incidente de segurança. Um app que verificou **antes** da
suspensão guarda N como `available` até a próxima verificação, que acontece na inicialização ou a cada 24 h.
Se o usuário confirmar, o app baixa N pela URL do manifesto antigo. Com os pacotes apagados, o download
falha, o app vai para `failed` e a versão instalada não muda.

```sh
# Pacotes de atualização e instaladores de N (ajuste à lista real do release)
for a in \
  Dev-Copilot_N_aarch64.app.tar.gz Dev-Copilot_N_aarch64.app.tar.gz.sig Dev-Copilot_N_aarch64.dmg \
  Dev-Copilot_N_x64-setup.exe Dev-Copilot_N_x64-setup.exe.sig \
  Dev-Copilot_N_amd64.AppImage Dev-Copilot_N_amd64.AppImage.sig dev-copilot_N_amd64.deb
do gh release delete-asset vN "$a" --repo $REPO -y; done
```

- **Mantenha** `SHA256SUMS`, `SHA256SUMS.asc` e `latest.json` em N. Com eles, quem já baixou um arquivo
  consegue identificar pelo hash qual artefato foi retirado.
- Antes de apagar, guarde uma cópia dos artefatos fora do GitHub (para análise), com os SHA-256, e
  registre onde ela está.
- Defeito grave **sem** risco de segurança ou de dados: o responsável pode manter os instaladores e só
  marcar N como pré-release com o aviso no topo das notas. Registre a decisão e o motivo.

### 3. Suspender uma plataforma só

Uma plataforma fora do `latest.json` significa "sem atualização" para ela. As outras continuam recebendo N.
O `make-manifest.mjs` só põe no manifesto as plataformas que têm pacote com `.sig` na pasta. Basta tirar o
pacote dela e regenerar, como na seção "Build Intel não validado" de `signing-and-distribution.md`:

```sh
mkdir rollback-vN && cd rollback-vN
gh release download vN --repo $REPO
rm Dev-Copilot_N_x64-setup.exe Dev-Copilot_N_x64-setup.exe.sig      # exemplo: suspender o Windows
gh release delete-asset vN Dev-Copilot_N_x64-setup.exe     --repo $REPO -y
gh release delete-asset vN Dev-Copilot_N_x64-setup.exe.sig --repo $REPO -y
rm SHA256SUMS SHA256SUMS.asc latest.json
node <repo>/tooling/release/make-manifest.mjs --dir . --version N --tag vN \
  --repo $REPO --notes-file <repo>/docs/release/vN/update-notes.txt
gpg --local-user <impressão digital> --detach-sign --armor SHA256SUMS
gpg --verify SHA256SUMS.asc SHA256SUMS
gh release upload vN latest.json SHA256SUMS SHA256SUMS.asc --clobber --repo $REPO
curl -sSL https://github.com/$REPO/releases/latest/download/latest.json | jq '.platforms | keys'
```

O `latest.json` e o `SHA256SUMS` regenerados ficam sem atestação do CI, porque foram feitos fora dele.
Registre isso no registro do incidente e no aviso.

### 4. Publicar o aviso público

Onde publicar, nesta ordem:

1. No topo das notas do release N (`gh release edit vN --notes-file …`).
2. Numa issue fixada (*pinned*) com o rótulo `release-<N>`.
3. Num bloco "Aviso" no início do `README.md`, até a publicação de N+1.

Modelo, em português e em inglês. Preencha os campos e apague as linhas que não se aplicam:

```markdown
> **Aviso — Dev Copilot <N> (<data, UTC>)**
>
> Encontramos <descrição curta e não técnica do problema> em <plataformas>. A versão <N> deixou de ser
> oferecida para download e pelo atualizador do app.
>
> - **Você está em <N−1> ou anterior:** não precisa fazer nada. O app não vai oferecer a <N>.
> - **Você já instalou a <N>:** <o que fazer até a correção, por exemplo "não use a função X" ou "continue
>   usando normalmente">. A correção <N+1> chega pelo atualizador. Seus dados locais não são afetados por
>   este aviso nem pela correção. Não apague a pasta de dados do app.
> - **Você baixou um arquivo da <N> e ainda não instalou:** não instale. Os arquivos retirados são:
>   `<arquivo>` (SHA-256 `<hash>`).
> - Para baixar e verificar uma versão, siga <link para verify-downloads.md>. Não desative o Gatekeeper, o
>   SmartScreen nem outra proteção do sistema para instalar o Dev Copilot.
>
> Acompanhe em <link da issue>. Próxima atualização deste aviso: <hora>.

> **Notice — Dev Copilot <N> (<date, UTC>)**
>
> We found <short, non-technical description> on <platforms>. Version <N> is no longer offered for
> download or by the in-app updater.
>
> - **You are on <N−1> or earlier:** no action needed. The app will not offer <N>.
> - **You already installed <N>:** <what to do until the fix>. The fix, <N+1>, arrives through the updater.
>   Your local data is not affected by this notice or by the fix. Do not delete the app's data folder.
> - **You downloaded a <N> file but did not install it:** do not install it. Withdrawn files: `<file>`
>   (SHA-256 `<hash>`).
> - To download and verify a version, follow <link to verify-downloads.md>. Do not disable Gatekeeper,
>   SmartScreen or any other system protection to install Dev Copilot.
>
> Follow <issue link>. Next update to this notice: <time>.
```

Num incidente de chave, acrescente quais versões e arquivos foram assinados com a chave afetada e a nova
impressão digital, quando houver.

Hora de fim do SC-013: quando os passos 1 e 2 (ou 3) e o aviso estão publicados e conferidos. Anote no
registro.

### 5. Corrigir para a frente (N+1)

- N+1 tem versão **maior** que N. Só assim o updater a oferece para quem está em N.
- Passa pelo pipeline normal: `rc.N`, assinatura, evidências e checklist (`docs/release/checklist.md`). A
  urgência não dispensa nenhum item. Ela permite reaproveitar evidências de casos que a correção não afeta,
  com a justificativa registrada (mesma regra da T068).
- As notas de N+1 dizem o que foi corrigido, se N+1 muda o formato dos dados locais e se a versão é
  reversível (veja "Dados locais").
- Ao publicar N+1 como *latest*, o endpoint passa a servir o manifesto de N+1. Tanto quem está em N−1 quanto
  quem está em N recebe N+1.

### 6. Encerrar

1. Atualize o aviso dizendo que N+1 está disponível e tire o bloco do `README.md`.
2. Feche a issue com o link para N+1 e para o registro.
3. Complete o registro do incidente: linha do tempo, tempo até a retirada, o que funcionou, o que mudar
   neste plano.
4. Se o problema revelou uma limitação, acrescente-a em `docs/release/v<N+1>/limitations.md`.
5. Revise este plano se algum passo não funcionou como escrito.

## Chaves e identidades

Identidades, segredos e procedimentos de renovação estão em `signing-and-distribution.md`. Aqui ficam só o
comprometimento e a perda. Depois de qualquer item abaixo, atualize a tabela "Identidades e chaves" do
ADR-005 e registre o incidente.

### Chave minisign de atualização

O app instalado confia **só** na chave pública embutida em `plugins.updater.pubkey`. Para instalar uma
atualização maliciosa, um atacante precisa da chave privada **e** precisa controlar o que o endpoint serve,
ou seja, a conta ou os releases do repositório no GitHub.

- **Comprometida, ainda em nosso poder:**
  1. Revise imediatamente o acesso ao GitHub (veja "Conta do GitHub" abaixo) e confira se o `latest.json`
     servido e os pacotes conferem com o `SHA256SUMS` assinado.
  2. Gere uma nova chave (`tauri signer generate`) e guarde a cópia offline cifrada.
  3. Publique N+1 **assinada com a chave antiga** e com a **nova** chave pública em `plugins.updater.pubkey`
     (e o novo `updater-pubkey-id` no ADR-005). A partir de N+2, assine com a nova chave.
  4. Troque os segredos `TAURI_SIGNING_PRIVATE_KEY*` no environment `release` e destrua as cópias da chave
     antiga.
  5. O aviso pede que todos atualizem para N+1. Quem ficar numa versão antiga continua confiando na chave
     antiga. O risco residual depende de o atacante também controlar o endpoint.
- **Perdida (sem cópia) ou comprometida sem rotação possível:** nenhum app instalado aceita mais uma
  atualização legítima.
  1. Gere uma nova chave e publique N+1 com a nova chave pública, assinada com ela.
  2. Os apps antigos ainda veem N+1 no manifesto, mas rejeitam o pacote na verificação. Vão para `failed`
     sem mudar de versão. Essa é a falha segura esperada, e o aviso precisa explicá-la para o usuário não
     achar que é defeito.
  3. O aviso orienta a **reinstalação manual** pelo instalador assinado de N+1, verificado como em
     `verify-downloads.md`. A reinstalação mantém os dados locais, porque o `identifier` e a identidade de
     assinatura são os mesmos (veja "Dados locais").

### Chave OpenPGP de release

1. Revogue a chave com o certificado de revogação gerado na T007
   (`gpg --import <revogação>.asc`) e publique a chave revogada em `docs/release/release-key.asc` e nos
   servidores de chaves usados.
2. Gere uma nova chave. Se a antiga ainda estiver em seu poder, assine a nova com a antiga.
3. Assine de novo o `SHA256SUMS` de cada release ainda oferecido e substitua os `SHA256SUMS.asc`
   (`gh release upload … --clobber`).
4. Atualize a impressão digital no `README.md`, no `apps/desktop/README.md`, no ADR-005 e em
   `verify-downloads.md`.
5. O aviso informa a nova impressão digital e pede para conferir de novo os downloads feitos depois da data
   provável do comprometimento.

A chave OpenPGP não entra no app. Trocá-la não afeta os apps instalados nem o updater.

### Certificado Apple Developer ID

1. Revogue o certificado em developer.apple.com e, se houver suspeita de uso por terceiros, avise a Apple
   (Developer Support). Revogar pode fazer o Gatekeeper bloquear as versões assinadas com esse certificado.
2. Emita um novo certificado **na mesma conta e Team ID** e troque `APPLE_CERTIFICATE*` e
   `APPLE_SIGNING_IDENTITY` no environment `release`.
3. Publique N+1 assinada com o novo certificado, o mais rápido possível.
4. Quem tiver o app bloqueado reinstala manualmente pelo `.dmg` de N+1. Com o mesmo Team ID e o mesmo
   `identifier`, o app continua lendo a chave do banco no Keychain (ADR-005, "Achado: Keychain do macOS").
   Mudar de Team ID equivale a perder o acesso ao banco existente. Só faça isso se não houver alternativa, e
   diga isso no aviso. Mesmo assim, o banco não é apagado.
5. Se a chave de API do App Store Connect vazou, revogue-a e emita outra (`APPLE_API_*`).

### Identidade Authenticode

1. Revogue ou suspenda o perfil de certificado no Azure Artifact Signing (ou no emissor OV) e troque as
   credenciais `AZURE_*` do service principal.
2. Assinaturas com carimbo de tempo continuam válidas, a menos que o certificado seja revogado com data
   anterior a elas. Nesse caso, publique N+1 assinada com a nova identidade.
3. Informe no aviso que o SmartScreen pode voltar a mostrar aviso de reputação na nova identidade, sem
   pedir que ele seja desativado.

### Conta do GitHub ou environment `release`

1. Troque a senha, revogue sessões, tokens e chaves SSH, e confirme o 2FA.
2. Revise o *audit log*, as tags, os releases e os assets publicados. Confira os artefatos de cada release
   ainda oferecido contra o `SHA256SUMS.asc` (assinado fora do CI) e contra as atestações
   (`gh attestation verify`).
3. Troque **todos** os segredos do environment `release`.
4. Se algum artefato não conferir, trate como incidente de segurança (passos 1, 2 e 4) e suponha que a
   chave minisign pode ter vazado.

## Dados locais

Formato e migrações: `docs/local-data.md`, seção "Formato e migrações".

- O app **recusa abrir um banco mais novo** que ele (`user_version` maior que a última migração conhecida) e
  informa erro de armazenamento. O banco não é alterado.
- Por isso, **voltar manualmente** de N para N−1 (reinstalando o instalador de N−1) só funciona se N **não**
  trouxe migração nova. Se trouxe, N−1 recusa abrir e os dados ficam intactos esperando N+1. O aviso não
  recomenda voltar de versão nesse caso.
- Toda nota de release diz se a versão muda o formato dos dados e se é reversível ("Reversível: sim/não").
  O 0.1.0 define a linha de base: esquema `v1`, sem versão anterior.
- O rollback nunca orienta a desinstalar com remoção de dados, apagar `devcopilot.db*`, apagar a entrada do
  cofre do SO nem "reinstalar do zero".
- Para quem quiser uma cópia antes de instalar N+1: feche o app e copie os três arquivos `devcopilot.db*`
  juntos (`docs/local-data.md`). A cópia continua cifrada e depende da mesma entrada do cofre.

## Ensaio (SC-013)

O plano é ensaiado num fork de teste antes do primeiro release (T089) e de novo depois de qualquer mudança
no pipeline, no updater ou neste documento. Roteiro e registro: `docs/release/v0.1.0/rollback-rehearsal.md`.
O ensaio cobre os passos 1, 2, 4 e 5. Ele termina quando os apps instalados em N−1 não veem mais N, os apps
em N recebem N+1 e as sessões locais dos computadores de teste continuam lá.

## Revisão

Este plano é revisado:

- em cada release (item 11 do checklist);
- depois de cada acionamento real ou ensaio;
- depois de qualquer rotação de chave ou mudança de provedor de assinatura.
