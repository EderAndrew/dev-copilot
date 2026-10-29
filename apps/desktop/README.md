# Dev Copilot — app desktop

Aplicação Tauri 2 + React + TypeScript (Vite) do Dev Copilot.

> **Status:** interface e comandos IPC da feature `001-meeting-sessions` implementados: sessões
> (criar, iniciar, pausar/retomar, parar, histórico, renomear, excluir), transcrição ao vivo, revisão de
> registros e plano, perguntas, exportação, provedores (locais e remotos com autorização) e
> configurações, em pt-BR e inglês. Áudio do sistema aparece indisponível até o spike S1
> ([ADR-002](../../docs/adr/ADR-002.md)).

## Responsabilidades

- `src/`: interface React. Apenas apresentação e estado de UI; regras de negócio e casos de
  uso ficam nos crates `domain` e `application`.
- `src-tauri/`: raiz de composição. Instancia os adaptadores de infraestrutura, injeta-os nos
  casos de uso e expõe comandos Tauri finos que validam a entrada e delegam para `application`.
- `src-tauri/capabilities/`: permissões do Tauri, restritas ao mínimo: a janela `main` tem apenas
  `core:default` (sem fs, shell, diálogo ou rede); o diálogo de exportação é aberto pelo núcleo.
- `src/ipc/`: tipos dos DTOs e eventos escritos à mão, verificados contra as fixtures compartilhadas em
  `specs/001-meeting-sessions/contracts/fixtures/` (teste de contrato nos dois lados).

Ver a [Constituição](../../.specify/memory/constitution.md) (Artigos II, IV e VII) e o
[ADR-001](../../docs/adr/ADR-001.md).

## Comandos

A partir da raiz do repositório:

```sh
pnpm --filter appsdesktop tauri dev     # desenvolvimento
pnpm --filter appsdesktop tauri build   # build empacotado
pnpm --filter appsdesktop check         # checagem de tipos TypeScript
pnpm --filter appsdesktop lint          # ESLint
pnpm --filter appsdesktop test          # Vitest (componentes, contratos IPC, i18n, acessibilidade)
```

## IDE recomendada

[VS Code](https://code.visualstudio.com/) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)
