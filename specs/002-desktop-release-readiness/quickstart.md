# Quickstart — 002 Desktop Release Readiness

Este é o roteiro para validar a feature de ponta a ponta. As seções 1 a 3 rodam numa máquina de
desenvolvimento. As seções 4 a 7 exigem hardware real e o build candidato **assinado**. Use só áudio e textos
de teste não confidenciais (Art. IV.16).

## 1. Checagens automáticas (desenvolvimento e CI)

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace                       # inclui csp_policy, update_service e redação do CSV de métricas
pnpm --filter appsdesktop check              # inclui a regra de lint que proíbe prop `style`
pnpm --filter appsdesktop build && pnpm --filter appsdesktop test:dist   # dist sem <style>/style=
```

Esperado: tudo verde; `csp_policy` falha se alguém reintroduzir `'unsafe-inline'`.

## 2. CSP estrita no build de produção local (antes de qualquer assinatura)

1. Use `pnpm --filter appsdesktop tauri build` (build de release, com a CSP de produção) e abra o app gerado.
   A `devCsp` só vale em `tauri dev`, que não serve para esta validação.
2. Percorra Sessões → nova sessão → consentimento → captura → pausa → parada → Histórico → Detalhe →
   Exportação (cancelar) → Exclusão (cancelar) → Provedores → Configurações.
3. Confira no log (`devcopilot.log`, no diretório de logs do app; caminhos em `docs/local-data.md`):
   esperado **zero** linhas `csp_violation`.
4. Faça isso nos três SOs. Uma violação é um achado do R1: corrija ou siga o caminho de exceção.

## 3. Ensaio do pipeline de release num canal de teste

1. Num fork ou repositório de teste com identidades de teste e versão `0.0.1`, crie a tag `v0.0.1-rc.1`
   (o gate de versão só aceita `v<versão>` e `-rc.N`; passos em `docs/release/signing-and-distribution.md`). O workflow
   `release.yml` gera um rascunho com os artefatos, `SHA256SUMS`, `latest.json` e atestações.
2. Confirme que, sem aprovação do environment `release`, nenhum job de assinatura roda.
3. Remova de propósito um segredo de assinatura e verifique que o job falha e o rascunho fica sem artefatos
   (FR-005).
4. Assine o `SHA256SUMS` localmente (`gpg --detach-sign --armor SHA256SUMS`) e anexe o `.asc`.

## 4. Instalação limpa e verificação (US1), por plataforma

Use um perfil de usuário **novo**, que nunca tenha rodado um build de desenvolvimento (R2, item do Keychain).

1. Siga `docs/release/verify-downloads.md` literalmente: assinatura do `SHA256SUMS`, hash do artefato.
2. Altere um byte de uma cópia do artefato e repita: a verificação **precisa falhar**.
3. Instale. macOS: nenhum aviso de "abrir mesmo assim". Windows: UAC com editor verificado. Linux: AppImage
   executável ou `.deb` instalado com `apt`.
4. Crie e inicie uma sessão: o pedido de permissão de microfone aparece com o nome do app e a captura
   funciona.

Registre em `docs/release/v0.1.0/evidence/<plataforma>.md`, seções I e C.

## 5. Matriz da 001 no build assinado (US3, US4)

Execute todos os casos de `specs/001-meeting-sessions/acceptance-matrix.md` (1–13, 3b, 3c, 11b e as medições
cronometradas) e registre-os no arquivo de evidência da plataforma. Casos com atenção especial:

- **3c**: suspender e retomar o SO e bloquear e desbloquear a sessão, com a captura ativa.
- **4**: encerrar à força (Activity Monitor, Gerenciador de Tarefas, `kill -9`) e reabrir.
- **11/11b**: VoiceOver (macOS), Narrator **e** NVDA (Windows), Orca (Linux), sem mouse.
- **12**: o áudio do sistema aparece indisponível com motivo em **todas** as plataformas (Q1).
- **13** (Linux): parar o Secret Service e abrir o app.

## 6. Atualização (US8), num canal de teste

1. Instale a versão N de teste. Na primeira execução aparece o aviso de verificação de atualizações, com os
   destinos. Antes de dispensá-lo, o proxy local (mitmproxy/Charles) registra **zero** requisições de
   atualização.
2. Dispense o aviso e publique N+1 legítima no canal. "Verificar agora" mostra a versão e as notas; instale
   com confirmação; o app reinicia na N+1 com os dados intactos.
3. Publique no canal: um pacote adulterado, um assinado com outra chave e uma versão menor. Os três são
   rejeitados (`failed`/sem oferta), e a versão instalada não muda.
4. Com uma sessão capturando ou pausada, tente instalar: estado `blocked_by_session`.
5. Desligue a verificação nas Configurações e reinicie: zero requisições no proxy.

## 7. Medições (US5)

1. Aprove e versione `docs/release/v0.1.0/measurement-plan.md` **antes** da primeira medição.
2. Latência: `DEVCOPILOT_METRICS=1` + gravações tocadas no dispositivo de entrada virtual; ≥ 100 trechos por
   combinação; mais uma execução ao vivo.
3. CPU e memória: `cargo run -p perf-sampler --release -- sample --process "Dev Copilot" --interval 5 --out <csv>`
   (ou `--pid <pid>` se o nome do processo instalado for outro ou não for único), depois
   `perf-sampler summarize --samples <csv> --warmup-min 10`, durante uma sessão de 2 h no app instalado.
   Amostre também o processo da WebView (`docs/release/v0.1.0/runbook-us5.md` §5).
4. Análise: `DEVCOPILOT_EVAL_PROVIDER=ollama DEVCOPILOT_EVAL_MODEL=<modelo> cargo test -p devcopilot-intelligence
   --release --test evaluation --test qa_evaluation --test prompt_injection -- --nocapture`, com o modelo e o
   digest do plano. Sem `--nocapture` as métricas por transcrição não aparecem.
5. Registre em `measurements.md`, com cada resultado classificado como ok, limitação ou bloqueador, e atualize
   o ADR-003.

## 8. Decisão

Preencha `docs/release/v0.1.0/checklist.md`. Com 12/12 itens resolvidos e zero `release-blocker`, registre
`decision.md` e só então publique o rascunho do GitHub. Se um artefato for reconstruído, a aprovação caduca.
