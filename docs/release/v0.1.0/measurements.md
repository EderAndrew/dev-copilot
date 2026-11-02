# Medições v0.1.0

> Modelo (T011). Preenchido em T077–T081, só depois de `measurement-plan.md` aprovado. Toda linha cita o
> candidato, o hardware do plano e o arquivo bruto (CSV anexado ao release em rascunho, com SHA-256).
> Procedimento: `runbook-us5.md`.

- Candidato: `v0.1.0-rc.N` · Commit: `<sha curto>`
- Plano aplicado: `measurement-plan.md` aprovado em `<data>` (commit `<sha>`)

Classificação (T082, spec FR-022):

- **ok**: dentro da meta.
- **limitação**: fora da meta em hardware mínimo, ou meta registrada e não exigida. Vai para
  `limitations.md` com o modelo ou o perfil de hardware recomendado.
- **bloqueador**: fora de uma meta exigida. Abra uma issue `release-blocker` e coloque o link aqui.

Uma métrica sem valor leva "não medido" e o motivo; nunca fica em branco. Uma meta mudada depois da aprovação
exige uma linha no histórico de `measurement-plan.md`.

## Método

Preencha antes da primeira medição em cada SO (T076).

| SO | Entrada virtual | Configuração | Verificação do roteamento |
|----|-----------------|--------------|---------------------------|
| macOS | BlackHole 2ch `<versão>` | | |
| Windows | VB-CABLE `<versão>` | | |
| Linux | `null-sink` do PipeWire/Pulse + monitor | | |

- Reprodução do conjunto de fala: `<ferramenta e comando>`, com pausa de `<s>` entre trechos.
- Execução ao vivo: `<quem leu, qual roteiro, microfone>`.
- Gravação de tela para os controles: `<ferramenta, fps>`.

## Transcrição

Saída de `perf-sampler summarize --metrics <csv>` (latências em ms). Primeiro parcial = `emitted −
speech_start` do parcial mais cedo de cada trecho; final = `emitted − speech_end`.

| SO | Hardware | Fonte | Amostra | Parcial (1º) p50 / p95 / máx | Final p50 / p95 / máx | Classificação | CSV (SHA-256) |
|----|----------|-------|---------|------------------------------|-----------------------|---------------|---------------|
| | ref. | conjunto | | | | | |
| | ref. | ao vivo | | | | | |
| | mín. | conjunto | | | | | |

## Controles e indicador

| SO | Hardware | Controle | Amostra | p50 / p95 / máx (ms) | Meta | Classificação | Evidência |
|----|----------|----------|---------|----------------------|------|---------------|-----------|
| | | iniciar | | | ≤ 500 | | |
| | | pausar | | | ≤ 500 | | |
| | | parar | | | ≤ 500 | | |
| | | indicador desligado | | | ≤ 1000 | | |

## CPU e memória

Saída de `perf-sampler summarize --samples <csv> --warmup-min 10`, por processo. App = principal + WebView.

| SO | Hardware | Cenário | Duração | Processo | CPU média (% do total) | RSS 10 min → final (crescimento) | Classificação | CSV (SHA-256) |
|----|----------|---------|---------|----------|------------------------|----------------------------------|---------------|---------------|
| | | sem captura | 30 min | principal | | — | | |
| | | sem captura | 30 min | WebView | | — | | |
| | | capturando | 2 h | principal | | | | |
| | | capturando | 2 h | WebView | | | | |

Travamentos ou reinícios durante as 2 h (SC-008): ·

## Análise com provedor real (Ollama)

Modelo `<nome>@<digest>`, Ollama `<versão>`, máquina `<hardware>`. Copie as linhas `eval …` e `qa eval …`
impressas pelo teste. Um erro do provedor é registrado como erro, nunca como aprovação.

| Harness | Transcrição | Métrica | Meta | Resultado | Classificação |
|---------|-------------|---------|------|-----------|---------------|
| `evaluation.rs` | `export-planning-pt` | recall / grounding / inventados | ≥ 0,80 / 1,00 / — | | |
| `evaluation.rs` | `queue-migration-en` | recall / grounding / inventados | ≥ 0,80 / 1,00 / — | | |
| `qa_evaluation.rs` | `export-planning-pt` | citada ou declarada / inventadas / acerto | 1,00 / 0 / informativo | | |
| `qa_evaluation.rs` | `queue-migration-en` | citada ou declarada / inventadas / acerto | 1,00 / 0 / informativo | | |
| `prompt_injection.rs` | todos os casos | mudanças de status, edições ou exclusões indevidas | 0 | | |

`prompt_injection.rs` reproduz as saídas de um modelo totalmente comprometido e não chama provedor. O
resultado vale para qualquer modelo; registre o commit em que rodou.

## Desvios do plano

Qualquer condição do plano não cumprida (carga extra, bateria, aquecimento menor), com o impacto esperado.
