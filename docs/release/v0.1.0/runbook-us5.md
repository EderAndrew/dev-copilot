# Roteiro de medição — US5 (T075–T082)

Como executar a Fase 8 de `specs/002-desktop-release-readiness/tasks.md`. Este roteiro **não** é um
resultado. Os valores vão para `measurements.md`, `docs/adr/ADR-003.md` e `limitations.md`.

## 1. Ordem e pré-condições

1. **T075 primeiro.** Preencha `measurement-plan.md` (hardware, metas, condições, modelos, amostras) e faça um
   commit que altere **só** esse arquivo. A data desse commit precisa ser anterior a qualquer resultado. Uma
   medição feita antes dele não vale e precisa ser refeita.
2. T076 (conjunto de fala) pode correr em paralelo com T075, mas os SHA-256 entram no plano **antes** da
   aprovação. Se o conjunto mudar depois, registre uma linha no histórico.
3. T077–T080 rodam no candidato assinado `v0.1.0-rc.N` instalado (T057–T061), nunca num build de dev.
4. T081 roda em qualquer máquina com Ollama, a partir do commit do candidato.
5. T082 vem por último.

Para cada máquina, registre em `measurements.md`: hardware igual ao do plano (referência ou mínimo), versão
exata do SO e o candidato com o commit.

## 2. Conjunto de fala (T076)

- Pelo menos 100 trechos no total, em pt-BR e en. Use roteiros técnicos escritos (requisitos, decisões,
  nomes de tecnologias), sem nenhum conteúdo de reunião real nem dado pessoal.
- Um trecho é uma fala contínua de 3 a 20 s, separada por pelo menos 2 s de silêncio. Assim cada trecho vira
  um `speech_start_ms` próprio no CSV de métricas.
- Formato: WAV 16 bits, 48 kHz ou 16 kHz, mono. Normalize o volume entre os arquivos.
- SHA-256 de cada arquivo:
  - macOS: `shasum -a 256 *.wav`
  - Windows: `Get-FileHash *.wav -Algorithm SHA256`
  - Linux: `sha256sum *.wav`
- Anexe os arquivos ao release em rascunho e liste-os em `measurement-plan.md` §Conjunto de fala.

## 3. Entrada virtual por SO

O app capta só o microfone (0.1.0). O áudio tocado precisa chegar a ele como um dispositivo de entrada.

| SO | Ferramenta | Roteamento |
|----|------------|------------|
| macOS | BlackHole 2ch | Saída do player → BlackHole; no app, escolha "BlackHole 2ch" como entrada |
| Windows | VB-CABLE | Saída do player → "CABLE Input"; no app, escolha "CABLE Output" como entrada |
| Linux | PipeWire/Pulse | `pactl load-module module-null-sink sink_name=devcopilot_eval`; player → `devcopilot_eval`; no app, escolha "Monitor of devcopilot_eval" |

Antes da série, confira o roteamento: toque um trecho, veja o parcial aparecer e anote em
`measurements.md` §Método.

## 4. Latência de transcrição (T077–T079)

1. Feche o app. Abra-o com o modo de medição ligado (a variável só vale na inicialização):
   - macOS: `open -n --env DEVCOPILOT_METRICS=1 -a "Dev Copilot"`. Se o `open` não aceitar `--env`, rode
     direto o executável em `"/Applications/Dev Copilot.app/Contents/MacOS/"`, com a variável no shell.
   - Windows (PowerShell): `$env:DEVCOPILOT_METRICS = "1"` e, na mesma janela, rode o `.exe` instalado (o
     destino do atalho do Menu Iniciar).
   - Linux: `DEVCOPILOT_METRICS=1 <binário do .deb ou AppImage>`.
2. Confira o modelo de transcrição: precisa ser o do plano.
3. Inicie uma sessão e toque o conjunto inteiro. Pare a sessão. Depois faça uma execução ao vivo (alguém lê
   um roteiro no microfone real) numa sessão separada.
4. O CSV fica na pasta de logs (`docs/local-data.md`) como `metrics-AAAA-MM-DD.csv` (data UTC). Copie-o para
   fora logo, porque a limpeza apaga arquivos com mais de 7 dias. Se a série cruzar a meia-noite UTC, junte os
   dois arquivos mantendo um só cabeçalho.
5. Separe o conjunto e a execução ao vivo em CSVs diferentes. O CSV não tem id de sessão, então anote o horário
   de cada sessão, ou use um arquivo por dia ou por abertura do app.
6. `cargo run -p perf-sampler --release -- summarize --metrics <csv>` imprime `count`, `p50`, `p95` e `max`
   do primeiro parcial e do final. `count` do conjunto deve ficar perto do número de trechos. Uma diferença
   grande indica trechos colados (pausa curta demais) ou divididos; registre-a em §Desvios.
7. Registre em `measurements.md` §Transcrição e na tabela "Resultados do spike S2" de `docs/adr/ADR-003.md`.
   Nessa tabela, substitua "a medir" pelos valores reais.

No hardware mínimo as metas de latência são registradas, não exigidas (plano). Um resultado fora da meta ali
vira `limitação` com o modelo recomendado.

## 5. CPU e memória (T080)

O Tauri roda a interface num processo separado. Amostre os dois:

| SO | Processo principal | Processo da WebView |
|----|--------------------|---------------------|
| macOS | o do app (Monitor de Atividade) | `com.apple.WebKit.WebContent` filho do app |
| Windows | o `.exe` do app | `msedgewebview2.exe` do app (vários; use o de maior memória com o app aberto, conferindo a linha de comando no Gerenciador de Tarefas → Detalhes) |
| Linux | o binário do app | `WebKitWebProcess` filho do app |

Ache os PIDs (macOS/Linux: `pgrep -lf -i copilot` e `ps -o pid,ppid,comm -A | grep -i webkit`; Windows:
Gerenciador de Tarefas → Detalhes, coluna PID). Prefira `--pid`, porque `--process` exige um nome exato e único.

```sh
cargo build -p perf-sampler --release
target/release/perf-sampler sample --pid <principal> --interval 5 --out main-<so>-<cenario>.csv
target/release/perf-sampler sample --pid <webview>   --interval 5 --out webview-<so>-<cenario>.csv
```

- Impeça o repouso durante as rodadas: macOS `caffeinate -dims`; Windows `powercfg /change standby-timeout-ac 0`
  (restaure depois); Linux `systemd-inhibit --what=idle:sleep sleep 3h`.
- **Sem captura (30 min):** app aberto na tela inicial, sem sessão. Deixe o aquecimento de 10 min antes de
  iniciar o amostrador ou use `--warmup-min 10` no resumo.
- **Capturando (2 h):** sessão com transcrição local, o conjunto de fala em loop na entrada virtual. Comece a
  amostrar junto com a sessão. O resumo com `--warmup-min 10` compara o RSS final com o valor de 10 minutos.
- Resumo: `perf-sampler summarize --samples <csv> --warmup-min 10`, para cada CSV. A CPU do app é a soma das
  médias dos dois processos, porque cada uma já é uma fração da capacidade total. Para o RSS, registre cada
  processo e a soma.
- Anote travamentos, reinícios ou avisos do SO durante as 2 h (SC-008).
- Anexe os CSVs ao release em rascunho e registre o SHA-256 de cada um.

## 6. Controles e indicador (T080)

Medidos no hardware de referência **e** no mínimo de cada SO, com 20 medições por controle.

1. Grave a tela a ≥ 60 fps: macOS QuickTime (⇧⌘5); Windows Barra de Jogo (Win+Alt+R, 60 fps nas
   configurações) ou OBS; Linux OBS.
2. Com uma sessão capturando e a transcrição local ativa, para que a CPU esteja ocupada:
   - **iniciar:** do clique em iniciar até o indicador mostrar "capturando";
   - **pausar:** do clique até o indicador mostrar "pausado";
   - **parar:** do clique até o indicador sair de "capturando";
   - **indicador desligado:** do clique em parar até o indicador de captura sumir por completo.
3. Num editor de vídeo quadro a quadro, conte os quadros entre o primeiro quadro com o botão pressionado e o
   primeiro quadro com o novo estado. Latência = quadros × 1000/fps ms.
4. O log (`devcopilot.log.*`) registra `session capturing`, `session paused` e `session stopping` com horário.
   Use-o só para conferir a ordem dos eventos; ele não marca o clique.
5. Registre p50/p95/máx em `measurements.md` §Controles e indicador. Pelo plano, o resultado está ok quando o p95
   fica dentro da meta. Pelo modelo de evidência da US3 (SC-006), só fica ✅ se as 20 medições estiverem dentro
   dela. Se esses dois critérios divergirem, registre os dois e decida em T082.

## 7. Análise com provedor real (T081)

1. Instale a versão do Ollama do plano e baixe o modelo. Confira o digest com `ollama list` (coluna ID) e
   compare com o do plano.
2. No commit do candidato:

   ```sh
   DEVCOPILOT_EVAL_PROVIDER=ollama DEVCOPILOT_EVAL_MODEL=<modelo> \
     cargo test -p devcopilot-intelligence --release \
     --test evaluation --test qa_evaluation --test prompt_injection -- --nocapture
   ```

   O endpoint padrão é `http://127.0.0.1:11434`; para mudá-lo, use `DEVCOPILOT_EVAL_ENDPOINT`. Só endpoints de
   loopback são aceitos.
3. Copie as linhas `eval <transcrição> model=…` e `qa eval <transcrição> model=…` para `measurements.md`
   §Análise e para a tabela do provedor de referência em `docs/adr/ADR-003.md`. Os testes medem todas as
   transcrições antes de falhar: um teste vermelho continua fornecendo os valores de cada uma.
4. `provider error` numa linha é erro do provedor (Ollama parado, modelo ausente, timeout). Registre-o como
   tal; nunca como aprovação. Corrija o ambiente e rode de novo.
5. `prompt_injection` não chama provedor: ele reproduz saídas de um modelo totalmente comprometido. Registre o
   resultado do commit como independente do modelo.
6. Se a variável estiver ausente, os testes de referência só imprimem "skipped" e passam. Confira que as
   linhas `eval …` apareceram antes de registrar qualquer coisa.

## 8. Classificação (T082)

Para cada linha de `measurements.md`:

- **ok**: dentro da meta.
- **limitação**: fora da meta onde ela é registrada e não exigida (latência no hardware mínimo), ou com um
  contorno aceito. Adicione-a a `limitations.md` com o modelo ou o perfil de hardware recomendado (por
  exemplo: "modelo `small` não recomendado abaixo de 16 GB").
- **bloqueador**: fora de uma meta exigida. Abra uma issue com `release-0.1.0`, `severity:*`, `platform:*` e
  `release-blocker`, e coloque o link na linha.

Para mudar uma meta depois de T075, adicione uma linha no histórico de `measurement-plan.md` com data,
justificativa e aprovador. Nunca edite a meta original.

## 9. Saída

- [ ] `measurement-plan.md` aprovado num commit só dele, anterior a todo resultado.
- [ ] Toda métrica com valor ou "não medido" com motivo, nos três SOs.
- [ ] Toda linha classificada; toda `limitação` em `limitations.md`; todo `bloqueador` com uma issue.
- [ ] ADR-003: "Resultados do spike S2" sem "a medir" e tabela do provedor de referência preenchida.
- [ ] CSVs e conjunto de fala anexados ao rascunho, com SHA-256 citados.
- [ ] Item 6 de `docs/release/v0.1.0/checklist.md` pronto para T094.
