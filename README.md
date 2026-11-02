# Dev Copilot

Assistente de engenharia desktop multiplataforma (macOS, Windows e Linux). O objetivo é que o
usuário inicie explicitamente a captura autorizada de áudio do sistema e/ou microfone, e o
aplicativo transcreva conversas técnicas, extraia requisitos, riscos e decisões, mantenha um
plano de desenvolvimento incremental e responda perguntas contextuais.

O Dev Copilot **não** se integra com plataformas de reunião, **não** exige backend em nuvem e
**não** executa código ou comandos de forma autônoma.

> **Status: MVP da feature `001-meeting-sessions` implementado, ainda não lançado.** Sem pacote
> publicado; a matriz de aceitação por SO e o spike de áudio do sistema estão pendentes (abaixo).

### O que já funciona

- **Sessões**: criar, iniciar com lembrete de consentimento, pausar/retomar (lacuna `paused` na
  transcrição), parar, renomear, excluir; indicador de captura persistente e acessível; recuperação de
  sessões interrompidas por encerramento inesperado.
- **Captura**: microfone via `cpal` em macOS, Windows e Linux; áudio nunca gravado em disco. Com duas
  fontes, cada trecho indica sua origem e a falha de uma não para a outra.
- **Transcrição**: local com whisper.cpp (modelo baixado por ação explícita, SHA-256 verificado) ou remota
  compatível com OpenAI (só texto final), somente após autorização; texto parcial e final no modo local, lacunas por pausa, atraso ou falha.
- **Análise técnica incremental**: requisitos, tarefas, decisões, riscos, premissas e perguntas em aberto
  como propostas com evidência; revisão humana (confirmar, rejeitar, editar, criar), histórico de mudanças e
  plano de desenvolvimento; análise final sob demanda. Provedores: Ollama local, Anthropic ou chat
  compatível com OpenAI.
- **Perguntas sobre a sessão** com citações dos trechos ou declaração explícita de ausência de evidência.
- **Exportação** em Markdown e JSON para um caminho escolhido pelo usuário.
- **Provedores remotos** só depois de autorização informada (destino, categorias, retenção), com revogação;
  chaves de API no cofre do SO.
- **Armazenamento** local cifrado (SQLCipher, chave no cofre do SO); interface em pt-BR e inglês.

### O que ainda não existe

- **Áudio do sistema**: a opção aparece indisponível com o motivo em todos os SOs até o spike S1 validar um
  adaptador em hardware real ([ADR-002](docs/adr/ADR-002.md)).
- Matriz de aceitação manual por SO (leitores de tela, troca de dispositivo, suspensão) ainda não executada;
  latência de transcrição medida apenas em macOS ([ADR-003](docs/adr/ADR-003.md)).
- Instaladores, assinatura e atualização automática.

Detalhes de armazenamento e permissões: [docs/local-data.md](docs/local-data.md) e
[docs/permissions.md](docs/permissions.md).

## Princípios

- **Local-first**: o conteúdo das sessões fica na máquina do usuário; qualquer envio a um
  provedor remoto exige configuração e autorização explícitas.
- **Consentimento e controle humano**: captura só por ação do usuário, com estado visível e
  parada efetiva; sugestões da IA são consultivas e nunca sobrescrevem decisões confirmadas.
- **Segurança por padrão**: menor privilégio no Tauri, validação em todas as fronteiras IPC,
  segredos no cofre do sistema operacional, saídas de LLM tratadas como não confiáveis.

As regras completas estão na [Constituição](.specify/memory/constitution.md) e as decisões
arquiteturais iniciais no [ADR-001](docs/adr/ADR-001.md).

## Arquitetura

Monólito modular: núcleo em Rust (Cargo Workspace) com interface Tauri 2 + React + TypeScript.

```text
apps/desktop/          App Tauri 2 + React/Vite
  src/                 Interface (apresentação apenas, sem regras de negócio)
  src-tauri/           Raiz de composição e comandos Tauri finos
crates/
  domain/              Entidades, value objects e invariantes (sem dependências de infra)
  application/         Casos de uso e portas
  audio/               Adaptadores de captura por SO
  intelligence/        Adaptadores de transcrição e LLM
  persistence/         Repositórios SQLite cifrados e cofre de credenciais
packages/              Pacotes TypeScript compartilhados
docs/adr/              Architecture Decision Records
.specify/              Constituição e templates do Spec Kit
```

Direção de dependências: `domain` ← `application` ← infraestrutura (`audio`, `intelligence`,
`persistence`) ← composição em `apps/desktop/src-tauri`.

## Stack

| Camada        | Tecnologia                                  |
| ------------- | ------------------------------------------- |
| Desktop       | Tauri 2                                     |
| Interface     | React 19 + TypeScript + Vite                |
| Núcleo        | Rust (edition 2024), Cargo Workspace        |
| Persistência  | SQLite cifrado com SQLCipher (local)        |
| Transcrição   | whisper.cpp (`whisper-rs`) local ou API remota autorizada |
| Monorepo      | Turborepo + pnpm Workspace                  |

## Pré-requisitos

- Node.js >= 24
- pnpm 11 (versão fixada em `packageManager` no `package.json`)
- Rust estável com suporte à edition 2024
- CMake ≥ 3.5 (compila o whisper.cpp)
- [Dependências de sistema do Tauri](https://tauri.app/start/prerequisites/) para o seu SO
- Linux: um provedor de Secret Service ativo (GNOME Keyring ou KWallet)
- Opcional: [Ollama](https://ollama.com) em `127.0.0.1` para análise local

Roteiro completo de desenvolvimento e validação:
[specs/001-meeting-sessions/quickstart.md](specs/001-meeting-sessions/quickstart.md).

## Desenvolvimento

```sh
pnpm install

# App desktop em modo de desenvolvimento (Vite + janela Tauri)
pnpm --filter appsdesktop tauri dev

# Tarefas orquestradas pelo Turborepo
pnpm build
pnpm lint
pnpm check-types
```

Verificações do núcleo Rust:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo audit && cargo deny check
pnpm --filter appsdesktop check
pnpm --filter appsdesktop test
```

## Fluxo de trabalho

Novas funcionalidades seguem o fluxo do Spec Kit: constituição → especificação → plano →
tarefas → implementação. Decisões com consequências de longo prazo são registradas como ADR em
`docs/adr/`. Pull requests devem descrever requisito, impacto de design, testes, considerações de
segurança/privacidade e impacto por plataforma.
