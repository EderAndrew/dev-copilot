# Contract: Formatos de exportação (FR-041)

O usuário escolhe destino, formatos (`markdown`, `json`) e conteúdo (`plan`, `transcript`). Arquivos exportados
**não são cifrados**; o aplicativo avisa antes de gravar. Credenciais, configurações de provedor e caminhos
locais nunca são incluídos.

Nome sugerido: `<titulo-normalizado>-<AAAA-MM-DD>.{md,json}` — título em minúsculas ASCII (acentos comuns
removidos, demais caracteres viram `-`, até 60 caracteres; `session` se nada restar) e a data (UTC) do início
da captura, ou da criação se a sessão nunca capturou.

O núcleo abre um diálogo de salvar por formato e só grava depois de todos escolhidos; cancelar qualquer um
não grava nada. O conteúdo é o persistido no momento da gravação.

Exemplos verificados por teste (`crates/application/tests/export_service.rs`):
[`examples/session-export.v1.json`](./examples/session-export.v1.json),
[`examples/session-export.pt-BR.md`](./examples/session-export.pt-BR.md) e
[`examples/session-export.en.md`](./examples/session-export.en.md).

## JSON — `devcopilot.session-export` v1

```json
{
  "format": "devcopilot.session-export",
  "version": 1,
  "exportedAt": "2026-10-27T18:00:00Z",
  "session": {
    "id": "0192f3a1-...",
    "title": "Revisão da API de exportação",
    "language": "pt-BR",
    "state": "ended",
    "startedAt": "2026-10-27T17:00:00Z",
    "endedAt": "2026-10-27T17:45:00Z",
    "activeDurationSecs": 2580,
    "sources": ["microphone"]
  },
  "transcript": [
    { "id": "…", "seq": 1, "type": "final", "source": "microphone", "startMs": 0, "endMs": 4200, "text": "…" },
    { "id": "…", "seq": 2, "type": "gap", "reason": "paused", "startMs": 4200, "durationMs": 60000 }
  ],
  "records": [
    {
      "id": "…",
      "kind": "requirement",
      "text": "O export precisa suportar CSV",
      "nature": "observed",
      "status": "confirmed",
      "rationale": "…",
      "origin": { "type": "ai", "analysisRunId": "…" },
      "evidenceSegmentIds": ["…"],
      "acceptanceCriteria": [],
      "alternatives": [],
      "flags": { "conflictsWith": null, "duplicateOf": null },
      "createdAt": "…",
      "updatedAt": "…",
      "history": [
        { "field": "created", "actor": "ai", "oldValue": null, "newValue": "proposed", "at": "…" },
        { "field": "status", "actor": "user", "oldValue": "proposed", "newValue": "confirmed", "at": "…" }
      ]
    }
  ],
  "questions": [
    { "id": "…", "question": "…", "answer": "…", "certainty": "supported", "evidence": [{ "type": "segment", "id": "…" }], "askedAt": "…" }
  ]
}
```

- `transcript` presente somente se `content` inclui `transcript`; `records`/`questions` somente se inclui `plan`.
- `origin.providerId` **não** é exportado (pode revelar configuração); apenas `type` e `analysisRunId`
  (`{ "type": "user" }` para registros do usuário). Perguntas também não levam `providerId`.
- `questions` traz apenas perguntas respondidas (`pending`/`failed` não têm resposta a exportar).
- Instantes em RFC 3339 UTC (milissegundos só quando não nulos); `startedAt`/`endedAt` e `source` podem ser
  `null`. A ordem das chaves não é significativa.
- Mudanças incompatíveis exigem `version` nova.

## Markdown

```markdown
# <título>

<data> · duração ativa: <HH:MM:SS> · idioma: <idioma> · fontes: <fontes>

## Plano

### Tarefas

- [<status>] [<natureza>] [<origem: IA | usuário>] <texto> — evidência: 00:12:30, 00:14:02
  - Justificativa: …
  - Critérios de aceitação: …; …

### Requisitos
### Decisões
  - Alternativas: …
### Premissas
### Riscos
### Perguntas em aberto

## Perguntas e respostas

**P:** …
**R:** … (certeza: apoiada; evidência: 00:00:12, “<registro citado>”)

## Transcrição

\[00:00:00\] (microfone) …
\[00:04:12\] — pausa (1 min) —
```

- Seções e itens seguem a ordem do plano na interface (tipo; depois `confirmado`, `pendente`, `proposto`,
  `rejeitado`; depois criação). Seções vazias são omitidas; sem itens, `Nenhum item.`.
- Cada item mostra status, natureza e proveniência (US6.1). Evidências aparecem como o instante do trecho.
- Texto da sessão é escapado (`\`, `` ` ``, `*`, `_`, `[`, `]`, `<`, `>`, `#`, `|`, `!`, `~`) e quebras de
  linha viram espaço, para que conteúdo não crie estrutura, links ou HTML.

Rótulos seguem o idioma da interface; o conteúdo segue o idioma da sessão (FR-053b).
