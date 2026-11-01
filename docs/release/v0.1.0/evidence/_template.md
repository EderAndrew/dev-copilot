# Evidências — <plataforma> — candidato <commit curto>

> Modelo (T011). Copie para `evidence/<plataforma>.md`: `macos-arm64`, `macos-x64` (se declarado),
> `windows-10`, `windows-11`, `linux-wayland`, `linux-x11`. Formato:
> `specs/002-desktop-release-readiness/contracts/release-evidence.md`.
>
> Resultado: ✅ passou · ❌ falhou (link para a issue, com severidade) · ➖ não se aplica (motivo obrigatório).
> Use só áudio e texto de teste não confidenciais. Capturas sem credenciais nem conteúdo real (≤ 500 KB).

## Ambiente

| SO e versão | Hardware | Leitor(es) de tela | Pilha de áudio | Perfil limpo? | Executor | Data |
|-------------|----------|--------------------|----------------|---------------|----------|------|
| | | | | | | |

## Instalação e verificação (US1)

| # | Caso | Resultado | Evidência |
|---|------|-----------|-----------|
| I1 | Verificação de assinatura e checksum | | anexo |
| I2 | Instalação sem contornar aviso de segurança | | |
| I3 | Artefato adulterado rejeitado | | |
| I4 | Primeiro pedido de permissão de microfone no build assinado | | |

## Política de conteúdo (US2)

| # | Caso | Resultado | Evidência |
|---|------|-----------|-----------|
| C1 | Percurso de todas as telas com zero `csp_violation` no log | | trecho do log |
| C2 | Comparação visual com a referência | | capturas |

## Matriz da 001 (casos 1–13, 3b, 3c, 11b)

Mesma numeração e mesmos resultados esperados de `specs/001-meeting-sessions/acceptance-matrix.md`.

| # | Caso (resumo) | Resultado | Evidência | Observação |
|---|---------------|-----------|-----------|------------|
| 1 | Permissão de microfone negada ao iniciar | | | |
| 2 | Ciclo completo com pausa de 30 s | | | |
| 3 | Microfone desconectado durante a captura | | | |
| 3b | Fone Bluetooth conectado/desconectado durante a captura | | | |
| 3c | Suspensão e bloqueio da sessão do SO durante a captura | | | |
| 4 | Encerramento forçado durante a captura | | | |
| 5 | Análise: confirmar requisito; sugestão conflitante | | | |
| 6 | Prompt injection falado | | | |
| 7 | Perguntas: presente e ausente na conversa | | | |
| 8 | Provedor remoto: configurar, autorizar, revogar (com proxy) | | | |
| 9 | Exportação Markdown e JSON | | | |
| 10 | Exclusão | | | |
| 11 | Só teclado: iniciar, parar, revisar, exportar | | | |
| 11b | Leitor de tela anuncia o estado da captura | | | |
| 12 | Áudio do sistema (indisponível com motivo onde S1 não aprovou) | | | |
| 13 | Cofre de credenciais indisponível | | | |

## Atualização (US8)

| # | Caso | Resultado | Evidência |
|---|------|-----------|-----------|
| U1 | N → N+1 legítima, com confirmação | | |
| U2 | Pacote adulterado / outra chave / versão menor rejeitados | | |
| U3 | Verificação desligada: zero requisições (proxy) | | |
| U4 | Atualização bloqueada com sessão capturando/pausada | | |

## Limitações observadas

## Problemas abertos (links)
