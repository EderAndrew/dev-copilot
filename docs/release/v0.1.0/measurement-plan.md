# Plano de medição v0.1.0

> Modelo (T011). Aprovado e congelado em T075, **antes** de qualquer resultado. Depois de aprovado, uma meta
> só muda por uma linha nova no histórico de alterações; linhas nunca são apagadas.
> Formato: `specs/002-desktop-release-readiness/contracts/release-evidence.md`.

- Aprovado por: · Data de aprovação: <anterior a qualquer resultado>
- Commit de aprovação: `<sha>`. O commit altera só este arquivo, e a data dele precede a de qualquer resultado
  em `measurements.md`.

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
- Modelo de transcrição: `<nome>`, arquivo com SHA-256 `<hash>` (o CSV de métricas registra o provedor, não o
  modelo).
- Ollama: versão `<versão>`; modelo `<nome>@<digest>` (`ollama show <nome> --modelfile` ou `ollama list`).
- Entrada de áudio: gravações não confidenciais lidas a partir de roteiros, tocadas em dispositivo virtual
  (BlackHole / VB-CABLE / `null-sink`), mais uma execução ao vivo por SO.
- Processos amostrados: o processo principal **e** o da WebView (WebKit no macOS/Linux, `msedgewebview2` no
  Windows), cada um com seu CSV. CPU e RSS do app = soma dos dois (`runbook-us5.md` §5).
- Controles e indicador: gravação de tela a ≥ 60 fps; latência = quadros entre o clique (botão pressionado)
  e a mudança do indicador, × duração do quadro. Resolução ≈ 17 ms (`runbook-us5.md` §6).
- Um resultado sem medição é registrado como "não medido" com o motivo; nunca fica em branco.

## Conjunto de fala (T076)

Fora do repositório, anexado ao release em rascunho. Roteiros técnicos escritos, lidos pela equipe; nenhum
conteúdo de reunião real.

| Arquivo | Idioma | Leitor (iniciais) | Trechos | Duração | SHA-256 |
|---------|--------|-------------------|---------|---------|---------|
| | pt-BR | | | | |
| | en | | | | |

Total de trechos (mínimo 100, nos dois idiomas): ·

## Histórico de alterações (nunca apagar linhas)

| Data | Métrica | Meta anterior | Meta nova | Justificativa | Aprovado por |
|------|---------|---------------|-----------|---------------|--------------|
