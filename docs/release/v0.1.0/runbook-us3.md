# Roteiro de validação em hardware real — v0.1.0 (US3)

Passo a passo para quem executa T062–T066 de `specs/002-desktop-release-readiness/tasks.md` numa máquina real.
O resultado de cada máquina é o arquivo `evidence/<plataforma>.md`, já criado com todos os casos. Este
roteiro não substitui `specs/001-meeting-sessions/acceptance-matrix.md` (critérios) nem
`specs/002-desktop-release-readiness/quickstart.md` §2, §4 e §5. Ele junta o que é preciso para executá-los.

| Máquina | Tarefa | Arquivo |
|---------|--------|---------|
| macOS Apple Silicon | T062 | [`evidence/macos-arm64.md`](evidence/macos-arm64.md) |
| Windows 10 22H2 | T063 | [`evidence/windows-10.md`](evidence/windows-10.md) |
| Windows 11 | T064 | [`evidence/windows-11.md`](evidence/windows-11.md) |
| Ubuntu LTS, Wayland (PipeWire) | T065 | [`evidence/linux-wayland.md`](evidence/linux-wayland.md) |
| Ubuntu LTS, X11 | T066 | [`evidence/linux-x11.md`](evidence/linux-x11.md) |

Também cobre US4 (T071–T073), na §8.

`macos-x64` só existe se T061 declarar o build Intel. Nesse caso, copie `evidence/_template.md`.

## 1. Pré-requisitos (bloqueiam o início)

- [ ] Candidato `v0.1.0-rc.N` gerado e `candidate.md` preenchido (T057). Anote o commit curto: ele vai no
      título de cada arquivo de evidência.
- [ ] Instalação limpa I1–I4 registrada na mesma máquina e no mesmo perfil (T058–T060).
- [ ] Capturas de referência da CSP em `evidence/csp-visual/baseline/<os>/` (T021). Sem elas, C2 não tem
      com o que comparar.
- [ ] Rótulos de issue criados no GitHub (`README.md`, "Rótulos de issues").
- [ ] Material de teste **não confidencial** (Art. IV.16): um roteiro falado curto com um requisito, uma
      decisão, uma pergunta respondida e a frase de injeção do caso 6.

Material por máquina: microfone USB removível (caso 3), fone Bluetooth (caso 3b), proxy local com registro de
requisições (mitmproxy ou Charles; casos 8 e P1), provedor de análise configurável (casos 5–7) e chave de
teste de um provedor remoto (caso 8). Para os casos de SC-001, chame alguém que nunca usou o app.

## 2. Regras da rodada

- Use o binário do candidato, instalado por I2. Nunca use `tauri dev` nem um build local.
- Use um perfil de usuário novo que nunca tenha rodado um build de desenvolvimento (research R2, Keychain).
- Nunca contorne um aviso de segurança ou de permissão para "fazer passar". O contorno é o achado.
- ❌ sempre tem uma issue com `release-0.1.0`, `severity:*` e `platform:*`. Toda issue crítica também leva
  `release-blocker` e entra em T068.
- ➖ sempre leva o motivo escrito.
- Capturas ≤ 500 KB, sem credenciais nem conteúdo real. Vídeos e arquivos grandes vão como anexo do
  release em rascunho, citados por link + SHA-256.

## 3. Ordem sugerida numa máquina

1. Preencha **Ambiente** (versão exata do SO e do build, hardware, leitor de tela, pilha de áudio; no Linux,
   GPU e driver).
2. **P1** antes de qualquer outro caso: o download do modelo só é "primeiro" uma vez por perfil (§5).
3. **C1 e C2**: o percurso de quickstart §2 com o candidato. Faça as capturas de C2 no mesmo percurso.
4. Matriz **1–13, 3b, 3c, 11b**, na ordem. Deixe o caso 4 (encerramento forçado) e o 13 (cofre) por último,
   porque eles mexem no estado do app.
5. **Medições cronometradas**, com o histórico já populado por pelo menos 10 sessões de teste para SC-017.
6. Casos da plataforma (**W1** ou **L1**).
7. Releia o arquivo: nenhuma célula de resultado vazia; **Limitações observadas** e **Problemas abertos**
   preenchidos ou com "nenhum".

U1–U4 são de T047 (canal de teste do updater) e podem ficar para outra rodada na mesma máquina.

## 4. Preparação por plataforma

Diretórios de dados e logs: `docs/local-data.md`. O log é `devcopilot.log.*` na pasta de logs.

### 4.1 macOS

- Logs: `~/Library/Logs/com.drewed.devcopilot/`.
- Para repetir o caso 1, apague a permissão: `tccutil reset Microphone com.drewed.devcopilot`.
- 3c: Repouso pelo menu Apple; bloqueio com ⌃⌘Q.
- 4: Activity Monitor → Forçar encerramento (ou `kill -9`).
- 11/11b: VoiceOver (⌘F5), sem mouse.

### 4.2 Windows 10 e 11

- Logs: `%LOCALAPPDATA%\com.drewed.devcopilot\logs\`.
- **W1**: Configurações → Privacidade (Windows 11: Privacidade e segurança) → Microfone → desligue "Permitir
  que aplicativos da área de trabalho acessem o microfone". Abra o app e inicie uma sessão. Esperado:
  explicação e orientação; nada captado nem gravado. Religue depois.
- 3c: Iniciar → Energia → Suspender; bloqueio com Win+L.
- 4: Gerenciador de Tarefas → Finalizar tarefa.
- 11/11b: Windows 11 com Narrator (Ctrl+Win+Enter) **e** NVDA, cada um num percurso completo; Windows 10 só
  com Narrator (T072, §8).
- Um aviso de reputação do SmartScreen é limitação (T059 e `limitations.md`), não falha da matriz.

### 4.3 Ubuntu (Wayland e X11)

- Escolha a sessão na tela de login (engrenagem: "Ubuntu" = Wayland, "Ubuntu on Xorg" = X11). Confirme com
  `echo $XDG_SESSION_TYPE`.
- Logs: `~/.local/share/com.drewed.devcopilot/logs/`.
- Pilha de áudio: `pactl info | grep "Server Name"` (esperado: PipeWire).
- GPU e driver para o Ambiente: `lspci -k | grep -A2 -E "VGA|3D"`.
- **L1** (research R4): abra o AppImage e o `.deb`. Se a janela abrir em branco, registre GPU e driver, abra de
  novo com `WEBKIT_DISABLE_DMABUF_RENDERER=1 <binário>` e registre se o contorno resolve. Os dois resultados
  vão para `limitations.md` (T069).
- 3c: `systemctl suspend`; bloqueio com Super+L.
- 4: `kill -9 $(pgrep -f dev-copilot)`.
- 11/11b: Orca (Super+Alt+S).

### 4.4 Caso 13 no Linux: Secret Service parado

O GNOME reativa o `gnome-keyring` por D-Bus quando alguém chama `org.freedesktop.secrets`, então só parar o
serviço não basta. Abra o app pelo menos uma vez antes, para o banco existir.

```sh
systemctl --user mask --runtime --now gnome-keyring-daemon.socket gnome-keyring-daemon.service
pkill -f gnome-keyring-daemon || true
busctl --user status org.freedesktop.secrets   # precisa falhar antes de abrir o app
# abra o app: esperado explicação, nada gravado, captura bloqueada (FR-039a)
systemctl --user unmask --runtime gnome-keyring-daemon.socket gnome-keyring-daemon.service
```

Se `busctl` ainda achar o serviço, a ativação veio por outro caminho. Registre o que foi feito e use uma
sessão sem o keyring (outro usuário de teste sem login gráfico, com `dbus-run-session`).

No macOS e no Windows, o caso 13 pode ser provocado conforme a observação do arquivo de evidência. Se não
houver um jeito confiável, use ➖ com o motivo.

## 5. P1: primeiro download do modelo com rede restrita ou proxy

O cliente de download (`crates/intelligence/src/model_download.rs`) usa `reqwest` sem as features padrão. Pelo
código, esperamos que ele respeite as variáveis `HTTPS_PROXY`/`ALL_PROXY` e **não** leia o proxy configurado
no SO (preferências do macOS, WinINET, GNOME). Isso precisa ser confirmado aqui: é hipótese, não resultado.

Execute em perfil novo, antes de baixar o modelo pela primeira vez, e registre cada variante na observação de
P1:

1. **Rede bloqueada**: bloqueie `huggingface.co` (firewall ou `hosts`). Esperado: mensagem que explica a
   falha de rede; o app continua usável; nenhum arquivo parcial fica como modelo válido.
2. **Proxy só nas configurações do SO**, sem acesso direto. Se falhar, a explicação aparece? É limitação.
3. **Proxy por variável de ambiente** (`HTTPS_PROXY=http://host:porta`, app aberto pelo terminal). Baixa?
4. **Proxy com inspeção TLS** (CA corporativa instalada no SO). O download aceita a CA ou falha com
   explicação?

Qualquer variante sem contorno documentável vira limitação em `limitations.md`, com a mensagem exibida. Nunca
desligue a verificação de HTTPS ou de SHA-256 para passar.

## 6. C1 e C2

Percurso (quickstart §2): Sessões → nova sessão → consentimento → captura → pausa → parada → Histórico →
Detalhe → Exportação (cancelar) → Exclusão (cancelar) → Provedores → Configurações.

C1: conte as linhas no log depois do percurso. Cole o comando e a saída como evidência.

```sh
# macOS
grep -c csp_violation ~/Library/Logs/com.drewed.devcopilot/devcopilot.log*
# Linux
grep -c csp_violation ~/.local/share/com.drewed.devcopilot/logs/devcopilot.log*
```

```powershell
# Windows
(Select-String -Path "$env:LOCALAPPDATA\com.drewed.devcopilot\logs\devcopilot.log*" -Pattern csp_violation).Count
```

Esperado: `0` em todos os arquivos. Uma violação é achado de R1 e segue o caminho de T026 (hash específico +
ADR de exceção + `release-blocker`).

C2: capture as mesmas telas da referência em `evidence/csp-visual/<os>/` (macos, windows, linux) e compare
cada SO só com a sua referência.

## 7. O que guardar para T067–T070

As tarefas seguintes dependem destes registros. Anote-os enquanto executa.

- **ADR-004 (T067)**, Windows e Linux: o banco cifrado abre no build assinado (casos 2 e 4 passam depois de
  reabrir); a chave vai e volta pelo cofre real (Credential Manager, Secret Service); o comportamento de
  `secret_store_unavailable` (caso 13), com o trecho do log.
- **Limitações (T069)**: toda observação de permissão, compatibilidade, SmartScreen, WebKitGTK, proxy (P1) e
  Keychain, com o caso de origem.
- **Issues (T068)**: links em **Problemas abertos**. Depois de uma correção, o novo `rc.N` repete os casos
  afetados. Um caso mantido precisa de justificativa escrita de que a diferença de commits não o afeta.

## 8. Acessibilidade com teclado e leitor de tela (US4: T071–T073)

Registro na seção **Acessibilidade (US4)** de cada arquivo de evidência (passos A1–A10), com um par de
colunas por leitor. No Windows 11, Narrator **e** NVDA, cada um num percurso completo e separado (T072). No
Windows 10, só Narrator. No Linux, Orca nas duas sessões.

| Máquina | Leitor | Ligar | Ver o que foi falado |
|---------|--------|-------|----------------------|
| macOS | VoiceOver | ⌘F5 | Legendas do VoiceOver (Utilitário VoiceOver → Visual → Painel de legendas) |
| Windows | Narrator | Ctrl+Win+Enter | Anote na hora ou grave a tela com o áudio do sistema |
| Windows 11 | NVDA | Ctrl+Alt+N | Ferramentas → Visualizador de fala |
| Ubuntu | Orca | Super+Alt+S | Anote na hora ou grave a tela com o áudio do sistema |

**Anúncios esperados** (texto de `apps/desktop/src/i18n/pt-BR.json`, região “Estado da captura”):
`Capturando: <fontes>`, `Captura pausada: <fontes>` e `Sem captura`. O ícone (●, ❚❚, ○) é oculto do leitor
de propósito. Registre o texto literal em cada passo, mesmo quando passar, porque é a evidência de SC-016.

**Pontos de atenção** (vistos no código, ainda não confirmados em leitor real):

1. **Duas regiões vivas educadas** (`CaptureIndicator` e o anúncio do `UpdateBanner`). Se as duas mudarem
   juntas, um leitor pode engolir uma delas. Em A2–A5, confirme que o estado da captura foi falado, e não só
   o da atualização.
2. **Aviso de primeira execução** (`UpdateNotice`): é uma seção comum, sem região viva e sem foco
   automático. Em A8, registre como o usuário o encontrou (navegação por títulos, Tab) e se foi percebido sem
   olhar a tela. Se não foi percebido, abra issue `severity:high` com a combinação SO/leitor.
3. **Diálogo de confirmação** (`UpdateConfirmDialog`): usa `aria-modal` e prende o Tab, mas não marca o
   resto da página como `inert`. Em A10, teste também o cursor virtual (VoiceOver VO+setas, NVDA modo de
   navegação, Orca setas): se ele sai do diálogo para a página de trás, registre como ❌ com o leitor.

Defeito encontrado → issue com `platform:*` e severidade → correção em T074, com teste de regressão em
`apps/desktop/src/a11y.test.tsx`. Estado da captura não anunciado é `release-blocker`, salvo contorno
documentado aprovado pelo responsável.
