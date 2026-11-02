# Ensaio de rollback v0.1.0 (SC-013)

> Modelo (preparação da Fase 10). Preenchido em T089. Procedimento: `docs/release/rollback.md`.
> Item 11 do checklist.

O ensaio roda num **fork de teste**, com identidades descartáveis, nunca no repositório principal. Ele
reaproveita a configuração do ensaio do pipeline (T055, `docs/release/v0.1.0/pipeline-rehearsal.md`).

## Preparação

- Fork: `<usuário>/dev-copilot` · chave minisign de teste com key ID `<id>` · chave OpenPGP descartável
  `<impressão digital>`
- No fork, `plugins.updater.endpoints` aponta para
  `https://github.com/<usuário>/dev-copilot/releases/latest/download/latest.json`, e a constante `ENDPOINT` de
  `apps/desktop/src-tauri/tests/updater_config.rs` recebe o mesmo valor (sem isso, o gate de testes do
  workflow recusa o build). Essas duas mudanças ficam só no fork e nunca voltam para o repositório principal.
- Três versões de teste, todas assinadas com a mesma chave de teste:
  - **N−1** = `0.0.1` (publicada como *latest*)
  - **N** = `0.0.2` (a versão "defeituosa")
  - **N+1** = `0.0.3` (a correção)
- Duas máquinas (ou perfis de usuário) com a 0.0.1 instalada pelo instalador assinado:
  - **A**: vai atualizar para N antes do acionamento (simula quem já instalou a versão ruim).
  - **B**: fica em N−1, mas verifica depois da publicação de N e fica com N em `available` sem confirmar
    (simula quem viu a atualização antes da suspensão).
- Em A e em B, crie uma sessão de teste com áudio e texto não confidenciais, pare e anote título, número de
  trechos e um item extraído.

## Roteiro

| # | Ação | Resultado esperado | Resultado | Hora (UTC) |
|---|------|--------------------|-----------|------------|
| 1 | Publicar 0.0.2 como *latest*; A atualiza; B verifica e não confirma | A em 0.0.2; B mostra 0.0.2 disponível | | |
| 2 | **T0**: abrir a issue de teste com `severity:critical` + `release-blocker` | — | | |
| 3 | Passo 1: 0.0.2 vira pré-release, 0.0.1 volta a *latest* | `curl …/latest.json \| jq .version` → `0.0.1` | | |
| 4 | Passo 2: apagar os pacotes e instaladores da 0.0.2 | download direto de um pacote → 404 | | |
| 5 | Passo 4: aviso nas notas da 0.0.2 + issue fixada + bloco no README do fork | os três publicados, sem pedir para desativar proteção do SO | | |
| 6 | **Fim do relógio**: passos 3–5 conferidos | ≤ 2 h desde T0 | | |
| 7 | Em B, confirmar a atualização vista antes da suspensão | download falha, estado `failed`, B continua em 0.0.1 | | |
| 8 | Em B, reiniciar o app ou "Verificar agora" | "Você já está na versão mais recente." | | |
| 9 | Em A, abrir o app | continua em 0.0.2; a sessão de teste está intacta | | |
| 10 | Passo 5: publicar 0.0.3 como *latest* | A e B veem 0.0.3 e instalam após confirmação | | |
| 11 | Em A e em B, abrir a sessão de teste | título, trechos e item extraído iguais aos anotados | | |
| 12 | Passo 6: atualizar o aviso, fechar a issue de teste | — | | |

## Resultado

- **Tempo de T0 até retirada + aviso:** <h:mm> · Meta: ≤ 2 h · ✅/❌
- **Dados locais intactos em A e B:** ✅/❌
- **Executor:** · **Data:**
- **Problemas encontrados e mudanças feitas em `rollback.md`:**

Variação opcional, se houver tempo: suspender só uma plataforma (passo 3 de `rollback.md`) e confirmar que
ela some do `latest.json` enquanto as outras continuam recebendo 0.0.2.
