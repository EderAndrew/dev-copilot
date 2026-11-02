# Verificar os downloads do Dev Copilot

Antes de instalar, você pode conferir que o arquivo baixado é exatamente o que o projeto publicou. Todos os
passos usam ferramentas que já vêm com o sistema (ou com o `gpg`). **Nenhum passo pede para desativar o
Gatekeeper, o SmartScreen ou qualquer outro controle de segurança.** Se um aviso do sistema aparecer e este
guia não o explicar, não instale: abra uma issue.

## 1. Baixe os arquivos

Na página do release (`https://github.com/EderAndrew/dev-copilot/releases`), baixe para a mesma pasta:

- o instalador da sua plataforma;
- `SHA256SUMS`;
- `SHA256SUMS.asc`.

| Plataforma | Arquivo |
|------------|---------|
| macOS (Apple Silicon) | `Dev-Copilot_<versão>_aarch64.dmg` |
| macOS (Intel), se publicado | `Dev-Copilot_<versão>_x64.dmg` |
| Windows 10/11 (x64) | `Dev-Copilot_<versão>_x64-setup.exe` |
| Linux (x86_64) | `Dev-Copilot_<versão>_amd64.AppImage` ou `dev-copilot_<versão>_amd64.deb` |

## 2. Importe a chave de release (uma vez)

A chave pública OpenPGP do projeto está em `docs/release/release-key.asc` no repositório. Impressão digital:

```text
<40 hex: publicada na T007>
```

Confira que a impressão digital acima é a mesma do `README.md` do repositório e do `docs/adr/ADR-005.md`.

```sh
curl -fsSLO https://raw.githubusercontent.com/EderAndrew/dev-copilot/main/docs/release/release-key.asc
gpg --import release-key.asc
gpg --fingerprint
```

No Windows, o `gpg` vem com o [Gpg4win](https://gpg4win.org). No macOS, com `brew install gnupg` ou o
GPG Suite.

## 3. Verifique a assinatura do `SHA256SUMS` (todos os sistemas)

```sh
gpg --verify SHA256SUMS.asc SHA256SUMS
```

O resultado precisa conter `Good signature` e a impressão digital da seção 2. O aviso
`This key is not certified with a trusted signature` só diz que você não assinou a chave do projeto com a
sua; a conferência da impressão digital acima é o que importa. `BAD signature` ou outra impressão digital:
**pare**, os arquivos não são do projeto.

## 4. Confira o hash do instalador

### macOS

```sh
grep ' Dev-Copilot_<versão>_aarch64.dmg$' SHA256SUMS | shasum -a 256 -c
```

O instalador baixado precisa aparecer como `OK`.

### Linux

```sh
sha256sum -c --ignore-missing SHA256SUMS
```

O instalador baixado precisa aparecer como `OK`.

### Windows (PowerShell)

```powershell
$file = "Dev-Copilot_<versão>_x64-setup.exe"
$expected = (Select-String -Path SHA256SUMS -Pattern ([regex]::Escape($file) + '$')).Line.Split(' ')[0]
$actual = (Get-FileHash -Algorithm SHA256 $file).Hash.ToLower()
if ($actual -eq $expected) { "OK" } else { "FALHOU: não instale" }
```

`FAILED` ou `FALHOU` significa que o arquivo foi alterado ou corrompido. Apague-o e baixe de novo pela
página do release.

## 5. Verificação própria de cada sistema

### macOS: assinatura Developer ID e notarização

Abra o `.dmg` e arraste o app para Aplicativos. Antes de abrir:

```sh
spctl -a -vv "/Applications/Dev Copilot.app"
```

O resultado precisa dizer `accepted` e `source=Notarized Developer ID`, com o nome do desenvolvedor do
projeto. Ao abrir, o macOS mostra no máximo o aviso normal de app baixado da internet, com a opção **Abrir**.
Se aparecer que o app "não pode ser aberto" ou que o desenvolvedor não pôde ser verificado, não contorne:
o arquivo não é o publicado.

No primeiro uso do microfone, o macOS pergunta se o **Dev Copilot** pode usá-lo. Isso é esperado.

### Windows: assinatura Authenticode

1. Clique com o botão direito no `-setup.exe` → **Propriedades** → aba **Assinaturas Digitais**. A
   assinatura precisa estar listada, em nome do editor do projeto, e **Detalhes** precisa dizer "Esta
   assinatura digital está OK".
2. Ou, no PowerShell:

   ```powershell
   Get-AuthenticodeSignature .\Dev-Copilot_<versão>_x64-setup.exe | Format-List Status, SignerCertificate, TimeStamperCertificate
   ```

   `Status` precisa ser `Valid`.
3. Ao executar, o instalador não pede privilégios de administrador (instala só para o seu usuário). Se o
   Windows mostrar o Controle de Conta de Usuário, ele precisa exibir o editor verificado do projeto.

O SmartScreen pode mostrar um aviso de reputação nas primeiras semanas de uma versão nova, mesmo com a
assinatura válida. Isso é uma limitação conhecida, listada nas limitações do release. A assinatura conferida
nos passos acima é o que garante a origem. Não desative o SmartScreen.

### Linux

No Linux, a verificação das seções 3 e 4 é a garantia de origem: não há assinatura de plataforma.

- **AppImage**: `chmod +x Dev-Copilot_<versão>_amd64.AppImage` e execute.
- **.deb**: `sudo apt install ./dev-copilot_<versão>_amd64.deb`. O `.deb` não se atualiza pelo app: para
  atualizar, verifique e instale o novo `.deb` da mesma forma.

## 6. Opcional: proveniência do build

Cada arquivo do release tem uma atestação do GitHub que o liga ao commit e ao workflow que o construiu:

```sh
gh attestation verify Dev-Copilot_<versão>_aarch64.dmg --repo EderAndrew/dev-copilot
```

## Atualizações dentro do app

As atualizações oferecidas pelo app usam outra assinatura (minisign), verificada pelo próprio app antes de
instalar. Um pacote alterado é recusado e nada é instalado. Você não precisa repetir este guia para elas.
