# Research — 002 Desktop Release Readiness

Decisões da Fase 0. Cada item resolve uma incógnita do Technical Context do `plan.md`. Itens marcados
**verificar** são premissas que a primeira tarefa da fase correspondente precisa confirmar no ambiente real
antes de construir em cima delas. Se uma verificação falhar, a decisão é revista aqui antes de continuar.

## R1. CSP de produção sem `'unsafe-inline'` (F-1)

- **Decisão**: CSP de produção passa a ter `style-src 'self'`, sem `'unsafe-inline'`, mantendo todas as
  outras diretivas iguais às atuais e acrescentando `connect-src` só ao que o R7 exige. A modificação
  automática de CSP do Tauri (hashes/nonces para assets embutidos) continua habilitada; não usar
  `dangerousDisableAssetCspModification`.
- **Rationale**: o frontend já não usa estilos inline. A revisão de segurança da 001 verificou, e um `grep`
  por `style=`, `CSSProperties`, `insertRule` e `<style` não encontra nada fora de testes. O Vite de produção
  emite CSS como arquivo externo coberto por `'self'`, e React não injeta `<style>`. A única incógnita
  registrada no F-1 é se o runtime do Tauri ou do webview injeta estilos sem hash. O Tauri acrescenta hashes
  para o que ele mesmo injeta no HTML empacotado.
- **Verificar (primeira tarefa da fase CSP)**: rodar o build de produção com a CSP estrita em macOS
  (WKWebView), Windows (WebView2) e Linux (WebKitGTK) e procurar violações. Se o runtime injetar estilo sem
  hash, a saída preferida é um hash específico daquele conteúdo (`'sha256-…'`), nunca `'unsafe-inline'`. Se
  nem isso for possível, abre-se uma exceção pelo processo formal (ADR com prazo), e o F-1 continua bloqueando.
- **Proteção contra regressão**:
  1. Regra de lint no frontend proibindo a prop `style` em elementos DOM (`react/forbid-dom-props`) e
     `dangerouslySetInnerHTML`.
  2. Teste de configuração em Rust (`apps/desktop/src-tauri/tests/csp_policy.rs`) que lê `tauri.conf.json` e
     falha se a CSP de produção contiver `'unsafe-inline'`, `'unsafe-eval'`, curingas, ou destinos fora da
     lista do contrato `contracts/csp-policy.md`.
  3. Teste sobre o `dist/` do build de produção: nenhum `<style>` nem atributo `style=` no HTML emitido.
- **Detecção de violação em produção (US2.4)**: um listener `securitypolicyviolation` no frontend envia um
  relatório mínimo ao núcleo pelo comando `diagnostics_report_csp_violation` (contrato em
  `contracts/ipc-updates.md`). O relatório leva só a diretiva (de uma lista fechada), o esquema do recurso
  bloqueado e a disposição. Nunca leva `sample`, URL completa nem trecho de conteúdo. O núcleo registra no log
  com limite de taxa. É assim que o testador vê violações no build assinado, onde as ferramentas de
  desenvolvedor estão desligadas.
- **Alternativas consideradas**: manter `'unsafe-inline'` com exceção formal (rejeitada: o código não precisa,
  e o F-1 exige a remoção); nonces por requisição (desnecessário, porque não há estilos inline a autorizar);
  habilitar as ferramentas de desenvolvedor no build candidato para ver o console (rejeitada: muda o build que
  está sendo validado).

## R2. Assinatura e validação no macOS

- **Decisão**: certificado *Developer ID Application* da conta Apple Developer do responsável; runtime
  reforçado (padrão do bundler do Tauri); notarização com `notarytool` usando chave de API do App Store
  Connect; *staple* do ticket no `.dmg` e no `.app`. Arquivo de entitlements novo,
  `apps/desktop/src-tauri/Entitlements.plist`, contendo só `com.apple.security.device.audio-input`.
  `bundle.macOS.minimumSystemVersion = "13.0"` (001/R16).
- **Rationale**: com runtime reforçado, o acesso ao microfone exige o entitlement `audio-input`. Sem ele, o
  pedido de permissão não aparece e a captura devolve silêncio (edge case da spec). O app não usa sandbox
  (fora da Mac App Store), então não precisa de entitlements de rede nem de arquivo. `NSMicrophoneUsageDescription`
  já existe no `Info.plist`.
- **Arquiteturas**: builds separados `aarch64-apple-darwin` e `x86_64-apple-darwin`. O de Intel só é
  publicado se for validado em hardware Intel real (FR-032); senão fica fora do release e aparece como
  limitação.
- **Achado relevante, Keychain e identidade**: os itens do Keychain criados pelo `keyring` ficam ligados à
  assinatura do app. Um computador que rodou um build de desenvolvimento (assinado ad hoc) tem banco e chave
  criados por outra identidade. O build assinado encontra o banco mas não consegue ler a chave, e pelo
  ADR-004 regra 5 entra em `secret_store_unavailable`. Isso está correto (falha segura), mas a matriz precisa
  rodar em **perfil de usuário limpo**, e o caso fica documentado como limitação para testadores. Versões
  futuras assinadas pela mesma identidade e com o mesmo `identifier` mantêm o acesso.
- **Alternativas consideradas**: sem notarização (rejeitada: o Gatekeeper bloqueia o app baixado, violando
  SC-001); binário universal (rejeitada: dobra o tamanho e obrigaria validar Intel para publicar
  qualquer coisa).

## R3. Assinatura no Windows

- **Decisão**: assinatura Authenticode de executável e instalador via `bundle.windows.signCommand`, que chama
  a ferramenta do provedor da identidade. Provedor, em ordem de preferência:
  1. **Azure Trusted Signing**, se o responsável for elegível (verificar a elegibilidade de pessoa física no
     país do responsável).
  2. Certificado **OV** com chave em HSM na nuvem do emissor (por exemplo, eSigner ou KeyLocker), usável no CI.
  A escolha fica registrada no ADR-005. Timestamp RFC 3161 é obrigatório para a assinatura continuar válida
  depois que o certificado expirar.
- **Rationale**: desde 2023 as chaves de assinatura de código precisam ficar em hardware certificado, o que
  inviabiliza um `.pfx` em segredo de CI. Token USB físico não funciona em runners hospedados. Certificado EV
  não dá mais reputação imediata no SmartScreen, então não justifica o custo. O aviso de reputação inicial
  vira limitação conhecida (edge case da spec). Os serviços de assinatura são usados **só no build**; o app
  não depende deles em tempo de execução, e isso não conflita com "sem serviços em nuvem".
- **Instalador**: só NSIS (`-setup.exe`), modo `currentUser` (sem admin), que também é o formato que o
  updater usa no Windows. MSI fica fora, para não dobrar a matriz. WebView2:
  `webviewInstallMode = embedBootstrapper`, para o instalador funcionar sem baixar componentes à parte quando
  o WebView2 estiver ausente.
- **Alternativas consideradas**: MSI e NSIS juntos (rejeitada: dois instaladores para validar);
  `offlineInstaller` do WebView2 (rejeitada: +~127 MB, e o bootstrapper embutido basta); deixar sem
  assinatura (proibido pelo FR-005).

## R4. Distribuição e verificação no Linux

- **Decisão**: AppImage e `.deb` para `x86_64`, construídos em `ubuntu-22.04` (glibc mais antiga entre as
  plataformas-alvo, o que garante compatibilidade para a frente). Um único `SHA256SUMS` cobre todos os
  artefatos do release (todos os SOs), com assinatura OpenPGP destacada `SHA256SUMS.asc` feita com a chave de
  release do projeto. A impressão digital da chave é publicada no README e no ADR-005. O usuário verifica com
  `gpg --verify` e `sha256sum -c`, ferramentas padrão do sistema (US1.3).
- **Proveniência (FR-007)**: atestações de build do GitHub (`actions/attest-build-provenance`) para cada
  artefato, verificáveis com `gh attestation verify`. Elas ligam o artefato ao commit e ao workflow. São um
  complemento para rastreabilidade, não o mecanismo que o usuário final precisa usar.
- **Limitações a documentar**: o `.deb` não se atualiza pelo app (o updater do Tauri atualiza o AppImage no
  Linux); quem instala pelo `.deb` atualiza manualmente. WebKitGTK no Wayland com certos drivers de GPU pode
  renderizar em branco (contorno `WEBKIT_DISABLE_DMABUF_RENDERER=1`). Isso precisa ser verificado na matriz e,
  se ocorrer, registrado.
- **Alternativas consideradas**: Flatpak/Snap (fora de escopo, por exigirem sandbox e portais); `.rpm`
  (adiado: Fedora pode usar o AppImage; adicionar `.rpm` exige outra rodada da matriz); só minisign para os
  usuários (rejeitada: não é ferramenta padrão das distribuições).

## R5. Atualização dentro do app (US8, FR-008 a FR-008d)

- **Decisão**: `tauri-plugin-updater` 2.x, acionado **só pelo núcleo Rust**, sem permissão de updater na
  capability da webview, seguindo o precedente do `SaveDialog` da exportação. As peças:
  - **Porta** `AppUpdater` em `crates/application` (`check() -> Option<AvailableUpdate>`,
    `download_and_install(&AvailableUpdate, CancellationToken)`).
  - **Caso de uso** `UpdateService` em `crates/application`, com a política: verificação habilitada, aviso
    de primeira execução já mostrado, nenhuma sessão em `capturing`/`paused`/`starting`/`stopping`,
    confirmação explícita referente à versão verificada, e uma operação por vez.
  - **Adaptador** `TauriUpdater` em `apps/desktop/src-tauri/src/updater.rs`, que depende do `AppHandle` e por
    isso fica na raiz de composição, como o `NativeSaveDialog`.
- **Verificação criptográfica**: cada pacote é verificado pelo plugin com minisign contra a chave pública
  embutida em `plugins.updater.pubkey`. O plugin só oferece versões maiores que a instalada, o que cobre a
  rejeição de downgrade. Os testes de aceitação (SC-014) cobrem pacote adulterado, chave diferente e versão
  menor num canal de teste.
- **Manifesto**: um `latest.json` estático publicado como asset do release do GitHub. O endpoint é
  `https://github.com/EderAndrew/dev-copilot/releases/latest/download/latest.json`, sem variáveis de versão na
  URL. A requisição só revela o que qualquer download HTTP revela (IP, user-agent com versão). Os destinos,
  incluindo o redirecionamento ao armazenamento de assets do GitHub, ficam no contrato `csp-policy.md` (não
  vão para a CSP, porque a requisição sai do núcleo, não da webview) e no aviso ao usuário.
- **Quando verifica**: nunca antes de o aviso de primeira execução ter sido exibido e dispensado. Depois
  disso, uma vez na inicialização e a cada 24 h com o app aberto, mais o botão "Verificar agora" nas
  Configurações. Desligada, nenhuma requisição (verificado com proxy, SC-014).
- **Instalação**: só depois da confirmação do usuário, com nova checagem do estado das sessões no momento da
  confirmação. Download com cancelamento; falha ou corrupção descarta o pacote (o plugin verifica antes de
  aplicar). Depois de instalar, reinício controlado pelo núcleo. No Windows o instalador NSIS encerra o app
  sozinho, e o caso de uso garante que nenhuma sessão está ativa antes disso.
- **TLS**: **verificar** que o cliente HTTP do plugin usa TLS com verificação de certificado (rustls ou nativo)
  e não aceita HTTP. A configuração do plugin rejeita endpoints não-HTTPS em build de release. Se o plugin
  puxar `native-tls`/OpenSSL no Linux, avaliar a feature `rustls-tls` para manter a pilha uniforme com o
  `reqwest` da 001 e passar no `cargo deny`.
- **Resultado da verificação de TLS (T002, 2026-11-01)**: `tauri-plugin-updater` 2.14.1 tem como features
  padrão `rustls-tls`, `system-proxy` e `zip`. Ele reutiliza o mesmo `reqwest` 0.13.5 e o mesmo `rustls` 0.23 da
  001, sem uma segunda versão. Não puxa `native-tls`. O único `openssl-sys` da árvore continua sendo o do
  SQLCipher no Linux (ADR-004). Crates novos no lockfile: `minisign-verify`, `zip`, `tar`, `filetime`, `xattr`,
  `osakit` e dependências de plataforma. `cargo deny check` passa (advisories, bans, licenses, sources).
  Nenhuma feature extra é necessária.
- **Resultado da implementação (T029, T033, T037, T038, 2026-11-01)**:
  - O `tauri-plugin-updater` 2.14.1 funciona no runtime mock do `tauri::test` (feature `test` do `tauri` só
    em dev-dependencies). Por isso a T029 testa o adaptador contra o plugin real, com `wiremock`: assinatura
    adulterada, de outra chave, ligada a outra versão, manifesto malformado, download truncado e
    cancelamento. Não foi preciso recorrer às funções puras. As chaves de teste ficam em
    `apps/desktop/src-tauri/tests/fixtures/updater/` (só as públicas; as privadas foram descartadas).
  - **Achado**: em build de debug o plugin só avisa sobre endpoint `http://` e faz a requisição mesmo assim.
    A recusa só existe em build de release. O `TauriUpdater` valida a configuração antes de qualquer
    requisição (`updater::validate_config`): endpoints só HTTPS, nenhuma opção `dangerous*` nem
    `allowDowngrades`, e `pubkey` bem formada. A URL do pacote no manifesto também precisa ser HTTPS, porque
    o plugin só registra um aviso.
  - **`requireSignedVersion: true`**: a assinatura precisa carregar a versão que assina, o que impede parear
    uma versão anunciada maior com o pacote legítimo de uma versão antiga (downgrade por manifesto forjado).
    Como 0.1.0 é o primeiro release, não há release antigo sem versão na assinatura. O CLI do Tauri grava a
    versão ao assinar (`--app-version`).
  - **Porta em dois passos**: a porta `AppUpdater` tem `download` (baixa e verifica) e `install` separados,
    em vez de um `download_and_install`. Assim o caso de uso checa as sessões de novo entre os dois passos
    (data-model §1.3). Cancelar um download volta para `available` (o update continua válido) e cancelar uma
    verificação volta para `idle`. Uma falha leva a `failed` e exige nova verificação.
  - Até a T006, `plugins.updater.pubkey` tem o marcador `PENDING-T006-updater-public-key`. Com ele, toda
    verificação falha com `manifest_invalid` sem nenhuma requisição, e `tests/updater_config.rs` falha quando
    `RELEASE_TAG` está definido. Um release não sai sem a chave real.
- **Rollback de uma atualização publicada**: o updater não aceita downgrade. Suspender uma versão significa
  republicar o `latest.json` apontando para a versão anterior (os apps que ainda não atualizaram deixam de vê-la)
  e corrigir para a frente com uma versão N+1. Rotação da chave de atualização: uma versão assinada com a chave
  antiga que embute a nova. Perda da chave antiga: os usuários precisam reinstalar manualmente pelo instalador
  assinado. Isso fica no plano de rollback.
- **Alternativas consideradas**: updater pela API JS na webview (rejeitada: amplia a capability e põe a
  decisão na fronteira não confiável); servidor de atualização próprio (rejeitada: é backend); verificação
  manual só por notas de release (rejeitada pela clarificação Q2).

## R6. Pipeline de release

- **Decisão**: workflow novo `.github/workflows/release.yml`, disparado por tag `v*.*.*`:
  1. Reexecuta os gates da CI (fmt, clippy, testes, audit, deny, gitleaks, frontend).
  2. Constrói em paralelo nos runners `macos-15` (aarch64 e x86_64), `windows-latest` e `ubuntu-22.04`.
  3. Assina, notariza, gera `latest.json`, `SHA256SUMS` e atestações.
  4. Cria um **release em rascunho** no GitHub.
  Os segredos ficam num *environment* `release` com aprovação obrigatória do responsável. Publicar (tirar do
  rascunho) é manual e acontece só depois do checklist aprovado (FR-030, FR-031).
- **Rationale**: o rascunho é o portão natural entre "artefatos prontos" e "público". O environment protegido
  impede que um workflow de PR acesse os segredos. Se qualquer passo de assinatura falhar, o job falha e não
  há asset (FR-005).
- **Assinatura OpenPGP de `SHA256SUMS`**: feita **localmente** pelo responsável depois de baixar e conferir o
  rascunho, com a chave de release fora do CI. Isso reduz a exposição da chave que o usuário final usa para
  verificar. O `.asc` é anexado ao rascunho antes de publicar.
- **Versão**: um teste/script de checagem garante que `tauri.conf.json`, `apps/desktop/package.json` e o
  `Cargo.toml` do app têm a mesma versão e que ela é igual à tag.
- **Alternativas consideradas**: `tauri-apps/tauri-action` (aceitável; a decisão final fica para a
  implementação, desde que os passos acima sejam explícitos e auditáveis); assinar o `SHA256SUMS` no CI
  (rejeitada pelo motivo acima).

## R7. Medição de desempenho (US5, FR-017 a FR-020)

- **Latência de transcrição no app instalado**: modo de métricas opcional, ligado só pela variável de
  ambiente `DEVCOPILOT_METRICS=1` e desligado por padrão. Grava num CSV no diretório de logs, por trecho:
  tipo (parcial/final), instante de início e fim da fala no relógio de áudio, instante de emissão no relógio
  monotônico e o modelo. **Nenhum texto**. O teste de redação de logs da 001 (T157) é estendido para cobrir
  esse arquivo. A latência é calculada como emissão menos fim da fala (final) e emissão menos início da fala
  (primeiro parcial).
- **Entrada de áudio reprodutível**: gravações de fala real não confidencial, lidas a partir de roteiros pelo
  time, tocadas num dispositivo de entrada virtual (BlackHole no macOS, VB-CABLE no Windows, `null-sink` com
  monitor no Linux). Mais uma execução ao vivo por SO com uma pessoa falando. O dispositivo virtual serve só
  para a medição; a validação de dispositivos usa hardware real (matriz).
- **CPU e memória**: binário auxiliar `tooling/perf-sampler` (crate Rust do workspace, `publish = false`, fora
  do app distribuído), usando `sysinfo` para amostrar CPU e RSS do processo do app a cada 5 s e gravar CSV.
  Escolhido no lugar de scripts por SO para usar o mesmo método nas três plataformas. Condições de repouso
  (energia na tomada, sem outras cargas, 10 min de aquecimento) ficam no plano de medição.
- **Alternativas consideradas**: instrumentar via logs de tracing existentes (rejeitada: misturaria métricas
  com o log de diagnóstico e dificultaria a redação); `psutil` em Python (rejeitada: dependência extra nas
  máquinas de teste).

## R8. Avaliação da análise com provedor real (FR-021)

- **Decisão**: reaproveitar sem mudanças os harnesses da 001 (`evaluation.rs`, `prompt_injection.rs`,
  `qa_evaluation.rs`) com `DEVCOPILOT_EVAL_PROVIDER` apontando para o Ollama local. O modelo exato (nome **e**
  digest) é fixado no plano de medição antes da execução, escolhido entre os que cabem em 16 GB de RAM. Um
  provedor remoto pode ser avaliado depois, com os mesmos fixtures não confidenciais e sob autorização
  (001/US7). Resultados registrados no ADR-003.
- **Rationale**: os harnesses já medem SC-008, SC-010 e SC-011. Criar outro conjunto de avaliação seria
  reimplementar a 001.

## R9. Spike S1 de áudio do sistema (FR-023 a FR-026)

- **Decisão**: o S1 segue o protocolo já escrito no ADR-002. Protótipos descartáveis ficam num branch
  `spike/s1-system-audio`, **não mesclado**. Só o resultado (tabela por SO, limitações, critérios para a
  feature futura) entra no ADR-002 por este branch. Nenhum código de adaptador, entitlement ou permissão de
  áudio do sistema entra no release (Q1, FR-024).
- **Rationale**: preserva a regra de não declarar suporte e mantém o release livre de `unsafe` de FFI ainda
  não revisado.

## R10. Registro de evidências e decisão de release

- **Decisão**: os documentos de processo que valem para todos os releases ficam em `docs/release/`
  (`checklist.md`, `rollback.md`, `signing-and-distribution.md`, `verify-downloads.md`). As evidências deste
  release ficam em `docs/release/v0.1.0/`: matriz por plataforma, plano de medição, resultados e decisão.
  Capturas de tela comprimidas (≤ 500 KB cada) são versionadas junto. Vídeos e arquivos grandes vão como
  anexo do release em rascunho, referenciados por link e SHA-256. Formatos em `contracts/release-evidence.md`.
- **Rationale**: evidência versionada é revisável em PR e rastreável ao commit, sem serviço externo. A matriz
  da 001 continua sendo o roteiro; cada plataforma ganha uma cópia preenchida, em vez de células editadas no
  documento da 001.
- **Severidade**: a escala da spec (crítico, alto, médio, baixo) é usada nos problemas abertos durante a
  validação, com o rótulo `release-blocker` para os críticos.

## R11. Achados que a primeira passada pela matriz pode revelar (riscos técnicos)

| Achado possível | Onde aparece | Resposta planejada |
|-----------------|--------------|--------------------|
| O webview injeta estilo sem hash | R1, todas as plataformas | Hash específico; ADR de exceção se impossível |
| Microfone mudo no build assinado do macOS | R2 | Entitlement `audio-input`; caso dedicado na matriz |
| Keychain inacessível depois de trocar de identidade | R2 | Perfil limpo; limitação documentada |
| WebView2 ausente no Windows 10 | R3 | Bootstrapper embutido; caso de instalação limpa |
| Tela em branco no WebKitGTK/Wayland | R4 | Contorno documentado ou limitação por combinação |
| Plugin do updater puxa outra pilha de TLS | R5 | Feature `rustls-tls`; `cargo deny` |
| Secret Service ausente em sessão mínima | Matriz caso 13 | Comportamento da 001 (bloqueia com explicação) |
