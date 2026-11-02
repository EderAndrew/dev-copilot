# Release do Dev Copilot

Processo de release definido pela Feature 002 (`specs/002-desktop-release-readiness/`). Os documentos desta
pasta valem para todos os releases. Cada release tem uma pasta própria com as evidências daquela versão.

## Documentos permanentes

| Documento | Conteúdo | Status |
|-----------|----------|--------|
| [checklist.md](checklist.md) | Os 12 critérios de publicação, cada um apontando para a evidência | modelo pronto (T011) |
| [signing-and-distribution.md](signing-and-distribution.md) | Identidades, chaves, pipeline, assinatura local do `SHA256SUMS`, renovação e rotação | pronto (T053); valores públicos pendentes (T005–T008) |
| [verify-downloads.md](verify-downloads.md) | Como o usuário verifica os downloads em cada SO | pronto (T054); impressão digital pendente (T007) |
| [rollback.md](rollback.md) | Gatilhos, passos, comunicação e chaves | a criar (T088) |
| `release-key.asc` | Chave pública OpenPGP de release | a criar (T007) |

Formatos: `specs/002-desktop-release-readiness/contracts/release-evidence.md` e `release-artifacts.md`.

## Pasta por release

`v<versão>/` contém:

- `candidate.md`: o build candidato.
- `measurement-plan.md`: o plano de medição, congelado antes das medições.
- `measurements.md`: os resultados das medições.
- `evidence/<plataforma>.md`: as evidências de cada plataforma.
- `limitations.md`: as limitações conhecidas.
- `checklist.md`: o checklist preenchido.
- `decision.md`: a decisão de publicação.

Capturas de tela comprimidas (no máximo 500 KB cada) ficam junto. Vídeos e CSVs grandes vão como anexo do
release em rascunho, referenciados por link e SHA-256.

## Rótulos de issues

Crie estes rótulos no GitHub antes da primeira rodada de validação:

| Rótulo | Uso |
|--------|-----|
| `release-0.1.0` | Problema encontrado na validação do 0.1.0 |
| `severity:critical` | Viola segurança, privacidade, consentimento ou integridade de dados, ou impede o fluxo principal numa plataforma declarada. **Bloqueia** sempre |
| `severity:high` | Degrada um fluxo, mas tem contorno. Bloqueia se o responsável decidir |
| `severity:medium` | Cosmético relevante ou raro |
| `severity:low` | Cosmético |
| `platform:macos`, `platform:windows`, `platform:linux-wayland`, `platform:linux-x11` | Plataforma afetada |
| `release-blocker` | Toda issue crítica, e as altas que o responsável marcar. O checklist não fecha com nenhuma aberta |

Para criar os rótulos com a CLI do GitHub:

```sh
gh label create release-0.1.0 --color 0E8A16
for s in critical high medium low; do gh label create "severity:$s" --color B60205; done
for p in macos windows linux-wayland linux-x11; do gh label create "platform:$p" --color 1D76DB; done
gh label create release-blocker --color 000000 --description "Bloqueia a publicação"
```

## Regras

- Evidências usam só áudio e texto de teste não confidenciais (Constitution Art. IV.16).
- Resultado de CI nunca é evidência de compatibilidade com uma plataforma (spec FR-014).
- Nenhum controle de segurança é enfraquecido para passar numa verificação: sem `'unsafe-inline'`, sem
  artefato não assinado, sem instrução para contornar Gatekeeper ou SmartScreen.
