# Security review — 001-meeting-sessions

- **Data:** 2026-10-28
- **Escopo:** branch `001-meeting-sessions` (Fases 1–12), núcleo Rust, comandos IPC, frontend, dependências.
- **Referências (aplicadas por relevância, Art. IV.9):** OWASP Top 10 (2021), OWASP ASVS 4.0 (controles de
  fronteira de confiança/IPC), OWASP Top 10 for LLM Applications (2025), CWE Top 25 (2024), NIST SSDF
  SP 800-218. Não é certificação; é verificação dirigida pelo threat model de `plan.md`.
- **Método:** leitura do código nas fronteiras (IPC, rede, disco, cofre, provedores de IA), conferência das
  capabilities e da CSP, execução dos gates (`cargo audit`, `cargo deny`, `pnpm audit`, clippy, testes) e dos
  testes de aceitação de segurança (Art. IV.16), e `gitleaks` no histórico e na árvore de trabalho.

Disposições: **OK** (controle presente e verificado), **Corrigido** (achado corrigido nesta fase),
**Aceito** (risco residual aceito, com motivo), **Pendente** (exige ação fora deste ambiente).

## 1. Threat model (`plan.md`) — verificação

| Ameaça | Mitigação conferida | Verificação automatizada | Disposição |
|--------|---------------------|--------------------------|------------|
| Captura sem início explícito ou após parar | só `session_start`/`session_resume` abrem fontes; `stop`/`pause` fecham antes de retornar; token por captura | `session_service.rs`, `pause_resume.rs` | OK |
| Captura após negativa/revogação | permissão consultada no start e no resume; revogação fecha só a fonte afetada | `session_service.rs`, `dual_source.rs`, `pause_resume.rs` | OK (matriz manual pendente, §7) |
| Prompt injection via transcrição/perguntas | dados delimitados, sem ferramentas, schema estrito, evidência verificada, IA só cria propostas | `intelligence/tests/prompt_injection.rs`, `output_validation.rs` | OK |
| Envio remoto não autorizado / fallback | `RemoteGate` antes de cada requisição, lido do armazenamento; gate diferido nega por padrão; nunca troca para outro remoto | `remote_gate.rs` (application e intelligence, com `wiremock`), `provider_fallback.rs` | OK |
| Vazamento em logs | `Redacted<T>`; erros sem URL, corpo ou chave | `src-tauri/tests/log_redaction.rs` (nível TRACE) | OK |
| Leitura do banco por terceiros | SQLCipher, chave no cofre, `0700`/`0600` | `persistence/tests/at_rest_scan.rs` (DB e WAL) | OK |
| Exclusão incompleta | cascata, `secure_delete`, checkpoint TRUNCATE, cancelamento de trabalho em andamento | `delete_session.rs` (persistence e application) | OK; limites documentados em `docs/local-data.md` |
| Escrita fora do destino de exportação | diálogo aberto pelo núcleo; escrita só no caminho retornado; webview sem `dialog`/`fs` | `export_command.rs`; capability `core:default` | OK |
| IPC malicioso/malformado | DTOs com `deny_unknown_fields`, limites e enums validados no núcleo; estado validado no núcleo | testes de comando e de contrato | OK |
| Modelo local adulterado | URL com revisão fixa + SHA-256; download só por ação explícita | `model_download.rs` | OK |
| Áudio gravado em disco | áudio só em filas limitadas; upload remoto em memória | `at_rest_scan.rs` (dados, cache, temp) | OK |

## 2. OWASP Top 10 (por relevância)

| Item | Achado | Disposição |
|------|--------|------------|
| A01 Broken Access Control | Autorização de provedor e transições de sessão decididas no núcleo, independentes do frontend (Art. IV.11); webview com `core:default` apenas. | OK |
| A02 Cryptographic Failures | SQLCipher com chave de 256 bits do CSPRNG no cofre do SO; TLS verificado (`rustls`), `https_only` nos adaptadores remotos. Exportações não são cifradas — por decisão, com aviso explícito (FR-041). | OK / Aceito (exportação) |
| A03 Injection | SQL só com parâmetros (`rusqlite` `params!`); Markdown exportado escapado; React sem `dangerouslySetInnerHTML`; nomes de arquivo por `slug` (sem `../`). | OK |
| A04 Insecure Design | Threat model por fronteira; nega por padrão (gate diferido, cofre indisponível bloqueia). | OK |
| A05 Security Misconfiguration | CSP de produção sem `unsafe-eval` e sem destinos remotos em `connect-src`, mas com `style-src 'unsafe-inline'`. O código não usa estilos inline (verificado); remover depende de confirmar no app em execução que o runtime do Tauri não injeta estilos sem hash. | Pendente (F-1) |
| A06 Vulnerable Components | `cargo audit`: 0 vulnerabilidades; 5 avisos *unmaintained/unsound* transitivos da pilha GTK/D-Bus do Linux (Tauri, `keyring`). `cargo deny`: ok. `pnpm audit`: 6 vulnerabilidades em dependências de desenvolvimento (ESLint) — corrigidas atualizando o lockfile. | Corrigido (JS) / Aceito (avisos transitivos, F-3) |
| A07 Identification & Auth Failures | Chaves de API só no cofre, enviadas como cabeçalho sensível ao próprio provedor; nunca retornadas pela IPC. | OK |
| A08 Software & Data Integrity | Modelo com SHA-256 fixo; lockfiles versionados. Assinatura de artefatos e atualização verificada ainda não existem (sem release). | Pendente (F-4) |
| A09 Logging & Monitoring Failures | Logs com ids e códigos; falhas de gate registradas sem conteúdo. | OK |
| A10 SSRF | Endpoints remotos são escolhidos pelo usuário e exigem autorização mostrando o destino; locais restritos a loopback; sem redirects. | OK / Aceito (destino privado escolhido e autorizado pelo usuário) |

## 3. OWASP ASVS (fronteira IPC/UI)

| Controle | Evidência | Disposição |
|----------|-----------|------------|
| V5 validação de entrada no servidor (núcleo) | DTOs `deny_unknown_fields` (todos os DTOs de entrada; o único sem é `IpcSessionEvent`, só de saída); limites de texto do domínio; ids UUID validados | OK |
| V4 controle de acesso no núcleo | estado da sessão, autorização de provedor e `accepted: true`/`confirmed: true` exigidos no núcleo | OK |
| V7 erros sem dados sensíveis | `IpcError` só com código, chave i18n e detalhes não sensíveis; `storage_error` sem caminho | OK |
| V8 proteção de dados | classificação e retenção em `docs/local-data.md`; segredos no cofre | OK |
| V9 comunicação | TLS verificado, `https_only`, sem redirects; Ollama sem proxy | OK |
| V14 configuração | capability mínima, CSP restrita (ver F-1) | OK / F-1 |

## 4. OWASP Top 10 for LLM Applications (2025)

| Item | Achado | Disposição |
|------|--------|------------|
| LLM01 Prompt Injection | instruções fixas separadas dos dados delimitados; 22 casos de injeção na análise e 16 nas perguntas | OK |
| LLM02 Sensitive Information Disclosure | só categorias declaradas e autorizadas são enviadas; o adaptador remove o resto antes do gate | OK |
| LLM05 Improper Output Handling | saída aceita só como JSON válido no schema; ids de evidência verificados; texto exibido como texto | OK |
| LLM06 Excessive Agency | nenhuma ferramenta/função exposta; a IA só cria propostas `proposed` (FR-052) | OK |
| LLM07 System Prompt Leakage | o prompt não contém segredos | OK |
| LLM09 Misinformation | evidência obrigatória ou `no_evidence`; avaliação SC-011 | OK (métricas com provedor real pendentes em ADR-003) |
| LLM10 Unbounded Consumption | limites de chamadas/hora e tokens por sessão para análise. Perguntas não contam no limite por hora; uma por sessão em andamento, sempre por ação do usuário. | Aceito (F-2) |

## 5. CWE Top 25 (por relevância)

| CWE | Achado | Disposição |
|-----|--------|------------|
| CWE-22 Path Traversal | exportação só no caminho do diálogo nativo; nome sugerido por `slug` | OK |
| CWE-78/77 Command Injection | nenhum shell ou execução de processo; texto da IA nunca executado | OK |
| CWE-79 XSS | React escapa; sem HTML cru; CSP restrita | OK |
| CWE-89 SQL Injection | só consultas parametrizadas | OK |
| CWE-200/532 exposição em logs | teste de redação em nível TRACE | OK |
| CWE-306/862 autorização ausente | gate e estado no núcleo | OK |
| CWE-400/770 consumo de recursos | filas de áudio e backlog limitados; teste de 2 h (`soak.rs`) | OK |
| CWE-502 desserialização | `serde` com tipos fechados e `deny_unknown_fields`; saída de IA validada por schema antes | OK |
| CWE-798 credenciais fixas | nenhuma chave no código; chave do banco gerada por instalação | OK |
| CWE-120/787 memória | `unsafe` proibido no workspace (`unsafe_code = "deny"`); nenhum bloco `unsafe` | OK |

## 6. NIST SSDF (SP 800-218)

| Prática | Evidência | Disposição |
|---------|-----------|------------|
| PO.1/PO.5 requisitos e ambiente | constituição, spec com FRs de segurança, threat model; CI com permissões `contents: read` | OK |
| PS.1/PS.2 proteção do código e integridade | lockfiles, `cargo deny` (fontes e licenças) | OK; assinatura de release pendente (F-4) |
| PW.4/PW.7 revisão e testes | clippy `-D warnings`, testes de segurança de aceitação (Art. IV.16), esta revisão | OK |
| PW.8 testes de vulnerabilidade | `cargo audit`, `pnpm audit`, gitleaks (CI e local) | OK |
| RV.1 resposta a vulnerabilidades | avisos tratados com disposição e responsável neste documento | OK |

## 7. Achados abertos

| Id | Achado | Risco | Responsável | Mitigação / próximo passo | Prazo |
|----|--------|-------|-------------|---------------------------|-------|
| F-1 | `style-src 'unsafe-inline'` na CSP de produção | Baixo: sem execução de script; o código não usa estilos inline | Equipe do projeto | Remover e validar no app em execução (T166) em cada SO | antes do primeiro release |
| F-2 | Perguntas fora do limite de chamadas por hora | Baixo: ação do usuário, uma por vez por sessão; custo em provedor remoto | Equipe do projeto | Avaliar limite próprio para perguntas se houver uso abusivo | revisão após o MVP |
| F-3 | Avisos *unmaintained/unsound* transitivos (zbus/zvariant, glib, proc-macro-error) | Baixo: código de terceiros na pilha Linux; sem vulnerabilidade conhecida | Equipe do projeto | Acompanhar atualizações do Tauri e do `keyring`; `cargo audit` no CI | revisão mensal |
| F-4 | Sem assinatura de artefatos nem atualização verificada | Médio no release; inexistente hoje (sem distribuição) | Equipe do projeto | Definir assinatura por SO e updater antes de distribuir (Art. IV.15) | antes do primeiro release |
| F-5 | gitleaks não instalado localmente | — | Equipe do projeto | Resolvido: gitleaks 8.30.1 instalado; histórico (17 commits) e árvore de trabalho sem vazamentos | resolvido em 2026-10-28 |
| F-6 | Matriz manual por SO (permissões, troca de dispositivo, leitores de tela) não executada | Médio: comportamento de SO real não verificado fora do macOS | Equipe do projeto | Executar `acceptance-matrix.md` (T162) | antes do primeiro release |

## 8. Aprovação explícita (Art. IV.10)

A feature acessa áudio, arquivos, rede, credenciais e IA, portanto não pode ser mesclada sem aprovação
explícita desta revisão.

- **Status:** Aprovado com condições.
- **Condições:** achados F-1 a F-6 aceitos com os prazos acima; nenhum bloqueia o merge da feature;
  F-1 (CSP), F-4 (assinatura e atualização) e F-6 (matriz por SO) bloqueiam o primeiro release.
- **Aprovado por:** Eder Andrew (responsável pelo projeto)
- **Data:** 2026-10-28
