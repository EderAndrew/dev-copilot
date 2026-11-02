# Contract: formatos de evidência, plano de medição e decisão de release

Documentos em `docs/release/`. São Markdown versionado; anexos pequenos (PNG ≤ 500 KB) ficam junto, e os
grandes vão como anexo do release em rascunho, referenciados por link + SHA-256.

## `docs/release/v0.1.0/candidate.md`

```markdown
# Build candidato v0.1.0-rc.N
- Tag: v0.1.0-rc.N · Commit: <sha completo> · Workflow: <URL da execução>
- Data do build: <RFC 3339>
| Artefato | SHA-256 | Assinatura | Notarização / timestamp | Atestação |
|----------|---------|------------|-------------------------|-----------|
```

Toda evidência cita o commit do candidato. Evidência produzida com outro candidato é revalidada, ou
justificada como não afetada pela diferença entre os commits.

## `docs/release/v0.1.0/evidence/<plataforma>.md`

Plataformas: `macos-arm64`, `macos-x64` (se declarado), `windows-10`, `windows-11`, `linux-wayland`,
`linux-x11`.

```markdown
# Evidências — <plataforma> — candidato <commit curto>
## Ambiente
| SO e versão | Hardware | Leitor(es) de tela | Pilha de áudio | Perfil limpo? | Executor | Data |
## Instalação e verificação (US1)
| # | Caso | Resultado | Evidência |
| I1 | Verificação de assinatura e checksum | ✅/❌ | anexo |
| I2 | Instalação sem contornar aviso de segurança | | |
| I3 | Artefato adulterado rejeitado | | |
| I4 | Primeiro pedido de permissão de microfone no build assinado | | |
## Política de conteúdo (US2)
| C1 | Percurso de todas as telas com zero `csp_violation` no log | | trecho do log |
| C2 | Comparação visual com a referência | | capturas |
## Matriz da 001 (casos 1–13, 3b, 3c, 11b) — mesma numeração de specs/001-meeting-sessions/acceptance-matrix.md
## Atualização (US8)
| U1 | N → N+1 legítima, com confirmação | | |
| U2 | Pacote adulterado / outra chave / versão menor rejeitados | | |
| U3 | Verificação desligada: zero requisições (proxy) | | |
| U4 | Atualização bloqueada com sessão capturando/pausada | | |
## Limitações observadas
## Problemas abertos (links)
```

Resultado: ✅ passou · ❌ falhou (link para a issue, com severidade) · ➖ não se aplica (motivo obrigatório).

## `docs/release/v0.1.0/measurement-plan.md` (congelado antes da primeira medição)

```markdown
# Plano de medição v0.1.0
- Aprovado por: · Data de aprovação: <anterior a qualquer resultado>
## Hardware
| SO | Perfil (referência/mínimo) | CPU | RAM | GPU | Observação |
## Metas
| Métrica | Meta | Hardware | Amostra mínima | Método |
| Parcial (1º) após início da fala, p95 | ≤ 5 s | ref. | 100 trechos | DEVCOPILOT_METRICS + entrada virtual |
| Final após fim da fala, p95 | ≤ 15 s | ref. | 100 trechos | idem |
| Latência de controles (iniciar/pausar/parar), p95 | ≤ 500 ms | ref. e mín. | 20 por controle | cronômetro de eventos |
| Indicador desligado após parar, p95 | ≤ 1 s | ref. e mín. | 20 | idem |
| RSS após 2 h vs. 10 min | ≤ +10 % | ref. | 1 sessão de 2 h | perf-sampler |
| CPU média sem captura | ≤ 2 % | ref. | 30 min | perf-sampler |
| CPU média capturando (modelo padrão) | ≤ 50 % da capacidade total | ref. | 2 h | perf-sampler |
| Análise: recall / grounding / injeção / Q&A | SC-008 / SC-010 / SC-011 da 001 | ref. | fixtures da 001 | harnesses da 001 + Ollama <modelo@digest> |
## Condições
energia na tomada · sem outras cargas · 10 min de aquecimento · modelo de transcrição <nome> · versão do Ollama/modelo
## Histórico de alterações (nunca apagar linhas)
| Data | Métrica | Meta anterior | Meta nova | Justificativa | Aprovado por |
```

No hardware mínimo, as metas de latência de transcrição são **registradas, não exigidas**. Resultado fora da
meta vira limitação, com o modelo recomendado (spec SC-007).

## `docs/release/checklist.md` (modelo) → `docs/release/v0.1.0/checklist.md` (preenchido)

Cada item tem estado (✅/❌/➖ com motivo) e um link de evidência.

1. Gates de CI verdes no commit do candidato (fmt, clippy, testes, audit, deny, gitleaks, frontend).
2. Versão igual em `tauri.conf.json`, `package.json`, `Cargo.toml` e na tag.
3. F-1: teste `csp_policy` verde e C1/C2 ✅ em todas as plataformas declaradas.
4. F-4: assinatura/notarização ✅ por artefato (`candidate.md`); `SHA256SUMS.asc` verificado; U1–U4 ✅.
5. F-6: evidência completa por plataforma declarada, sem ❌ crítico.
6. Plano de medição aprovado antes dos resultados; cada resultado classificado.
7. ADR-002 (S1), ADR-003 (medições) e ADR-004 (Windows/Linux) atualizados.
8. Revisão de segurança atualizada com F-1…F-6 e nova aprovação explícita.
9. Zero issues abertas com `release-blocker`.
10. Notas de release e READMEs declaram só o que foi validado; limitações por plataforma listadas.
11. Plano de rollback revisado e ensaiado no canal de teste (SC-013).
12. Reprodução da documentação de release por outra pessoa do projeto (SC-012).

## `docs/release/v0.1.0/decision.md`

```markdown
# Decisão de publicação v0.1.0
- Decisão: publicar | não publicar
- Responsável: · Data:
- Versão / tag / commit:
- Checksums aprovados: (cópia do SHA256SUMS)
- Checklist: link · Itens ✅: N/12
- Disposição dos achados de segurança: F-1 … F-6
- Plataformas declaradas e limitações:
- Invalidação: esta aprovação perde efeito se qualquer checksum mudar ou se uma issue `release-blocker` for aberta antes da publicação.
```
