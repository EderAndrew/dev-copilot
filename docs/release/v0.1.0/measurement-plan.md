# Plano de medição v0.1.0

> Modelo (T011). Aprovado e congelado em T075, **antes** de qualquer resultado. Depois de aprovado, uma meta
> só muda por uma linha nova no histórico de alterações; linhas nunca são apagadas.
> Formato: `specs/002-desktop-release-readiness/contracts/release-evidence.md`.

- Aprovado por: · Data de aprovação: <anterior a qualquer resultado>

## Hardware

| SO | Perfil (referência/mínimo) | CPU | RAM | GPU | Observação |
|----|----------------------------|-----|-----|-----|------------|
| macOS | referência | | | | |
| macOS | mínimo | | | | |
| Windows | referência | | | | |
| Windows | mínimo | | | | |
| Linux | referência | | | | |
| Linux | mínimo | | | | |

## Metas

| Métrica | Meta | Hardware | Amostra mínima | Método |
|---------|------|----------|----------------|--------|
| Parcial (1º) após início da fala, p95 | ≤ 5 s | ref. | 100 trechos | `DEVCOPILOT_METRICS=1` + entrada virtual; `perf-sampler summarize --metrics` |
| Final após fim da fala, p95 | ≤ 15 s | ref. | 100 trechos | idem |
| Latência de controles (iniciar/pausar/parar), p95 | ≤ 500 ms | ref. e mín. | 20 por controle | cronômetro de eventos |
| Indicador desligado após parar, p95 | ≤ 1 s | ref. e mín. | 20 | idem |
| RSS após 2 h vs. 10 min | ≤ +10 % | ref. | 1 sessão de 2 h | `perf-sampler sample` + `summarize --samples --warmup-min 10` |
| CPU média sem captura | ≤ 2 % | ref. | 30 min | `perf-sampler` |
| CPU média capturando (modelo padrão) | ≤ 50 % da capacidade total | ref. | 2 h | `perf-sampler` |
| Análise: recall / grounding / injeção / Q&A | SC-008 / SC-010 / SC-011 da 001 | ref. | fixtures da 001 | harnesses da 001 + Ollama `<modelo>@<digest>` |

No hardware mínimo, as metas de latência de transcrição são **registradas, não exigidas**. Resultado fora da
meta vira limitação, com o modelo recomendado (spec SC-007).

## Condições

- Energia na tomada; sem outras cargas; 10 min de aquecimento.
- Modelo de transcrição: `<nome>` (o CSV de métricas registra o provedor, não o modelo).
- Ollama: versão `<versão>`; modelo `<nome>@<digest>`.
- Entrada de áudio: gravações não confidenciais lidas a partir de roteiros, tocadas em dispositivo virtual
  (BlackHole / VB-CABLE / `null-sink`), mais uma execução ao vivo por SO.

## Histórico de alterações (nunca apagar linhas)

| Data | Métrica | Meta anterior | Meta nova | Justificativa | Aprovado por |
|------|---------|---------------|-----------|---------------|--------------|
