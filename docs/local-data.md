# Dados locais do Dev Copilot

Como o aplicativo guarda dados neste computador: formato, cifragem, migrações, classificação, retenção e os
limites da exclusão. Normas: Constitution Art. IV.12, ADR-004, spec 001 (FR-020, FR-039–FR-044, FR-049).

## Onde ficam

O diretório de dados é o `app_data_dir` do Tauri para o identificador `com.drewed.devcopilot`:

| SO | Dados | Logs de diagnóstico |
|----|-------|---------------------|
| macOS | `~/Library/Application Support/com.drewed.devcopilot/` | `~/Library/Logs/com.drewed.devcopilot/` |
| Windows | `%APPDATA%\com.drewed.devcopilot\` | `%LOCALAPPDATA%\com.drewed.devcopilot\logs\` |
| Linux | `$XDG_DATA_HOME/com.drewed.devcopilot/` (padrão `~/.local/share/…`) | `$XDG_DATA_HOME/com.drewed.devcopilot/logs/` |

| Arquivo | Conteúdo | Cifrado |
|---------|----------|---------|
| `devcopilot.db`, `devcopilot.db-wal`, `devcopilot.db-shm` | sessões, transcrições finais, registros e histórico, perguntas, execuções de análise, provedores, autorizações, exportações (sem caminho), configurações | sim (SQLCipher, banco inteiro, inclusive WAL) |
| `models/*.bin` | modelos de transcrição local baixados por ação do usuário (catálogo fixo, SHA-256 verificado) | não — não são dados de sessão |
| logs `devcopilot.log.*` | ids, códigos de estado e de erro; nunca conteúdo de sessão nem segredos (FR-051) | não |

O diretório de dados é criado com permissão `0700` e o banco com `0600` (somente o usuário) em macOS e Linux.

**Nunca gravados em disco:** áudio bruto ou WAV (FR-020; o áudio vive só em filas limitadas na memória e, com
provedor remoto autorizado, sai apenas no corpo da requisição), texto parcial de transcrição (FR-017) e chaves de
API.

## Cifragem e chave

- SQLite com SQLCipher (`rusqlite` + `bundled-sqlcipher`), chave aleatória de 256 bits gerada na primeira
  execução com o CSPRNG do SO.
- A chave fica no cofre de credenciais do SO (Keychain, Credential Manager, Secret Service) sob o serviço
  `dev.devcopilot.desktop`, nunca no disco nem no código (ADR-004).
- Sem cofre disponível o app não abre nem cria o banco e bloqueia a captura, com explicação (FR-039a).
- Perder a entrada do cofre torna o banco ilegível; não há recuperação por design.
- Chaves de API de provedores remotos ficam no mesmo cofre (`provider:<id>`); o banco guarda só a referência
  (FR-049).

Verificação automatizada: `crates/persistence/tests/at_rest_scan.rs` procura frases conhecidas nos arquivos do
banco e do WAL (SC-018) e confirma que nenhum áudio é gravado nos diretórios de dados, cache e temporário,
inclusive durante o envio de trechos a um provedor STT remoto.

## Formato e migrações

- Esquema em `crates/persistence/src/migrations/` (`v1.sql`, …); a versão aplicada fica em
  `PRAGMA user_version`.
- **Somente para frente:** ao abrir, cada migração pendente roda em sua própria transação (`BEGIN IMMEDIATE`
  … `PRAGMA user_version = N` … `COMMIT`). Se uma falha, ela é revertida, o banco fica na última versão
  completa e o app informa erro de armazenamento sem gravar nada.
- **Banco mais novo que o app** (`user_version` maior que a última migração conhecida): o app recusa abrir,
  informando erro de armazenamento, em vez de arriscar corromper dados; atualize o app.
- **Sem cópia automática antes de migrar**, por decisão: uma cópia do banco manteria sessões que o usuário
  depois excluísse, contrariando FR-042/FR-043. Para guardar uma cópia antes de atualizar o app, feche-o e
  copie os três arquivos `devcopilot.db*` juntos; a cópia continua cifrada e depende da mesma entrada do cofre.
- Novas migrações são aditivas, testadas em banco novo e existente (`crates/persistence/tests/migrations.rs`)
  e nunca reescrevem `vN.sql` já publicada.

## Classificação dos dados

| Classe | Exemplos | Onde | Sai da máquina? |
|--------|----------|------|-----------------|
| Conteúdo de sessão (sensível) | trechos finais, registros, histórico, plano, perguntas e respostas, título | banco cifrado | só para provedor remoto **autorizado**, apenas nas categorias declaradas (FR-046); e em exportações escolhidas pelo usuário |
| Áudio (sensível) | microfone, áudio do sistema | só memória | só para provedor STT remoto autorizado (categoria `audio`) |
| Segredos | chave do banco, chaves de API | cofre do SO | chaves de API vão apenas no cabeçalho da requisição ao próprio provedor |
| Metadados operacionais | ids, estados, códigos de erro, contagem de tokens | banco e logs | não |
| Configuração não sensível | idioma da UI, limites de análise, provedores padrão | banco cifrado | não |

## Retenção e exclusão

- Sessões ficam até o usuário excluí-las; não há expiração automática no MVP.
- Exclusão (FR-042): remove a sessão e tudo o que pertence a ela (fontes, trechos, registros e histórico,
  execuções, perguntas, registros de exportação) numa transação com `ON DELETE CASCADE`, com
  `secure_delete=ON` e `PRAGMA wal_checkpoint(TRUNCATE)` em seguida. Análises e perguntas em andamento da
  sessão são canceladas e descartadas antes.
- **Limites, exibidos no diálogo de exclusão:**
  - em SSD e sistemas de arquivos com cópia na escrita, a sobrescrita não garante que blocos antigos sejam
    irrecuperáveis no nível físico (o conteúdo continua cifrado);
  - backups do sistema (Time Machine, Histórico de Arquivos, snapshots) feitos antes da exclusão continuam
    contendo o banco cifrado da época;
  - arquivos exportados não são cifrados, ficam onde o usuário os salvou e não são excluídos pelo app;
  - dados já enviados a um provedor remoto autorizado seguem a política de retenção desse provedor.
- Revogar a autorização de um provedor impede novos envios, mas não apaga o que ele já recebeu.
