# Research: Sessões de reunião e assistência técnica incremental

**Feature**: `001-meeting-sessions` | **Date**: 2026-10-27 | **Spec**: [spec.md](./spec.md)

Cada decisão segue o formato Decisão / Justificativa / Alternativas. Itens marcados **[SPIKE]** dependem de
validação em dispositivo real antes de serem considerados viáveis (Constituição Art. V.2: build passando não
prova captura). Versões exatas de dependências são fixadas na implementação, com revisão de manutenção,
licença e advisories (Art. VII.6, `cargo deny`).

---

## R1. Captura de microfone (P1, todos os SOs)

- **Decisão**: crate `cpal` como adaptador único de microfone para macOS (CoreAudio), Windows (WASAPI) e Linux
  (ALSA, alcançando PulseAudio/PipeWire pelas camadas de compatibilidade). O callback de áudio só copia
  amostras para um ring buffer SPSC limitado (`rtrb`); reamostragem para 16 kHz mono (`rubato`) ocorre fora
  do callback.
- **Justificativa**: uma única implementação multiplataforma madura para o caso obrigatório; callback sem
  alocação, lock ou I/O atende Art. V.3. A permissão de microfone no macOS é disparada pelo SO na abertura do
  stream e exige `NSMicrophoneUsageDescription` no `Info.plist`.
- **Alternativas**: bindings diretos por SO (mais código e `unsafe` sem ganho para microfone); `portaudio`
  (dependência C adicional, sem vantagem).

## R2. Captura de áudio do sistema (P2, dependente de viabilidade) **[SPIKE]**

- **Decisão**: um adaptador por SO atrás da mesma porta `AudioSource`, cada um habilitado somente após spike
  aprovado e registrado em **ADR-002**:
  - **macOS**: avaliar Core Audio process taps (macOS 14.4+, permissão de gravação de áudio do sistema) versus
    ScreenCaptureKit (macOS 13+, permissão de gravação de tela). Ambos exigem FFI Objective-C (`objc2`), com
    `unsafe` isolado, `// SAFETY:` e testes focados (Art. I.4).
  - **Windows**: WASAPI loopback no dispositivo de saída padrão (suportado pelo `cpal` ao abrir stream de
    entrada em dispositivo de saída); validar comportamento com troca de dispositivo.
  - **Linux**: fonte "monitor" do PulseAudio/PipeWire; avaliar seleção via `libpulse-binding` versus API
    nativa do PipeWire; validar em sessões Wayland e X11.
- **Justificativa**: capacidades, permissões e versões mínimas variam; não se assume equivalência (spec,
  Platform Impact). A opção fica visível como indisponível com motivo onde não validada (FR-014).
- **Alternativas**: drivers virtuais de áudio (instalação privilegiada, viola menor privilégio); captura via
  plataformas de reunião (fora de escopo).

## R3. Pipeline de áudio, concorrência e cancelamento

- **Decisão**: `tokio` (já usado pelo Tauri) com `tokio_util::sync::CancellationToken` por sessão.
  Fluxo: callback → `rtrb` (capacidade ~2 s) → tarefa de reamostragem/mixagem → `mpsc` limitado de chunks
  (capacidade ~30 s) → worker de transcrição → eventos de trecho → agendador de análise.
  Política de sobrecarga: ao encher, descartar os chunks mais antigos e emitir `Gap{reason: overload}`
  (FR-018, FR-019). Parar = cancelar token, fechar streams imediatamente, dar até 5 s para drenar a
  transcrição em curso, depois abortar e emitir lacuna (SC-002).
- **Justificativa**: buffers limitados e política explícita (Art. V.3); controles nunca esperam por provedor
  (SC-006) porque comandos só sinalizam o token e retornam.
- **Alternativas**: threads dedicadas com canais `crossbeam` (viável, mas duplicaria o runtime do Tauri);
  canais ilimitados (proibidos pelo Art. V.3).

## R4. Transcrição local **[SPIKE de desempenho]**

- **Decisão**: `whisper-rs` (bindings do whisper.cpp) como provedor local padrão onde viável. Streaming por
  janela deslizante: o trecho corrente é retranscrito como **parcial** a cada ~2 s; vira **final** quando um
  detector de atividade de voz indica pausa ou a janela atinge ~25 s. Aceleração por GPU (Metal/CUDA/Vulkan)
  é opcional e decidida por plataforma. Modelos **não** são embutidos: o usuário escolhe e baixa um modelo por
  ação explícita; o download (que não envia conteúdo de sessão) mostra origem e tamanho e verifica hash
  SHA-256 fixado no aplicativo.
- **Justificativa**: transcrição offline sem envio de conteúdo (local-first); multiplataforma; viabilidade de
  SC-005 depende do hardware de referência — o spike mede latência parcial/final por SO e define o modelo
  padrão recomendado. Se inviável em um SO, o caminho remoto autorizado cobre FR-045.
- **Alternativas**: Vosk (qualidade inferior em pt-BR técnico); APIs de reconhecimento do SO (cobertura e
  privacidade desiguais entre SOs); ONNX Runtime com modelos próprios (mais integração, sem ganho claro).
- Registrar em **ADR-003** (provedores de transcrição e IA).

## R5. Transcrição remota

- **Decisão**: adaptador HTTP para APIs de transcrição compatíveis com o formato OpenAI
  (`/audio/transcriptions`), com endpoint e modelo configuráveis. Envia apenas chunks de áudio da sessão
  ativa; categoria de dados declarada: "áudio". Sem streaming bidirecional no MVP (chunks finais de ~10–25 s;
  parciais não são exibidos no modo remoto, o que é informado ao usuário).
- **Justificativa**: um formato amplamente suportado cobre vários provedores com um adaptador.
- **Alternativas**: SDKs específicos por fornecedor (acoplamento, mais dependências).

## R6. Análise e perguntas (LLM)

- **Decisão**: porta `AnalysisProvider` com dois adaptadores iniciais:
  - **Remoto**: Anthropic Messages API (modelo configurável) e, opcionalmente, endpoint compatível com
    OpenAI Chat Completions.
  - **Local**: servidor Ollama já instalado e iniciado pelo usuário (`127.0.0.1`), tratado como provedor
    local; o Dev Copilot não inicia nem embute servidores (Art. IV.5).
  Saída estruturada: o prompt exige JSON conforme schema versionado
  ([contracts/analysis-output.schema.json](./contracts/analysis-output.schema.json)); a resposta é validada e
  convertida em propostas. Saída inválida é descartada com aviso (FR-030). Nenhuma ferramenta/função é
  exposta ao modelo (Art. IV.14).
- **Justificativa**: portas mantêm tipos de fornecedor fora do domínio (Art. VI.2); ausência de ferramentas e
  conversão estrita por schema neutralizam prompt injection no nível de ação (FR-029).
- **Alternativas**: modelo local embutido (`llama.cpp` via bindings): distribuição de modelos grandes e
  desempenho incertos — fica para feature futura após ADR.

## R7. Agendamento, deduplicação e limites da análise (Q-05)

- **Decisão**: análise incremental disparada a cada 6 trechos finais novos ou 90 s de fala, com intervalo
  mínimo de 30 s, no máximo 1 chamada em voo por sessão e teto configurável de chamadas/hora (padrão 40) e de
  tokens por sessão (padrão configurável por provedor). O contexto enviado é: trechos novos + resumo dos
  registros existentes (ids, tipo, texto curto, status). O modelo pode indicar `relates_to` a um id existente;
  o domínio cria proposta conflitante (FR-025) ou marca duplicata (FR-027) — nunca altera o existente.
  Deduplicação local adicional por similaridade de texto normalizado.
- **Justificativa**: custo e taxa limitados (Art. VI.3, FR-028) com valores ajustáveis pelo usuário.
- **Alternativas**: análise por trecho (custo alto); análise só no fim (não é incremental).

## R8. Persistência e criptografia em repouso

- **Decisão**: SQLite via `rusqlite` compilado com SQLCipher (feature `bundled-sqlcipher`), banco inteiro
  cifrado com chave aleatória de 256 bits gerada na primeira execução e guardada no cofre do SO via crate
  `keyring` (Keychain no macOS, Credential Manager no Windows, Secret Service no Linux). Sem cofre disponível →
  nenhuma gravação e captura bloqueada (FR-039a). Migrações versionadas por `PRAGMA user_version` em código.
  `PRAGMA secure_delete=ON`, `journal_mode=WAL` com checkpoint após exclusão e `VACUUM` sob demanda.
  Arquivo em diretório de dados do app (`app_data_dir`) com permissões apenas do usuário.
- **Justificativa**: atende Art. IV.12 (chave fora dos dados, não embutida) e FR-039a com um único mecanismo
  verificável por SC-018. Limitações de exclusão segura (SSD, snapshots, backups do SO) são exibidas ao usuário
  (FR-042).
- **Alternativas**: cifrar campo a campo na aplicação (impede consultas, mais código de cripto próprio);
  chave por sessão para "crypto-shredding" (melhor exclusão, mas complexidade de gestão de chaves — reavaliar
  em ADR futuro); depender da criptografia de disco do SO (rejeitado na clarificação Q-04).
- Registrar em **ADR-004** (armazenamento local cifrado).

## R9. Credenciais e autorização de provedores

- **Decisão**: chaves de API no cofre do SO (`keyring`), referenciadas por id; nunca no banco, logs ou
  exportação (FR-049). Autorização de provedor remoto persistida como registro com destino, categorias,
  texto exibido, data e estado (concedida/revogada). Todo acesso de rede passa por um `RemoteGateway` que
  verifica a autorização vigente antes de cada requisição; clientes HTTP (`reqwest` + `rustls`, TLS
  verificado) só existem dentro dos adaptadores remotos.
- **Justificativa**: ponto único de enforcement facilita SC-013 (teste com servidor fake + asserção de zero
  requisições sem autorização).
- **Alternativas**: verificação na UI (proibida: autorização deve ser independente do frontend, Art. IV.11).

## R10. Fronteira Tauri (IPC, capabilities, CSP)

- **Decisão**: comandos Tauri finos que desserializam DTOs, validam (tamanhos, enums, ids) e delegam para
  casos de uso; eventos de sessão via `tauri::ipc::Channel` por assinatura. Sem plugins de shell, fs ou http
  no frontend. Exportação usa `tauri-plugin-dialog` apenas para obter o caminho de salvamento
  (`dialog:allow-save`); a escrita é feita no Rust somente no caminho retornado. Remover
  `tauri-plugin-opener` se não houver uso. CSP restrita já configurada; `connect-src` não inclui destinos
  remotos (a rede ocorre só no núcleo).
- **Justificativa**: Art. IV.5, IV.11, VII.4.
- **Alternativas**: `tauri-plugin-fs` com escopo (amplia superfície sem necessidade).

## R11. Tipos compartilhados Rust ↔ TypeScript

- **Decisão**: DTOs de IPC definidos em Rust (`serde`) e tipos TypeScript escritos à mão em
  `apps/desktop/src/ipc/types.ts`, ambos validados contra fixtures JSON compartilhadas em
  `specs/001-meeting-sessions/contracts/fixtures/` (teste de contrato nos dois lados).
- **Justificativa**: evita dependência de geração de código no MVP e mantém contrato explícito.
- **Alternativas**: `tauri-specta`/`ts-rs` (reavaliar se o número de DTOs crescer).

## R12. Frontend

- **Decisão**: React 19 + TypeScript + Vite; estado com `useReducer` + contexto por sessão (sem biblioteca
  global); i18n com `i18next` + `react-i18next` (pt-BR e en, FR-053a); testes com Vitest + Testing Library +
  `vitest-axe` para checagens de acessibilidade. Indicador de captura em componente fixo no layout com
  `aria-live="polite"`, texto e ícone (não só cor, FR-053).
- **Alternativas**: Redux/Zustand (desnecessário no escopo); dicionário i18n próprio (pluralização e
  interpolação reinventadas).

## R13. Logs e redação

- **Decisão**: `tracing` com arquivo rotativo local; conteúdo de sessão envolto em newtype `Redacted<T>` cujo
  `Debug`/`Display` imprime `[redacted]`; lint de revisão proíbe logar DTOs de conteúdo. Teste de redação varre
  logs após sessão de teste (SC-014).
- **Alternativas**: filtros por regex na saída (frágeis).

## R14. Exportação

- **Decisão**: Markdown (legível) e JSON (estruturado, schema `devcopilot.session-export` v1 —
  [contracts/export-format.md](./contracts/export-format.md)). Aviso de que o arquivo não é cifrado (FR-041).
- **Alternativas**: CSV/formatos de ferramentas (fora do MVP, clarificação Q-06).

## R15. Recuperação após falha

- **Decisão**: trechos finais e registros são gravados em transação à medida que surgem. Na inicialização,
  sessões em estado não terminal (`starting`, `capturing`, `paused`, `stopping`) passam a `failed` com motivo
  `interrupted` (FR-007, SC-007).

## R16. Plataformas mínimas de referência

- **Decisão (provisória, confirmar nos spikes)**: macOS 13+ (Apple Silicon e Intel), Windows 10 22H2+/11 com
  WebView2, Linux Ubuntu 22.04+/Fedora 39+ com WebKitGTK 4.1 e PipeWire ou PulseAudio. Hardware de referência
  para SC-005: definido no spike R4 e registrado em `quickstart.md`.

## R17. CI e verificação

- **Decisão**: GitHub Actions com matriz `macos-latest`, `windows-latest`, `ubuntu-latest`:
  `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`,
  `cargo audit`, `cargo deny check`, varredura de segredos (gitleaks), `pnpm` typecheck/lint/test.
  Testes de captura real ficam na matriz de aceitação manual/automatizada por dispositivo (Art. V.5, VIII.4).

## R18. Avaliação de IA e prompt injection

- **Decisão**: conjunto de avaliação em `crates/intelligence/tests/fixtures/` com transcrições sintéticas de
  conversas técnicas (pt-BR e en) anotadas com requisitos/decisões esperados (SC-008) e um conjunto de casos de
  prompt injection (SC-010). Testes determinísticos usam provedor fake; avaliação com provedores reais roda
  sob demanda, nunca com dados confidenciais.

## R19. Dependências auxiliares

- **Decisão**: além das dependências centrais, o workspace usa: `time` (timestamps e durações no domínio, sem
  dependência de SO), `sha2` (verificação SHA-256 do download de modelos, R4), `tracing-appender` (arquivo de
  log rotativo no diretório de logs do app, R13) e `tempfile` (somente dev: bancos e diretórios temporários em
  testes).
- **Justificativa**: cada uma tem um propósito único e é amplamente mantida (Art. VII.6); `tempfile` não entra
  no binário distribuído.
- **Alternativas**: `chrono` no lugar de `time` (API maior, histórico de advisories); implementar hash ou
  rotação de log à mão (código de segurança sem necessidade).

---

## Itens que permanecem dependentes de spike

| Spike | Critério de saída | Registro |
|-------|-------------------|----------|
| S1 — áudio do sistema por SO (R2) | captura funcional + permissões + troca de dispositivo em hardware real | ADR-002 |
| S2 — desempenho de transcrição local (R4) | SC-005 atingido no hardware de referência, por SO | ADR-003 |
| S3 — SQLCipher + keyring em CI e nos 3 SOs (R8) | build reprodutível e SC-018 verificado | ADR-004 |
