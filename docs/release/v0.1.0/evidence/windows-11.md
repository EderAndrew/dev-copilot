# Evidências — windows-11 — candidato <commit curto>

> **Status: não executado.** Aguarda o candidato `v0.1.0-rc.1` (T057, `../candidate.md`) e a instalação limpa.
> Plataforma: Windows 11. Roteiro: [`../runbook-us3.md`](../runbook-us3.md). Formato:
> `specs/002-desktop-release-readiness/contracts/release-evidence.md`.
> Preenchido por: I1–I4 → T059 · C1, C2, matriz, medições, P1, W1 → T064 · U1–U4 → T047.
>
> Resultado: ✅ passou · ❌ falhou (link para a issue, com severidade) · ➖ não se aplica (motivo obrigatório).
> Use só áudio e texto de teste não confidenciais. Capturas sem credenciais nem conteúdo real (≤ 500 KB).

## Ambiente

| SO e versão | Hardware | Leitor(es) de tela | Pilha de áudio | Perfil limpo? | Executor | Data |
|-------------|----------|--------------------|----------------|---------------|----------|------|
| Windows 11 <versão> (build <…>) | x64 <modelo, RAM> | Narrator e NVDA <versão> | WASAPI | | | |

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
| C2 | Comparação visual com a referência (`csp-visual/baseline/windows/`) | | capturas em `csp-visual/windows/` |

## Matriz da 001 (casos 1–13, 3b, 3c, 11b)

Mesma numeração e mesmos resultados esperados de `specs/001-meeting-sessions/acceptance-matrix.md`.

| # | Caso (resumo) | Resultado | Evidência | Observação |
|---|---------------|-----------|-----------|------------|
| 1 | Permissão de microfone negada ao iniciar | | | Negar no primeiro pedido. Desligar o acesso global ao microfone é o caso W1 |
| 2 | Ciclo completo com pausa de 30 s | | |  |
| 3 | Microfone desconectado durante a captura | | |  |
| 3b | Fone Bluetooth conectado/desconectado durante a captura | | |  |
| 3c | Suspensão e bloqueio da sessão do SO durante a captura | | | Suspender (Iniciar → Energia → Suspender) e bloquear (Win+L) com a captura ativa |
| 4 | Encerramento forçado durante a captura | | | Gerenciador de Tarefas → Finalizar tarefa. Esperado: `failed (interrupted)`, finais preservados |
| 5 | Análise: confirmar requisito; sugestão conflitante | | |  |
| 6 | Prompt injection falado | | |  |
| 7 | Perguntas: presente e ausente na conversa | | |  |
| 8 | Provedor remoto: configurar, autorizar, revogar (com proxy) | | |  |
| 9 | Exportação Markdown e JSON | | |  |
| 10 | Exclusão | | |  |
| 11 | Só teclado: iniciar, parar, revisar, exportar | | | Detalhe por leitor na seção Acessibilidade (US4) |
| 11b | Leitor de tela anuncia o estado da captura | | | Narrator **e** NVDA, os dois. Detalhe por leitor na seção Acessibilidade (US4) |
| 12 | Áudio do sistema (indisponível com motivo onde S1 não aprovou) | | | Esperado em todas as plataformas: indisponível com motivo (Q1) |
| 13 | Cofre de credenciais indisponível | | | Provocar removendo a credencial `dev.devcopilot.desktop` com o banco existente, ou ➖ com motivo |

## Acessibilidade (US4)

Preenchido em T072. Só teclado, com o leitor de tela ligado e o monitor desligado ou coberto a partir de
A2. Copie o anúncio **literalmente** (o que o leitor falou ou mostrou no visualizador de fala). A9 e A10
precisam de uma versão mais nova no canal de teste e podem ser feitos na rodada de T047. Detalhes e
pontos de atenção: [`../runbook-us3.md`](../runbook-us3.md) §8.

Versões: Narrator <versão> · NVDA <versão>.

| # | Passo | Narrator: resultado | Narrator: anúncio literal | NVDA: resultado | NVDA: anúncio literal |
|---|---|---|---|---|---|
| A1 | Nova sessão → consentimento, só teclado | |  |  | |
| A2 | Iniciar: estado **capturando** anunciado | |  |  | |
| A3 | Pausar: estado **pausada** anunciado | |  |  | |
| A4 | Retomar: **capturando** anunciado de novo | |  |  | |
| A5 | Parar: **sem captura** anunciado | |  |  | |
| A6 | Revisar um registro da sessão salva | |  |  | |
| A7 | Exportar (até o diálogo de salvar) | |  |  | |
| A8 | Aviso de verificação de atualizações (primeira execução): encontrado, lido e dispensado | |  |  | |
| A9 | Banner “atualização disponível” anunciado; botão Instalar alcançável | |  |  | |
| A10 | Diálogo de confirmação: foco inicial em “Agora não”, Tab circula dentro, Esc fecha, foco volta ao botão | |  |  | |

Estado de captura não anunciado (A2–A5) é `release-blocker`, salvo contorno documentado aprovado pelo
responsável (T074).

## Medições cronometradas (matriz da 001)

Cinco tentativas por medição, cada uma com um participante que não conhece o app (SC-001) ou com o histórico
já populado com pelo menos 10 sessões de teste (SC-017). Registre cada tempo. Com 5 tentativas, “≥ 90 %” exige 5/5 dentro da
meta; com 10, exige 9/10. SC-006: 20 medições por controle, ✅ só com todas ≤ 500 ms.

| Medição | Critério | Tempos medidos | Resultado | Evidência |
|---------|----------|------------|-----------|-----------|
| Novo usuário cria e inicia uma sessão (app aberto → indicador "capturando") | ≤ 60 s em ≥ 90 % (SC-001) | | | |
| Encontrar e abrir uma sessão anterior pelo histórico | ≤ 30 s em ≥ 90 % (SC-017) | | | |
| Latência de controle iniciar/pausar/parar, com análise ou provedor lento | ≤ 500 ms (SC-006) | | | |

## Casos adicionais da plataforma (US3)

| # | Caso | Resultado | Evidência | Observação |
|---|------|-----------|-----------|------------|
| P1 | Primeiro download do modelo de transcrição atrás de rede restritiva ou proxy corporativo | | | O app explica a falha (sem travar nem mensagem genérica). Sem contorno → limitação. Ver roteiro §5 |
| W1 | Configuração de privacidade “Permitir que apps da área de trabalho acessem o microfone” desligada (Configurações → Privacidade e segurança → Microfone) | | | O app explica e orienta; nada é captado nem gravado |

## Atualização (US8)

| # | Caso | Resultado | Evidência |
|---|------|-----------|-----------|
| U1 | N → N+1 legítima, com confirmação | | |
| U2 | Pacote adulterado / outra chave / versão menor rejeitados | | |
| U3 | Verificação desligada: zero requisições (proxy) | | |
| U4 | Atualização bloqueada com sessão capturando/pausada | | |

## Limitações observadas

## Problemas abertos (links)
