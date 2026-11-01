# Checklist de publicação (modelo)

Modelo permanente. Para cada release, copie este arquivo para `docs/release/v<versão>/checklist.md` e
preencha a cópia. Este modelo nunca recebe resultados.

Formato: `specs/002-desktop-release-readiness/contracts/release-evidence.md`.

- **Versão / tag:** `v<versão>`
- **Commit do candidato:** `<sha completo>` (o mesmo de `v<versão>/candidate.md`)
- **Preenchido por:** · **Data:**

Estado: ✅ cumprido · ❌ não cumprido (link para a issue) · ➖ não se aplica (motivo obrigatório).
Todo item precisa de um link de evidência. Item sem evidência conta como ❌.

| # | Critério | Estado | Evidência |
|---|----------|--------|-----------|
| 1 | Gates de CI verdes no commit do candidato: fmt, clippy, testes, audit, deny, gitleaks, frontend | | link da execução |
| 2 | Versão igual em `tauri.conf.json`, `package.json`, `Cargo.toml` e na tag (teste `version_consistency` com `RELEASE_TAG`) | | link do job |
| 3 | F-1: teste `csp_policy` verde, e C1/C2 ✅ em todas as plataformas declaradas | | evidências por plataforma |
| 4 | F-4: assinatura/notarização ✅ por artefato (`candidate.md`); `SHA256SUMS.asc` verificado; U1–U4 ✅ | | `candidate.md`, evidências |
| 5 | F-6: evidência completa por plataforma declarada, sem ❌ crítico | | `evidence/` |
| 6 | Plano de medição aprovado antes dos resultados; cada resultado classificado | | `measurement-plan.md`, `measurements.md` |
| 7 | ADR-002 (S1), ADR-003 (medições) e ADR-004 (Windows/Linux) atualizados | | commits/PRs |
| 8 | Revisão de segurança atualizada com F-1…F-6 e nova aprovação explícita | | documento da revisão |
| 9 | Zero issues abertas com `release-blocker` | | consulta do GitHub com data e hora |
| 10 | Notas de release e READMEs declaram só o que foi validado; limitações por plataforma listadas | | `limitations.md`, notas |
| 11 | Plano de rollback revisado e ensaiado no canal de teste (SC-013) | | `rollback.md`, registro do ensaio |
| 12 | Documentação de release reproduzida por outra pessoa do projeto (SC-012) | | registro da reprodução |

**Itens ✅:** N/12

A decisão de publicar (`v<versão>/decision.md`) só é tomada com 12/12 itens em ✅ ou ➖ justificado.
