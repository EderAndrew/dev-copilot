# Dev Copilot — app desktop

Aplicação Tauri 2 + React + TypeScript (Vite) do Dev Copilot.

> **Status:** ainda é o template padrão do Tauri (comando `greet` de exemplo). Nenhuma
> funcionalidade do produto foi implementada aqui.

## Responsabilidades

- `src/`: interface React. Apenas apresentação e estado de UI; regras de negócio e casos de
  uso ficam nos crates `domain` e `application`.
- `src-tauri/`: raiz de composição. Instancia os adaptadores de infraestrutura, injeta-os nos
  casos de uso e expõe comandos Tauri finos que validam a entrada e delegam para `application`.
- `src-tauri/capabilities/`: permissões do Tauri, que devem ser restritas ao mínimo necessário
  por janela, comando e plugin.

Ver a [Constituição](../../.specify/memory/constitution.md) (Artigos II, IV e VII) e o
[ADR-001](../../docs/adr/ADR-001.md).

## Comandos

A partir da raiz do repositório:

```sh
pnpm --filter appsdesktop tauri dev     # desenvolvimento
pnpm --filter appsdesktop tauri build   # build empacotado
pnpm --filter appsdesktop check         # checagem de tipos TypeScript
```

## IDE recomendada

[VS Code](https://code.visualstudio.com/) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)
