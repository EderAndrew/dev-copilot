# Contract: Formatos de exportação (FR-041)

O usuário escolhe destino, formatos (`markdown`, `json`) e conteúdo (`plan`, `transcript`). Arquivos exportados
**não são cifrados**; o aplicativo avisa antes de gravar. Credenciais, configurações de provedor e caminhos
locais nunca são incluídos.

Nome sugerido: `<titulo-normalizado>-<AAAA-MM-DD>.{md,json}`.

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
- `origin.providerId` **não** é exportado (pode revelar configuração); apenas `type` e `analysisRunId`.
- Mudanças incompatíveis exigem `version` nova.

## Markdown

```markdown
# <título>
<data> · <duração ativa> · idioma: <idioma> · fontes: <fontes>

## Plano
### Decisões
- [confirmado] [observado] <texto> — evidência: 00:12:30, 00:14:02
  - Alternativas: …
### Requisitos
### Tarefas
- [proposto] [sugerido] <texto>
  - Critérios de aceitação: …
### Riscos
### Premissas
### Perguntas em aberto

## Perguntas e respostas
**P:** … **R:** … (evidência: …)

## Transcrição
[00:00:00] (microfone) …
[00:04:12] — pausa de 1 min —
```

Rótulos seguem o idioma da interface; o conteúdo segue o idioma da sessão (FR-053b).
