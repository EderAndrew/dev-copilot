# Reprodução da documentação de release v0.1.0 (SC-012)

> Modelo (preparação da Fase 10). Preenchido em T090. Item 12 do checklist.

Uma pessoa do projeto que **não** executou T055 nem T057 reproduz um release no canal de teste (fork, com
identidades descartáveis) seguindo **só** `docs/release/signing-and-distribution.md` e os documentos
para os quais ele aponta. Ela não pede ajuda. Cada vez que travar, anota a lacuna e segue a melhor leitura
que conseguir do texto.

- **Executor:** · **Data:** · **Fork:** <URL>
- **Commit da documentação usada:** `<sha>`
- **SO e ferramentas:** <SO, versões de `gh`, `gpg`, Node, `pnpm`>

## Etapas

| # | Etapa (seção do documento) | Concluída sem ajuda? | Tempo | Lacuna encontrada |
|---|----------------------------|----------------------|-------|-------------------|
| 1 | Pré-requisitos e identidades de teste | | | |
| 2 | Environment `release` e segredos | | | |
| 3 | Ajustar versão e chave pública de teste; criar a tag `rc` | | | |
| 4 | Aprovar o environment e acompanhar o workflow | | | |
| 5 | Baixar o rascunho, verificar `SHA256SUMS` e atestações | | | |
| 6 | Assinar `SHA256SUMS` localmente e anexar `.asc` | | | |
| 7 | Verificar um download como usuário (`verify-downloads.md`) | | | |
| 8 | Publicar no canal de teste | | | |

## Lacunas e correções

| Lacuna | Documento / seção | Correção (commit) |
|--------|-------------------|-------------------|

## Resultado

- Reproduziu empacotamento, assinatura e publicação só com a documentação: ✅/❌
- Se ❌: o que faltou e o que foi corrigido. Depois da correção, a etapa afetada é repetida.
