# Registro de rollback (modelo)

Modelo permanente. A cada acionamento, copie para `docs/release/v<N>/rollback-<AAAA-MM-DD>.md` e preencha a
cópia. Procedimento: [rollback.md](rollback.md). Este modelo nunca recebe resultados.

- **Versão afetada (N):** `v<N>` · **Última boa (N−1):** `v<N−1>` ou "nenhuma" · **Correção (N+1):** `v<N+1>`
- **Gatilho:** defeito crítico | defeito de plataforma | incidente de segurança | comprometimento de chave |
  perda de chave
- **Issue:** <link> · **Advisory privado:** <link ou "não se aplica">
- **Plataformas afetadas:**
- **Papéis:** responsável pelo release: · guardião das chaves: · comunicação:

## Linha do tempo (UTC)

| Hora | Passo (`rollback.md`) | O que foi feito | Conferência |
|------|-----------------------|-----------------|-------------|
| T0 = | 0. Acionar | issue aberta, rótulos | — |
| | 1. Suspender versão / 3. Suspender plataforma | comandos executados | `latest.json` servido: versão / plataformas |
| | 2. Retirar artefatos | arquivos apagados; local da cópia guardada | download de um arquivo retirado → 404 |
| | 4. Aviso publicado | notas, issue fixada, README | links |
| | 5. N+1 publicada | tag, commit | apps em N e em N−1 receberam N+1 |
| | 6. Encerrado | aviso atualizado, issue fechada | — |

**Tempo de T0 até retirada + aviso:** <h:mm> (meta SC-013: ≤ 2 h)

## Artefatos retirados

| Arquivo | SHA-256 | Cópia guardada em |
|---------|---------|-------------------|

## Chaves e identidades (se houver)

| Chave / identidade | Ação (revogada, rotacionada, substituída) | Nova identificação pública | ADR-005 atualizado |
|--------------------|-------------------------------------------|----------------------------|--------------------|

## Dados locais

- N mudou o formato dos dados? sim/não · Voltar para N−1 manualmente é seguro? sim/não
- Algum relato de perda de dados? <não | link>

## Lições

- O que funcionou:
- O que atrasou:
- Mudanças em `rollback.md`: <commit ou "nenhuma">
