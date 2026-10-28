# Dev Copilot

Assistente de engenharia desktop multiplataforma (macOS, Windows e Linux). O objetivo é que o
usuário inicie explicitamente a captura autorizada de áudio do sistema e/ou microfone, e o
aplicativo transcreva conversas técnicas, extraia requisitos, riscos e decisões, mantenha um
plano de desenvolvimento incremental e responda perguntas contextuais.

O Dev Copilot **não** se integra com plataformas de reunião, **não** exige backend em nuvem e
**não** executa código ou comandos de forma autônoma.

> **Status: estágio inicial.** A estrutura do monorepo e a arquitetura estão definidas, mas
> nenhuma funcionalidade de captura, transcrição, análise ou persistência foi implementada
> ainda. Os crates Rust são esqueletos e o app desktop é o template padrão do Tauri.

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
  persistence/         Repositórios SQLite
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
| Persistência  | SQLite (local)                              |
| Monorepo      | Turborepo + pnpm Workspace                  |

## Pré-requisitos

- Node.js >= 24
- pnpm 11 (versão fixada em `packageManager` no `package.json`)
- Rust estável com suporte à edition 2024
- [Dependências de sistema do Tauri](https://tauri.app/start/prerequisites/) para o seu SO

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
```

## Fluxo de trabalho

Novas funcionalidades seguem o fluxo do Spec Kit: constituição → especificação → plano →
tarefas → implementação. Decisões com consequências de longo prazo são registradas como ADR em
`docs/adr/`. Pull requests devem descrever requisito, impacto de design, testes, considerações de
segurança/privacidade e impacto por plataforma.
