# Medições v0.1.0

> Modelo (T011). Preenchido em T077–T081, só depois de `measurement-plan.md` aprovado. Toda linha cita o
> candidato, o hardware do plano e o arquivo bruto (CSV anexado ao release em rascunho, com SHA-256).

- Candidato: `v0.1.0-rc.N` · Commit: `<sha curto>`
- Plano aplicado: `measurement-plan.md` aprovado em `<data>`

Classificação: **atende** · **não atende** (link para a issue, com severidade) · **limitação** (fora da meta
em hardware mínimo, ou meta registrada e não exigida; vai para `limitations.md` com o modelo recomendado).

## Transcrição

Saída de `perf-sampler summarize --metrics <csv>` (latências em ms).

| SO | Hardware | Amostra | Parcial (1º) p50 / p95 / máx | Final p50 / p95 / máx | Classificação | CSV (SHA-256) |
|----|----------|---------|------------------------------|-----------------------|---------------|---------------|
| | | | | | | |

## Controles e indicador

| SO | Hardware | Controle | Amostra | p50 / p95 / máx (ms) | Meta | Classificação | Evidência |
|----|----------|----------|---------|----------------------|------|---------------|-----------|
| | | iniciar | | | ≤ 500 | | |
| | | pausar | | | ≤ 500 | | |
| | | parar | | | ≤ 500 | | |
| | | indicador desligado | | | ≤ 1000 | | |

## CPU e memória

Saída de `perf-sampler summarize --samples <csv> --warmup-min 10`.

| SO | Hardware | Cenário | Duração | CPU média (% do total) | RSS 10 min → final (crescimento) | Classificação | CSV (SHA-256) |
|----|----------|---------|---------|------------------------|----------------------------------|---------------|---------------|
| | | sem captura | 30 min | | — | | |
| | | capturando | 2 h | | | | |

## Análise com provedor real (Ollama)

| Harness | Métrica | Meta | Resultado | Classificação | Saída |
|---------|---------|------|-----------|---------------|-------|
| `evaluation.rs` | recall / grounding | SC-008 | | | |
| `prompt_injection.rs` | mudanças de status indevidas | SC-010 | | | |
| `qa_evaluation.rs` | citação vs. "sem evidência" | SC-011 | | | |

## Desvios do plano

Qualquer condição do plano não cumprida (carga extra, bateria, aquecimento menor), com o impacto esperado.
