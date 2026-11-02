# Feature Specification: Prontidão para o primeiro release desktop

**Feature Branch**: `002-desktop-release-readiness`
**Created**: 2026-11-01
**Status**: Draft
**Input**: User description: "Feature 002 — Desktop Release Readiness do Dev Copilot: preparar o aplicativo
desktop para seu primeiro release seguro e validado em macOS, Windows e Linux, respeitando a Constitution
v1.1.0 e os ADR-001 a ADR-004 — segurança de distribuição (CSP sem `style-src 'unsafe-inline'`, instaladores
assinados, atualizações verificadas), validação multiplataforma em hardware real, medição de qualidade e
desempenho contra critérios definidos antes dos testes, decisão sobre áudio do sistema (T150–T153) e
checklist de release e rollback com evidências de aprovação. Sem reimplementar a Feature 001, sem backend
obrigatório, sem enfraquecer controles de segurança."

## Contexto

A Feature 001 (`specs/001-meeting-sessions`) está implementada: 480 testes Rust e 176 testes de frontend
passam, e os gates de CI (fmt, clippy, audit, deny, gitleaks, checagens de frontend) estão verdes. O
aplicativo nunca foi empacotado nem distribuído. A revisão de segurança da 001 aprovou o merge e deixou três
achados que **bloqueiam o primeiro release**:

- **F-1**: a CSP de produção ainda permite `style-src 'unsafe-inline'`.
- **F-4**: não há assinatura de artefatos nem atualização verificada.
- **F-6**: a matriz de aceitação por SO (T162) não foi executada.

Também ficaram abertos: T150–T153 (spike S1 e adaptadores de áudio do sistema), T166 (validação ponta a
ponta do quickstart com uma pessoa usando o app), as medições de Windows e Linux para SC-005 (ADR-003), as
métricas com provedor real (ADR-003, T083/T111a) e a verificação do armazenamento cifrado no Windows e no
Linux (ADR-004).

A única funcionalidade de produto nova desta feature é a **atualização verificada dentro do app** (US8).
No mais, ela produz um candidato a release assinado, evidências de validação em SO real e uma decisão de
publicação registrada, e corrige só o que essas validações mostrarem como bloqueador.

## Clarifications

### Session 2026-11-01

- Q1: A implementação dos adaptadores de áudio do sistema (T151–T153) faz parte desta feature? → A: Não. A
  002 executa o spike S1 (T150), registra a decisão por SO no ADR-002, e o primeiro release sai só com
  microfone. Os adaptadores ficam para uma feature futura.
- Q2: O primeiro release inclui atualização dentro do app? → A: Sim, com verificação criptográfica da
  assinatura de cada pacote, instalação só com confirmação do usuário, e verificação de versão nova que o
  usuário pode desligar.

**Atores**:

- **Responsável pelo release**: decide a publicação, guarda as identidades de assinatura e aprova as
  evidências.
- **Testador de aceitação**: executa a matriz em máquinas reais e registra as evidências.
- **Usuário final**: baixa, verifica, instala e usa o Dev Copilot no próprio computador.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Instalar um build assinado e verificável (Priority: P1)

O usuário final baixa o instalador do Dev Copilot para o seu SO, confirma que ele veio do projeto e não foi
alterado, e instala sem precisar desativar ou contornar avisos de segurança do sistema.

**Why this priority**: sem isso não há release (F-4, Art. IV.15). Um usuário treinado a contornar avisos de
segurança para instalar um app que capta áudio é exatamente o hábito que o projeto não pode incentivar.

**Independent Test**: num computador limpo de cada SO, baixar o artefato publicado, verificar assinatura e
checksum seguindo só a documentação pública, instalar e abrir o app sem alterar configurações de segurança do
SO.

**Acceptance Scenarios**:

1. **Given** um macOS com as configurações de segurança padrão, **When** o usuário abre o instalador baixado,
   **Then** o sistema reconhece o desenvolvedor identificado e o app aberto passou pela validação da Apple,
   sem aviso de origem desconhecida nem passo de "abrir mesmo assim".
2. **Given** um Windows com as configurações padrão, **When** o usuário executa o instalador, **Then** o
   controle de conta de usuário exibe o editor verificado do projeto (não "Editor desconhecido").
3. **Given** uma distribuição Linux suportada, **When** o usuário segue o procedimento documentado de
   verificação, **Then** consegue confirmar a assinatura e o checksum do artefato com ferramentas padrão do
   sistema antes de instalar.
4. **Given** um artefato com um único byte alterado, **When** o usuário o verifica, **Then** a verificação
   falha em todos os SOs, de forma visível.
5. **Given** o app instalado e assinado, **When** o usuário inicia a primeira sessão com microfone, **Then**
   o build assinado mantém o comportamento validado no build de desenvolvimento, conforme a plataforma: no
   macOS o pedido de permissão aparece com o nome do app e o motivo do uso; no Windows vale a configuração de
   privacidade do microfone para apps desktop, e o app orienta quando ela bloqueia; no Linux, que em geral não
   tem pedido de permissão, a captura funciona ou o app mostra a orientação. Em todos, a captura funciona
   depois da permissão.

---

### User Story 2 - Interface funcionando sob política de conteúdo estrita (Priority: P1)

O usuário usa o Dev Copilot com a mesma aparência, comportamento e acessibilidade de hoje, mas o app passa a
rodar com uma política de conteúdo que não aceita estilos inline em nenhuma diretiva.

**Why this priority**: fecha o F-1. É condição de release e não pode custar nenhuma funcionalidade nem
acessibilidade (requisito explícito; Art. IV.11).

**Independent Test**: num build de produção em cada SO, percorrer todas as telas e fluxos da matriz com o
registro de violações de política ligado, e comparar a aparência com a referência visual atual.

**Acceptance Scenarios**:

1. **Given** o build de produção, **When** sua configuração é inspecionada, **Then** a política de conteúdo
   não contém `'unsafe-inline'` nem `'unsafe-eval'` em nenhuma diretiva, e não é mais permissiva que a atual
   em nenhuma outra diretiva.
2. **Given** o build de produção em cada SO, **When** o testador percorre Sessões, Histórico, Detalhe da
   sessão, Provedores, Configurações, diálogos (consentimento, exportação, exclusão) e o indicador de captura,
   **Then** zero violações de política são registradas e nenhum elemento fica sem estilo, deslocado ou
   invisível.
3. **Given** a nova política, **When** a verificação automática de acessibilidade e o fluxo só com teclado da
   001 (SC-016) rodam, **Then** os resultados são iguais ou melhores que a linha de base anterior à mudança.
4. **Given** o mecanismo do app que injeta estilos próprios, **When** ele tenta aplicar um estilo que não
   está explicitamente permitido, **Then** o estilo é bloqueado e a violação aparece no registro de
   diagnóstico, sem conteúdo de sessão.

---

### User Story 3 - Validar captura e sessões em hardware real em cada SO (Priority: P1)

O testador de aceitação executa a matriz de aceitação existente (`specs/001-meeting-sessions/acceptance-matrix.md`)
no build candidato assinado, em máquinas reais com macOS, Windows, Linux Wayland e Linux X11, e registra as
evidências e limitações de cada plataforma.

**Why this priority**: fecha o F-6, T162 e T166. Testes automatizados não provam compatibilidade
multiplataforma (Art. V.2, V.5). Sem esta história não existe base para declarar suporte a nenhum SO.

**Independent Test**: executar todos os casos da matriz (1–13 e medições cronometradas) numa plataforma e
produzir o registro de evidências dela. Cada plataforma é aprovada ou reprovada separadamente.

**Acceptance Scenarios**:

1. **Given** um computador sem permissão de microfone concedida, **When** o usuário nega o pedido ao iniciar
   uma sessão, **Then** nada é captado nem gravado, e o app mostra como conceder a permissão naquele SO
   (001/FR-009, FR-010, SC-004).
2. **Given** a permissão concedida, **When** o testador cria, inicia, pausa por 30 s, retoma e para uma
   sessão, **Then** o indicador mostra cada estado, a transcrição registra a lacuna de pausa e o indicador
   desliga em até 1 s depois de parar (001/SC-002).
3. **Given** uma captura em andamento, **When** o microfone em uso é desconectado, um fone Bluetooth é
   conectado ou desconectado, ou o dispositivo padrão muda, **Then** o app informa a perda da fonte e não troca
   de dispositivo sem avisar (001/FR-011).
4. **Given** uma captura em andamento, **When** o SO é suspenso ou a sessão do usuário é bloqueada e depois
   retomada, **Then** o app informa a perda da fonte, não trava e não volta a captar sozinho.
5. **Given** uma captura em andamento, **When** o processo do app é encerrado à força e o app é reaberto,
   **Then** a sessão aparece como interrompida, com 100% dos trechos finais salvos antes da falha
   (001/FR-007, SC-007).
6. **Given** o cofre de credenciais do SO indisponível, **When** o app abre, **Then** ele explica a situação
   e não grava conteúdo nem capta áudio (001/FR-039a).
7. **Given** a execução de uma plataforma concluída, **When** o responsável revisa o registro, **Then** cada
   caso tem resultado (passou, falhou com link para o problema, ou não se aplica com motivo), ambiente
   identificado (versão do SO, hardware, build), executor, data e evidência anexada.

---

### User Story 4 - Validar acessibilidade com teclado e leitores de tela (Priority: P1)

Um usuário que depende de teclado ou de leitor de tela consegue iniciar, pausar, retomar e parar uma captura,
revisar registros e exportar uma sessão em cada SO, com o estado da captura anunciado por texto.

**Why this priority**: o app capta áudio e o estado da captura é informação de privacidade, não só de
conveniência. Se o usuário não percebe que está captando, há risco de captura sem consentimento informado
(Art. IV.1). As verificações automáticas da 001 (T160) não substituem leitores de tela reais.

**Independent Test**: com VoiceOver (macOS), Narrator e NVDA (Windows) e Orca (Linux), executar os casos 11 e
11b da matriz sem mouse e sem olhar a tela, e registrar o que foi anunciado.

**Acceptance Scenarios**:

1. **Given** um leitor de tela ativo, **When** a captura muda entre capturando, pausada e sem captura,
   **Then** o leitor anuncia o novo estado por texto, sem depender de cor ou ícone (001/SC-016).
2. **Given** só o teclado, **When** o usuário percorre iniciar → consentimento → pausar → retomar → parar →
   revisar registro → exportar, **Then** o fluxo termina sem mouse, com foco visível e sem armadilha de foco
   em diálogos.
3. **Given** um leitor de tela que não anuncia algum estado num SO, **When** o resultado é registrado,
   **Then** a falha é classificada como bloqueadora para aquela plataforma, a menos que haja contorno
   documentado e aprovado pelo responsável.

---

### User Story 5 - Medir desempenho e qualidade contra critérios definidos antes (Priority: P2)

O responsável pelo release define, antes de qualquer medição, um plano com metas numéricas para latência de
transcrição, CPU, memória e qualidade da análise. Depois, o testador mede no app real em hardware
representativo de cada SO e registra os resultados contra essas metas.

**Why this priority**: o release não deve prometer um desempenho que ninguém mediu fora de um Apple M4 (ADR-003
mede só o macOS). Isso vem depois de US1–US4 porque um desempenho abaixo da meta pode virar uma limitação
documentada, enquanto falhas de segurança ou de captura não podem.

**Independent Test**: congelar o plano de medição (versionado e com data anterior às medições), executar as
medições de uma plataforma e comparar cada resultado com a meta.

**Acceptance Scenarios**:

1. **Given** o plano de medição aprovado, **When** a primeira medição começa, **Then** o plano já está
   registrado com data, metas, hardware, método e tamanho de amostra. Metas alteradas depois exigem
   justificativa registrada e não apagam a meta original.
2. **Given** o hardware de referência e o hardware mínimo de cada SO, **When** fala real não confidencial é
   transcrita localmente com o modelo padrão, **Then** a latência de parcial e de final é medida em pelo menos
   100 trechos por combinação e comparada com 001/SC-005.
3. **Given** uma sessão contínua de 2 horas no app instalado (não simulada), **When** CPU e memória são
   amostradas, **Then** os resultados são comparados com as metas do plano e com 001/SC-015.
4. **Given** um provedor de análise real configurado e o conjunto de avaliação controlado, **When** a
   avaliação roda, **Then** recall de requisitos e decisões explícitos, grounding dos registros "observados",
   taxa de itens inventados, resistência a prompt injection e respostas sem evidência são registrados contra
   001/SC-008, SC-010 e SC-011.
5. **Given** um resultado abaixo da meta, **When** o responsável o avalia, **Then** ele é classificado como
   bloqueador ou como limitação documentada (por exemplo, "modelo `small` não recomendado em hardware
   mínimo"), nunca ignorado.

---

### User Story 6 - Decidir a publicação com checklist, evidências e plano de rollback (Priority: P2)

O responsável pelo release percorre um checklist de release que referencia cada evidência, confirma que nenhum
bloqueador crítico está aberto, registra a aprovação e só então publica. Se um problema grave aparecer depois,
o projeto segue um plano de rollback documentado.

**Why this priority**: junta o resultado de US1–US5 numa decisão rastreável (Art. IX.4: não afirmar o que não
foi validado). Depende das outras histórias, mas o checklist e o plano de rollback podem ser escritos e
revisados de forma independente.

**Independent Test**: fazer um ensaio completo com um candidato de teste: preencher o checklist, simular um
bloqueador aberto (a publicação precisa ser recusada) e executar o rollback num canal de teste.

**Acceptance Scenarios**:

1. **Given** um bloqueador crítico aberto, **When** o responsável tenta aprovar o release, **Then** o
   checklist não pode ser concluído e a decisão registrada é "não publicar".
2. **Given** todos os itens do checklist com evidência, **When** o responsável aprova, **Then** a aprovação
   registra nome, data, identificação exata do build (versão e commit), checksums dos artefatos e a lista de
   limitações conhecidas por plataforma.
3. **Given** um release publicado com defeito grave, **When** o rollback é acionado, **Then** os artefatos
   defeituosos deixam de ser oferecidos, os usuários são orientados por uma nota pública, e a orientação não
   pede a ninguém que contorne controles de segurança.
4. **Given** a documentação de release, **When** uma pessoa do projeto que não participou do release a segue,
   **Then** ela consegue reproduzir o empacotamento, a assinatura e a publicação sem conhecimento que só exista
   na cabeça de alguém.

---

### User Story 7 - Decidir o suporte a áudio do sistema por plataforma (Priority: P3)

O responsável executa o spike S1 (T150) em hardware real seguindo o protocolo do ADR-002 e registra, para cada
SO, se o áudio do sistema é aprovado, reprovado ou adiado, com as limitações de permissão e compatibilidade.

**Why this priority**: o áudio do sistema aumenta o valor do produto, mas o release com microfone é viável
sem ele (001/US8.2, FR-014). A regra inegociável é não declarar suporte sem teste em hardware real. Por
decisão de escopo (Clarifications, Q1), esta feature executa o S1 e registra a decisão, mas **não implementa**
os adaptadores T151–T153. O primeiro release sai só com microfone em todas as plataformas.

**Independent Test**: executar o protocolo S1 numa plataforma e verificar que o ADR-002 registra o resultado
e que o release continua mostrando a fonte como indisponível com motivo.

**Acceptance Scenarios**:

1. **Given** o build candidato em qualquer plataforma, **When** o usuário abre o formulário de nova sessão,
   **Then** a fonte "áudio do sistema" aparece indisponível com motivo, a sessão com microfone continua
   possível, e as notas de release dizem que o recurso não é suportado neste release (caso 12 da matriz).
2. **Given** S1 aprovado numa plataforma, **When** o resultado é registrado, **Then** o ADR-002 registra a
   aprovação e as permissões e limitações encontradas, que servem de entrada para a feature futura dos
   adaptadores. Nada disso é anunciado como recurso do release.
3. **Given** qualquer resultado de S1, **When** o ADR-002 é atualizado, **Then** ele registra versão do SO,
   hardware, data, executor e resultado de cada item do protocolo.
4. **Given** o S1 não executado em alguma plataforma (por exemplo, por falta de hardware), **When** o release é
   avaliado, **Then** isso aparece como "adiado" no ADR-002 e não bloqueia o release.

---

### User Story 8 - Receber atualizações com verificação criptográfica (Priority: P2)

Quando houver uma versão nova, o app avisa o usuário e, com a confirmação dele, instala só o que foi assinado
pelo projeto, sem enviar conteúdo de sessão. O usuário pode desligar a verificação.

**Why this priority**: por decisão de escopo (Clarifications, Q2), o primeiro release já inclui atualização
dentro do app. Sem ela, usuários ficariam presos em versões com falhas de segurança já corrigidas (Art.
IV.15). É P2 porque depende da assinatura (US1) e o app continua seguro mesmo se a verificação estiver
desligada.

**Independent Test**: num canal de teste, a partir da versão N instalada, publicar uma N+1 assinada, uma
adulterada, uma assinada com outra chave e uma N−1 assinada, e verificar que só a N+1 legítima é instalada.

**Acceptance Scenarios**:

1. **Given** o app instalado com a verificação ligada e uma versão nova assinada publicada, **When** o app
   verifica, **Then** ele mostra a versão disponível e as notas, e só baixa e instala depois da confirmação
   do usuário.
2. **Given** um pacote com assinatura inválida, ausente ou de outra chave, ou de versão menor que a
   instalada, **When** ele é oferecido, **Then** é rejeitado, nada é instalado, a versão atual continua
   funcionando e o usuário vê uma mensagem clara.
3. **Given** a verificação ligada, **When** o app consulta se há versão nova, **Then** a requisição não
   contém conteúdo de sessão, credenciais nem identificadores pessoais além do mínimo técnico (versão,
   plataforma e arquitetura), vai só ao destino de atualização documentado, usa TLS autenticado e, se
   falhar, não afeta nenhuma outra função do app.
4. **Given** o usuário desligou a verificação nas Configurações, **When** o app roda, **Then** zero
   requisições de atualização são feitas (verificado com proxy local, como no caso 8 da matriz).
5. **Given** uma sessão capturando ou pausada, **When** uma atualização está disponível, **Then** a
   instalação não começa até a sessão parar, e uma atualização que muda o formato dos dados locais segue o
   procedimento de migração da 001 (`docs/local-data.md`).
6. **Given** um download interrompido ou corrompido, **When** a instalação seria aplicada, **Then** ela é
   descartada e a versão instalada continua íntegra.

---

### Edge Cases

- **Build assinado diferente do build de desenvolvimento**: as permissões do SO (microfone, cofre de
  credenciais) ficam ligadas à identidade assinada e ao identificador do app. Uma permissão concedida ao build
  de desenvolvimento pode não valer para o assinado, e vice-versa. A matriz roda no **build candidato
  assinado**, não no de desenvolvimento.
- **Runtime reforçado do macOS sem a declaração de uso de microfone**: o pedido de permissão não aparece e a
  captura falha em silêncio ou devolve silêncio (nota da T048 sobre permissão indeterminada). Precisa ser
  detectado na matriz.
- **Reputação do instalador no Windows**: mesmo assinado, um certificado novo pode disparar um aviso de
  reputação nos primeiros downloads. Isso é registrado como limitação conhecida com orientação ao usuário,
  nunca resolvido mandando o usuário desativar a proteção.
- **Componente de navegador embutido ausente ou desatualizado no Windows**: o instalador precisa
  providenciá-lo ou explicar o que falta. O app não pode abrir em branco.
- **Linux sem serviço de segredos** (sessões mínimas, alguns gerenciadores de janela): coberto pelo caso 13
  da matriz. O app explica e bloqueia gravação e captura.
- **Linux Wayland vs X11 e PipeWire vs PulseAudio**: comportamentos diferentes são registrados como
  plataformas separadas. Uma não aprova a outra.
- **Restaurar a política de conteúdo exige estilos calculados em tempo de execução** (por exemplo, posição ou
  dimensão dinâmica): eles precisam ir por um caminho permitido pela política estrita, nunca reabrindo
  `'unsafe-inline'`.
- **Medição em hardware compartilhado ou com processos de fundo pesados**: invalida a amostra. O plano de
  medição define condições de repouso e como registrá-las.
- **Rollback depois de uma migração de dados**: a 001 recusa abrir um banco com formato mais novo que o app.
  Um rollback que oriente voltar a uma versão anterior precisa considerar isso. Para o primeiro release, o
  formato inicial vira a linha de base, e as notas futuras precisam dizer se uma versão é reversível.
- **Comprometimento de uma identidade de assinatura**: o plano de rollback cobre revogação ou rotação da
  identidade, retirada dos artefatos afetados e comunicação aos usuários.
- **Leitor de tela com comportamento diferente entre versões** (por exemplo, NVDA vs Narrator): cada leitor é
  registrado separadamente na matriz.
- **Download do modelo de transcrição na primeira execução** (001/ADR-003) em rede restrita ou com proxy
  corporativo: o app precisa explicar a falha. Isso não é falha do instalador e entra nas limitações se não
  houver contorno.

### Failure Cases *(mandatory — Constitution Art. V.4, IX.2)*

- **Permissão de microfone negada ou revogada no build assinado**: nada é captado. O app mostra a orientação
  daquele SO e a sessão continua utilizável para revisão. Validado em cada plataforma (caso 1 da matriz).
- **Parada ou pausa durante transcrição pesada**: a captura para em até 500 ms (001/SC-006) e o indicador
  desliga em até 1 s. Trechos finais já salvos são preservados.
- **Troca de dispositivo, Bluetooth, suspensão ou bloqueio**: a fonte é dada como perdida e informada, sem
  troca silenciosa, sem crash e sem retomar sozinha. A sessão persistida não é corrompida.
- **Provedor de análise real com erro, tempo esgotado ou saída inválida durante a avaliação**: os controles
  de captura continuam responsivos, a falha aparece nos resultados e não é contada como acerto, e o conjunto
  de avaliação é reexecutado só depois de registrar o incidente.
- **Transcrição parcial** (sobrecarga, falha do transcritor): lacunas registradas como na 001 e contabilizadas
  nas medições de latência, nunca descartadas da amostra.
- **Falha de armazenamento ou cofre indisponível no SO testado**: nenhuma gravação e captura bloqueada, com
  explicação (001/FR-039a). No Windows e no Linux isso também verifica o ADR-004.
- **Violação da política de conteúdo em produção**: o estilo é bloqueado e a violação aparece no diagnóstico
  sem conteúdo de sessão. Uma violação que quebre a interface é bloqueadora de release.
- **Assinatura ou validação da Apple falha no pipeline de release**: nenhum artefato não assinado ou não
  validado é publicado. O pipeline falha de forma visível e não existe caminho de "publicar sem assinar".
- **Atualização adulterada ou interrompida no meio do download**: rejeitada ou descartada. A versão instalada
  continua íntegra e funcional.
- **Bloqueador crítico descoberto depois da aprovação e antes da publicação**: a aprovação é invalidada e o
  checklist volta ao estado "não publicar" até nova avaliação.
- **Defeito grave depois da publicação**: aciona o plano de rollback. Os dados locais do usuário não são
  apagados nem migrados de forma irreversível pela ação de rollback.

## Out of Scope *(mandatory)*

- Novas funcionalidades de produto ou mudança de comportamento da Feature 001. Correções feitas aqui se
  limitam a bloqueadores encontrados nas validações e preservam os contratos da 001.
- Reimplementar qualquer item concluído da 001 (captura de microfone, pausa e retomada, análise, perguntas,
  exportação, exclusão, autorização remota, armazenamento cifrado).
- Backend obrigatório, telemetria, coleta de relatórios de falha ou qualquer serviço em nuvem operado pelo
  projeto. Hospedagem estática de artefatos e de metadados de atualização não é backend.
- Lojas de aplicativos (Mac App Store, Microsoft Store, Snap Store, Flathub) e seus requisitos de sandbox.
- Pacotes para todas as distribuições Linux. Só os formatos e distribuições listados como suportados no
  release.
- Arquiteturas ou versões de SO não testadas em hardware real. Elas não são declaradas suportadas.
- Implementar os adaptadores de áudio do sistema (T151–T153) ou oferecer essa fonte no primeiro release.
  Aqui entram só o spike S1 e o registro da decisão.
- Atualização automática silenciosa (instalar sem confirmação do usuário), canais beta/estável separados e
  atualização delta.
- Mudanças de arquitetura (novos processos, serviços locais, divisão do monólito).
- Afrouxar qualquer controle de segurança para facilitar a distribuição, incluindo exceções permanentes de
  política de conteúdo, permissões amplas de capability ou distribuição de artefatos não assinados.
- Certificação formal de acessibilidade ou segurança. As referências da constituição são aplicadas por
  relevância (Art. IV.9).
- Integrações com plataformas de reunião (fora do produto, conforme a constituição).

## Security & Privacy *(mandatory)*

- **Assets**: identidades e chaves de assinatura de código e de atualização; credenciais de contas de
  desenvolvedor das plataformas; artefatos de release e seus checksums; integridade da política de conteúdo do
  app; dados locais do usuário (sessões cifradas, chave no cofre do SO); conjuntos de avaliação e gravações de
  teste (não confidenciais).
- **Actors & trust boundaries**: responsável pelo release ↔ pipeline de build; pipeline ↔ serviços de
  assinatura e validação das plataformas; canal de distribuição ↔ usuário final; app instalado ↔ fonte de
  atualização (se houver); webview ↔ núcleo nativo (política de conteúdo e IPC, inalterados exceto o F-1);
  app ↔ provedor de análise real durante a avaliação (sob a autorização da 001/US7).
- **Abuse cases**: artefato adulterado no canal de distribuição; atualização maliciosa ou downgrade forçado
  para versão vulnerável; vazamento de chave de assinatura pelo log do pipeline ou pelo repositório; injeção
  de estilo/CSS para ocultar ou falsificar o indicador de captura; usuário orientado a desativar a proteção do
  SO; envio de dados de teste confidenciais ao provedor real; evidência de aceitação forjada ou incompleta
  usada para aprovar o release.
- **Permissions**: inalteradas em relação à 001, exceto a declaração de uso de microfone exigida pelo
  empacotamento assinado e o acesso de rede só ao destino de atualização documentado. Nenhuma declaração de
  áudio do sistema entra neste release. Toda declaração nova é justificada no checklist. A capability da
  webview continua mínima, e a atualização é decidida e verificada no núcleo nativo, não na webview.
- **Data handling**: nenhum dado de sessão sai da máquina por causa desta feature. A verificação de
  atualização envia só versão, plataforma e arquitetura, ao destino documentado, e pode ser desligada. Avaliações com provedor real usam só o conjunto de
  avaliação controlado e não confidencial. Evidências (capturas de tela, gravações) não contêm conteúdo
  confidencial nem credenciais.
- **Verification**: integridade e assinatura verificadas em cada SO (US1); política de conteúdo sem
  `'unsafe-inline'` e zero violações (US2); rejeição de atualização adulterada (US8); varredura de segredos no
  pipeline de release e nos logs de build; casos de segurança do Art. IV.16 reexecutados no build assinado
  (permissão negada, início/parada, redação de logs, exclusão, prompt injection). A revisão de segurança da
  001 é atualizada com a disposição final de F-1, F-4 e F-6 e uma nova aprovação explícita (Art. IV.10).

## Platform Impact *(mandatory)*

| Platform | Behavior / API | Acceptance verification |
|----------|----------------|-------------------------|
| macOS | Assinatura com identidade de desenvolvedor, runtime reforçado e validação (notarização) da Apple; declaração de uso de microfone; permissões ligadas à identidade assinada; áudio do sistema indisponível neste release (S1 só registrado no ADR-002) | Manual em hardware real (Apple Silicon; Intel se declarado): instalação limpa, matriz completa, VoiceOver |
| Windows | Instalador e executável assinados com editor verificado; componente de navegador embutido presente; configuração de privacidade do microfone para apps desktop; SQLCipher com criptografia embutida (ADR-004) | Manual em hardware real (Windows 10 22H2 e 11 se declarados): instalação limpa, matriz completa, Narrator e NVDA |
| Linux | Artefatos com assinatura e checksum verificáveis por ferramentas padrão; serviço de segredos obrigatório; Wayland e X11; PipeWire e/ou PulseAudio | Manual em hardware real por combinação declarada (no mínimo Ubuntu LTS Wayland e X11): matriz completa, Orca |

## Requirements *(mandatory)*

### Functional Requirements

**Segurança de distribuição**

- **FR-001**: O build de produção MUST aplicar uma política de conteúdo sem `'unsafe-inline'` e sem
  `'unsafe-eval'` em qualquer diretiva, e nenhuma outra diretiva MUST ficar mais permissiva que a atual.
- **FR-002**: Todas as telas, diálogos e estados da interface MUST manter aparência, comportamento e
  acessibilidade sob a nova política, com zero violações registradas ao percorrer a matriz em cada SO.
- **FR-003**: A política de conteúdo de desenvolvimento MAY continuar mais permissiva, mas a política de
  produção MUST ser a usada em toda validação desta feature.
- **FR-004**: Todo artefato distribuído MUST ser assinado conforme os requisitos da plataforma: identidade de
  desenvolvedor e validação da Apple no macOS; assinatura de código com editor verificado no Windows;
  checksum de cada artefato publicado num arquivo com assinatura OpenPGP destacada para o Linux.
- **FR-005**: O processo de release MUST falhar e não publicar nada quando qualquer assinatura, validação ou
  checksum falhar. Não MUST existir caminho documentado ou automatizado para publicar artefatos não assinados.
- **FR-006**: Os segredos de assinatura MUST ficar fora do repositório, dos logs do pipeline e dos artefatos,
  com acesso restrito ao responsável pelo release, e MUST passar pela varredura de segredos.
- **FR-007**: Cada artefato publicado MUST ter checksum publicado junto com a versão e MUST ser rastreável até
  o commit e o ambiente de build que o produziram (Art. IV.15).
- **FR-008**: O app MUST oferecer atualização que verifica criptograficamente cada pacote contra a chave de
  atualização do projeto antes de instalar, e MUST rejeitar pacotes não assinados, adulterados, assinados com
  outra chave ou de versão menor que a instalada.
- **FR-008a**: A instalação de uma atualização MUST exigir confirmação do usuário e MUST NOT começar enquanto
  uma sessão estiver capturando ou pausada.
- **FR-008b**: A verificação de versão nova MUST enviar só versão, plataforma e arquitetura ao destino
  documentado, por TLS autenticado, e MUST NOT enviar conteúdo de sessão, credenciais ou identificadores
  pessoais. O usuário MUST poder desligá-la nas Configurações, e desligada ela MUST NOT fazer nenhuma
  requisição. O app MUST informar ao usuário, na primeira execução, que verifica atualizações e para onde.
- **FR-008c**: A falha da verificação ou do download de atualização MUST NOT afetar captura, sessões nem
  outras funções do app, e MUST deixar a versão instalada íntegra.
- **FR-008d**: A chave privada de atualização MUST seguir as mesmas regras dos segredos de assinatura
  (FR-006), e o plano de rollback MUST cobrir sua perda ou comprometimento.
- **FR-009**: A documentação pública MUST explicar como verificar a autenticidade dos artefatos em cada SO
  sem pedir que o usuário desative controles de segurança do sistema.

**Validação multiplataforma**

- **FR-010**: A matriz de aceitação existente MUST ser executada integralmente no build candidato assinado em
  cada plataforma declarada (macOS, Windows, Linux Wayland, Linux X11), em hardware real.
- **FR-011**: A execução MUST cobrir permissão de microfone (conceder, negar, revogar), iniciar, pausar,
  retomar e parar a captura, troca de dispositivo (incluindo Bluetooth), suspensão e bloqueio do SO,
  encerramento forçado com recuperação de sessão, e cofre de credenciais indisponível.
- **FR-012**: A acessibilidade MUST ser validada só com teclado e com VoiceOver, Narrator, NVDA e Orca nas
  respectivas plataformas, incluindo o anúncio textual do estado da captura.
- **FR-013**: Cada caso executado MUST registrar resultado, ambiente (versão do SO, hardware, leitor de tela,
  build e commit), executor, data e evidência. Um caso falho MUST apontar para um problema registrado com
  classificação de severidade.
- **FR-014**: Resultados de testes automatizados ou de CI MUST NOT ser usados como evidência de compatibilidade
  com uma plataforma nem substituir casos da matriz.
- **FR-015**: Cada plataforma MUST ter um registro de limitações conhecidas (permissões, compatibilidade,
  desempenho) publicado nas notas de release.
- **FR-016**: O armazenamento cifrado e o cofre de credenciais (ADR-004) MUST ser verificados no Windows e no
  Linux reais, e o ADR-004 atualizado com o resultado.

**Qualidade e desempenho**

- **FR-017**: Um plano de medição MUST ser aprovado e registrado antes da primeira medição, com metas
  numéricas, método, hardware, condições de repouso e tamanho de amostra para: latência de transcrição local,
  CPU e memória em sessão prolongada, latência dos controles e qualidade da análise.
- **FR-018**: O hardware representativo MUST incluir, por SO, uma máquina de referência e uma máquina de
  especificação mínima declarada, ambas registradas no plano.
- **FR-019**: A latência de transcrição local MUST ser medida com fala real não confidencial, no app
  instalado, em cada SO, e registrada no ADR-003 contra 001/SC-005.
- **FR-020**: O consumo de CPU e memória MUST ser medido numa sessão contínua de pelo menos 2 horas no app
  instalado em cada SO, com captura e transcrição local ativas.
- **FR-021**: A análise técnica MUST ser avaliada com pelo menos um provedor real e o conjunto de avaliação
  controlado (001/T083, T110, T111a), e as métricas MUST ser registradas no ADR-003.
- **FR-022**: Cada resultado fora da meta MUST ser classificado como bloqueador ou como limitação documentada,
  com justificativa e aprovação do responsável.

**Áudio do sistema**

- **FR-023**: O spike S1 (T150) MUST seguir o protocolo do ADR-002 em hardware real, e o resultado por SO
  (aprovado, reprovado ou adiado) MUST ser registrado no ADR-002.
- **FR-024**: O primeiro release MUST NOT oferecer nem anunciar áudio do sistema em nenhuma plataforma, seja
  qual for o resultado do S1. Os adaptadores T151–T153 ficam para uma feature futura.
- **FR-025**: Em todas as plataformas, o app MUST manter a fonte indisponível com motivo e a sessão com
  microfone (comportamento atual da 001), verificado pelo caso 12 da matriz.
- **FR-026**: As limitações de permissão e compatibilidade do áudio do sistema encontradas no S1 (versões
  mínimas do SO, permissões exigidas, combinações de pilha de áudio) MUST ser documentadas por plataforma no
  ADR-002, junto com os critérios que uma feature futura MUST cumprir para declarar suporte.

**Release**

- **FR-027**: MUST existir um checklist de release versionado que liste cada critério de publicação e aponte
  a evidência correspondente.
- **FR-028**: MUST existir um plano de rollback documentado cobrindo: retirada de artefatos, comunicação aos
  usuários, defeito grave, comprometimento de identidade de assinatura ou da chave de atualização,
  suspensão da oferta de uma atualização já publicada aos apps instalados, e compatibilidade de dados locais
  entre versões.
- **FR-029**: Empacotamento, assinatura e distribuição MUST estar documentados a ponto de outra pessoa do
  projeto reproduzir o release.
- **FR-030**: A publicação MUST ser bloqueada enquanto houver bloqueador crítico aberto. MUST ser bloqueadores
  críticos, no mínimo: F-1, F-4 e F-6 não resolvidos; qualquer captura sem início explícito ou após negativa;
  falha de assinatura ou verificação; falha na recuperação de sessão; vazamento de conteúdo ou credenciais em
  logs ou armazenamento; estado de captura não perceptível por leitor de tela ou teclado.
- **FR-031**: A aprovação de publicação MUST registrar responsável, data, versão, commit, checksums,
  disposição de cada achado de segurança e limitações por plataforma, e MUST ser invalidada se um bloqueador
  crítico surgir antes da publicação.
- **FR-032**: As notas de release e os READMEs MUST declarar como suportadas só as plataformas, versões e
  recursos aprovados com evidência (Art. IX.4).
- **FR-033**: A revisão de segurança MUST ser atualizada com a disposição final de F-1, F-4 e F-6 e receber
  nova aprovação explícita antes da publicação (Art. IV.10).

### Key Entities

- **Build candidato**: um conjunto de artefatos produzido de um commit identificado, com versão, checksums,
  estado de assinatura por plataforma e ambiente de build.
- **Registro de evidência**: o resultado de um caso da matriz, medição ou verificação, ligado a um build
  candidato, a um ambiente (SO, versão, hardware, leitor de tela), a um executor, a uma data e a anexos.
- **Plano de medição**: as metas, métodos, hardware e amostras definidos antes da medição, com data de
  aprovação e histórico de alterações justificadas.
- **Declaração de suporte por plataforma**: o que é suportado em cada SO (versões, arquiteturas, recursos como
  áudio do sistema) e as limitações conhecidas, sempre derivado de registros de evidência.
- **Problema/bloqueador**: um defeito encontrado na validação, com severidade, plataforma, estado e decisão
  (corrigir, aceitar como limitação).
- **Checklist de release**: os critérios de publicação com o estado de cada um e a referência à evidência.
- **Decisão de publicação**: a aprovação ou recusa registrada, com responsável, data e o build candidato a que
  se refere.
- **Plano de rollback**: os gatilhos, passos, responsáveis e comunicação para retirar ou substituir um release.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Em 100% das plataformas declaradas, um usuário instala o app num computador limpo seguindo só a
  documentação pública, sem desativar nem contornar nenhum aviso ou controle de segurança do SO.
- **SC-002**: 100% dos artefatos publicados passam na verificação de assinatura e checksum, e 100% dos
  artefatos adulterados de teste são rejeitados, em cada SO.
- **SC-003**: Zero violações de política de conteúdo e zero ocorrências de `'unsafe-inline'` no build de
  produção. Em cada SO, as telas ficam iguais à referência visual daquele SO e o fluxo só com teclado e os
  anúncios do leitor de tela (casos 11 e 11b) continuam funcionando no build de produção.
- **SC-004**: 100% dos casos da matriz de aceitação têm resultado registrado com evidência em cada plataforma
  declarada, e zero casos falhos de severidade crítica ficam abertos.
- **SC-005**: Zero ocorrências de áudio captado sem início explícito ou após negativa de permissão em toda a
  matriz executada no build assinado.
- **SC-006**: Em cada SO, os controles de captura respondem em até 500 ms e o indicador desliga em até 1 s
  depois de parar, em pelo menos 95% das tentativas medidas.
- **SC-007**: No hardware de referência de cada SO, com o modelo de transcrição padrão, parciais aparecem em
  até 5 s e finais em até 15 s em 95% dos trechos. No hardware mínimo, o resultado é registrado e, se ficar
  fora da meta, aparece como limitação com o modelo recomendado.
- **SC-008**: Numa sessão contínua de 2 horas no app instalado, sem travamentos, a memória residente fica no
  máximo 10% acima do valor medido após 10 minutos de aquecimento, e a CPU média fica dentro da meta definida
  no plano de medição antes do teste.
- **SC-009**: Com um provedor de análise real no conjunto de avaliação controlado: pelo menos 80% de recall de
  requisitos e decisões explícitos, 100% dos registros "observados" com evidência que os sustenta, 100% dos
  casos de prompt injection sem mudança de status ou ação externa, e zero fatos inventados em perguntas sem
  resposta na sessão.
- **SC-010**: Em 100% das plataformas, o estado da captura é anunciado por texto pelo leitor de tela daquela
  plataforma, e o fluxo iniciar → pausar → retomar → parar → revisar → exportar é concluído só com teclado.
- **SC-011**: Zero plataformas, versões ou recursos (incluindo áudio do sistema) anunciados como suportados sem
  registro de evidência em hardware real.
- **SC-012**: Uma pessoa do projeto que não executou o release anterior reproduz o empacotamento, a
  assinatura e a publicação num canal de teste seguindo só a documentação, sem pedir ajuda.
- **SC-013**: Um ensaio de rollback num canal de teste retira os artefatos e publica a orientação aos usuários
  em até 2 horas depois do acionamento, sem perda de dados locais nos computadores de teste.
- **SC-014**: Em cada SO, 100% das atualizações legítimas de teste são instaladas depois da confirmação, 100%
  dos pacotes adulterados, assinados com outra chave ou de versão menor são rejeitados sem alterar a versão
  instalada, e zero requisições de atualização são observadas com a verificação desligada.
- **SC-015**: A decisão de publicação registra 100% dos campos exigidos (responsável, data, versão, commit,
  checksums, disposição dos achados e limitações) antes de qualquer artefato ficar público.

## Assumptions

- **Plataformas e versões candidatas** seguem a 001/research R16: macOS 13+ (Apple Silicon e Intel), Windows
  10 22H2+/11, Ubuntu 22.04+/Fedora 39+ com WebKitGTK 4.1. Só as combinações efetivamente testadas em
  hardware real são declaradas suportadas.
- **Hardware de referência**: Apple M4 com 16 GB (já medido no ADR-003) para macOS. Para Windows e Linux, uma
  máquina x86-64 de 8 núcleos com 16 GB, de 2022 ou mais nova. **Hardware mínimo**: 4 núcleos, 8 GB de RAM,
  de 2019 ou mais novo. O plano de medição pode refinar isso antes das medições.
- **Metas iniciais de CPU** a confirmar no plano de medição: com o app aberto e sem captura, até 2% de CPU
  média; com captura e transcrição local pelo modelo padrão no hardware de referência, até 50% da capacidade
  total de CPU em média.
- **Provedor real de análise**: o provedor local de referência (Ollama, 001/ADR-003) é obrigatório por ser
  local-first. Um provedor remoto MAY ser avaliado também, sob a autorização da 001/US7 e só com dados não
  confidenciais.
- **Distribuição**: um release público para download direto pela página de releases do repositório
  (hospedagem estática), que também hospeda os metadados assinados de atualização. Não há loja de aplicativos
  nem servidor próprio.
- **Formatos Linux**: um formato portátil e um pacote para distribuições baseadas em Debian, ambos com
  assinatura destacada e checksum. Outros formatos ficam para depois.
- **Identidades de assinatura** (conta de desenvolvedor Apple e certificado de assinatura de código para
  Windows) serão adquiridas pelo responsável pelo release. O custo e o prazo de emissão são dependências
  externas.
- **Conjuntos de avaliação**: os fixtures não confidenciais da 001 (T083, T110, T111a) são o conjunto
  controlado. A fala real usada nas medições é gravada pelo próprio time a partir de roteiros não
  confidenciais.
- **Severidade**: crítico = viola segurança, privacidade, consentimento ou integridade de dados, ou impede o
  fluxo principal numa plataforma declarada; alto = degrada um fluxo com contorno; médio/baixo = cosmético ou
  raro. Só crítico bloqueia por definição. Um alto bloqueia se o responsável decidir.

## Dependencies

- Acesso a hardware real: Mac Apple Silicon (e Intel, se declarado), PC Windows 10 e 11, PC Linux com Wayland
  e X11, dispositivos Bluetooth de áudio e microfones USB/integrados.
- Conta de desenvolvedor Apple ativa e serviço de validação da Apple disponível. Certificado de assinatura de
  código para Windows emitido por autoridade reconhecida.
- Leitores de tela instalados e configurados: VoiceOver, Narrator, NVDA e Orca.
- Ollama (ou outro provedor local de referência) com o modelo definido no plano de medição, e opcionalmente
  credenciais de um provedor remoto.
- CI existente da 001 (`.github/workflows/ci.yml`) com matriz macOS, Windows e Linux para build e gates.
- Documentos da 001: `acceptance-matrix.md`, `quickstart.md`, `security-review.md`, `docs/local-data.md`,
  `docs/permissions.md`; ADR-002 (protocolo S1), ADR-003 (medições), ADR-004 (armazenamento).
- Pessoas disponíveis para executar a matriz (testador) e aprovar (responsável pelo release).

## Risks

| Risco | Impacto | Probabilidade | Mitigação |
|-------|---------|---------------|-----------|
| O runtime do app injeta estilos que a política estrita bloqueia, e não há como permiti-los sem `'unsafe-inline'` | Alto: o F-1 fica sem solução e bloqueia o release | Média | Investigar cedo (antes da matriz), usar permissões específicas por conteúdo e escalar com ADR se a única saída for uma exceção. Exceção só pelo processo formal, com prazo |
| Prazo ou custo para obter as identidades de assinatura (Apple e Windows) | Alto: bloqueia US1 e o release | Média | Iniciar a aquisição no começo da feature e preparar o pipeline com identidades de teste enquanto isso |
| Comportamento de permissão diferente no build assinado | Alto: a captura falha em silêncio | Média | Matriz sempre no build assinado. Caso específico de primeira permissão em instalação limpa |
| Desempenho insuficiente no hardware mínimo do Windows/Linux | Médio | Média | Metas por hardware, recomendação de modelo por perfil, limitação documentada |
| Diversidade do Linux (Wayland/X11, PipeWire/PulseAudio, serviço de segredos) | Médio: suporte parcial | Alta | Declarar só as combinações testadas e registrar limitações por combinação |
| S1 reprovado em todas as plataformas | Baixo para o release (sai só com microfone); alto para o valor percebido | Média | Comunicar a limitação nas notas e manter a fonte indisponível com motivo |
| Avisos de reputação do Windows apesar da assinatura | Médio: o usuário desconfia | Alta no começo | Documentar o comportamento esperado e nunca orientar a desativar a proteção |
| Vazamento de chave de assinatura | Crítico | Baixa | Segredos fora do repositório, varredura, acesso restrito e plano de rotação/revogação no rollback |
| Evidências incompletas ou inconsistentes entre executores | Médio: aprovação frágil | Média | Modelo único de registro e revisão do responsável antes da aprovação |
| Avisos transitivos de dependências (F-3) virarem vulnerabilidades antes do release | Médio | Baixa | `cargo audit` no pipeline de release, que bloqueia se surgir vulnerabilidade |
| Perda ou comprometimento da chave de atualização | Crítico: sem atualizar os apps instalados, ou com atualização maliciosa | Baixa | Guarda separada dos segredos de assinatura de código, cópia de segurança offline, procedimento de rotação documentado e ensaiado no canal de teste |
