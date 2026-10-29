# Permissões do sistema operacional

O que o Dev Copilot pede a cada SO, quando e por quê. Normas: spec 001 (FR-008–FR-014, FR-039a, FR-049),
research.md R1/R2/R9/R10, ADR-002 e ADR-004.

## Princípios

- Nenhuma permissão é pedida ao abrir o app. O prompt de captura só aparece quando o usuário inicia uma
  sessão que usa aquela fonte (FR-009); listar as fontes apenas consulta o estado.
- Permissão negada ou revogada interrompe a captura daquela fonte e mostra como reativá-la nas
  configurações do SO (FR-010). O app nunca troca de dispositivo silenciosamente (FR-011).
- A janela (webview) só tem a capability `core:default` (`apps/desktop/src-tauri/capabilities/default.json`):
  sem acesso a sistema de arquivos, shell, diálogos ou rede. A CSP não permite destinos remotos; toda rede
  ocorre no núcleo Rust, atrás da autorização de provedor (R9, R10).

## Por recurso

| Recurso | macOS | Windows | Linux | Quando |
|---------|-------|---------|-------|--------|
| Microfone | Privacidade e Segurança › Microfone (`NSMicrophoneUsageDescription` em `Info.plist`) | Privacidade e segurança › Microfone, com "permitir apps da área de trabalho" | sem prompt do SO; depende do PipeWire/PulseAudio e, em sandbox (Flatpak/Snap), da permissão de microfone | ao iniciar a primeira sessão com microfone |
| Áudio do sistema | **indisponível** — não validado (ADR-002, spike S1 pendente) | **indisponível** — não validado | **indisponível** — não validado | — (a opção aparece desativada com o motivo) |
| Cofre de credenciais | Keychain (pode pedir a senha do usuário para liberar o item) | Credential Manager | Secret Service (GNOME Keyring ou KWallet ativo e desbloqueado) | ao abrir o banco e ao salvar chave de API |
| Rede | sem prompt | sem prompt; o firewall pode perguntar | sem prompt | só para baixar modelo local (ação explícita) e para provedores remotos autorizados |
| Arquivos | diálogo de salvar nativo, aberto pelo núcleo | idem | idem | só ao exportar; grava apenas no caminho escolhido |

Sem cofre de credenciais utilizável, o app não guarda nada e não inicia captura (FR-039a); a mensagem explica o
motivo.

Quando a captura do áudio do sistema for aprovada em uma plataforma, esta tabela, o `Info.plist`/entitlements
(macOS) e a orientação de permissão (`permissionHelp.<so>.system`) serão atualizados junto com o adaptador.

## Orientação mostrada ao usuário

As mensagens ficam em `apps/desktop/src/i18n/{pt-BR,en}.json` sob `permissionHelp.<so>.<fonte>` e são
escolhidas pelo núcleo conforme o SO (`audio_permission_help`).
