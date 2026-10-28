//! Session export (FR-041, contracts/export-format.md).
//!
//! ```text
//! export_session ─▶ file_names ─▶ save dialog (per format) ─▶ render ─▶ write ─▶ record
//! ```
//!
//! - Documents are rendered from what is persisted when the export runs; proposals of an
//!   analysis still in flight are not included (spec edge case).
//! - The structured format is `devcopilot.session-export` v1. Provider ids, provider settings,
//!   credentials and local paths are never part of a document.
//! - Markdown labels follow the interface language; the content stays in the session language
//!   (FR-053b). Session text is escaped so it cannot add Markdown structure, links or HTML.
//! - Writing the files is the caller's job (only to the paths the user chose); this service
//!   then records the export with its formats and content, never the path.

use std::collections::{BTreeSet, HashMap};
use std::fmt::Write as _;
use std::sync::Arc;

use devcopilot_domain::Redacted;
use devcopilot_domain::audio_source::AudioSourceKind;
use devcopilot_domain::export::{ExportContent, ExportFormat, ExportRecord};
use devcopilot_domain::ids::{ExportId, RecordId, SegmentId, SessionId};
use devcopilot_domain::language::UiLanguage;
use devcopilot_domain::question::{Certainty, ContextQuestion, EvidenceRef, QuestionStatus};
use devcopilot_domain::record::{Nature, Origin, RecordKind, RecordStatus, TechnicalRecord};
use devcopilot_domain::record_change::RecordChange;
use devcopilot_domain::session::Session;
use devcopilot_domain::time::{Duration, Timestamp};
use devcopilot_domain::transcript::{GapReason, SegmentKind, TranscriptSegment};
use serde_json::{Value, json};

use crate::error::ApplicationError;
use crate::plan::DevelopmentPlan;
use crate::ports::{
    Clock, ExportRepository, QuestionRepository, RecordRepository, SegmentRepository,
    SessionRepository,
};
use crate::settings_service::SettingsService;

/// `format` of the structured export.
pub const JSON_FORMAT: &str = "devcopilot.session-export";
/// `version` of the structured export; incompatible changes need a new one.
pub const JSON_VERSION: u32 = 1;
/// Longest title part of a suggested file name, in characters.
const MAX_SLUG_CHARS: usize = 60;
/// Longest quote of a cited record in the Markdown answers, in characters.
const QUOTE_CHARS: usize = 80;

/// Collaborators of [`ExportService`].
pub struct ExportDeps {
    /// Session storage.
    pub sessions: Arc<dyn SessionRepository>,
    /// Transcript storage.
    pub segments: Arc<dyn SegmentRepository>,
    /// Record storage.
    pub records: Arc<dyn RecordRepository>,
    /// Question storage.
    pub questions: Arc<dyn QuestionRepository>,
    /// Export events.
    pub exports: Arc<dyn ExportRepository>,
    /// Interface language of the Markdown labels.
    pub settings: Arc<SettingsService>,
    /// Wall clock.
    pub clock: Arc<dyn Clock>,
}

/// What to export.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportRequest {
    /// Exported session.
    pub session_id: SessionId,
    /// Files to produce, one per format.
    pub formats: BTreeSet<ExportFormat>,
    /// Parts of the session to include.
    pub content: BTreeSet<ExportContent>,
}

/// One rendered file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportDocument {
    /// Format.
    pub format: ExportFormat,
    /// Suggested file name, without directory.
    pub file_name: String,
    /// File contents (session content, never logged).
    pub contents: Redacted<String>,
}

/// Export use cases.
pub struct ExportService {
    deps: ExportDeps,
}

/// Everything persisted for a session at export time.
struct Snapshot {
    session: Session,
    transcript: Vec<TranscriptSegment>,
    records: Vec<(TechnicalRecord, Vec<RecordChange>)>,
    questions: Vec<ContextQuestion>,
}

impl ExportService {
    /// Creates the service.
    #[must_use]
    pub const fn new(deps: ExportDeps) -> Self {
        Self { deps }
    }

    /// Suggested file name of each format, `<normalized-title>-<YYYY-MM-DD>.<ext>`, in format
    /// order.
    ///
    /// # Errors
    ///
    /// [`ApplicationError::NotFound`]; [`ApplicationError::Storage`].
    pub fn file_names(
        &self,
        session: SessionId,
        formats: &BTreeSet<ExportFormat>,
    ) -> Result<Vec<(ExportFormat, String)>, ApplicationError> {
        let session = self.deps.sessions.get(session)?;
        Ok(formats
            .iter()
            .map(|format| (*format, file_name(&session, *format)))
            .collect())
    }

    /// Renders one document per requested format from the content persisted now.
    ///
    /// # Errors
    ///
    /// [`ApplicationError::InvalidInput`] (`formats`, `content`) when either is empty;
    /// [`ApplicationError::NotFound`]; [`ApplicationError::Storage`].
    pub fn render(&self, request: &ExportRequest) -> Result<Vec<ExportDocument>, ApplicationError> {
        validate(request)?;
        let snapshot = self.snapshot(request)?;
        let labels = Labels::for_language(self.deps.settings.get()?.ui_language);
        let exported_at = self.deps.clock.now();
        Ok(request
            .formats
            .iter()
            .map(|format| {
                let contents = match format {
                    ExportFormat::Markdown => markdown(&snapshot, &request.content, labels),
                    ExportFormat::Json => structured(&snapshot, &request.content, exported_at),
                };
                ExportDocument {
                    format: *format,
                    file_name: file_name(&snapshot.session, *format),
                    contents: Redacted::new(contents),
                }
            })
            .collect())
    }

    /// Records that the files of `request` were written (data-model.md "ExportRecord").
    ///
    /// # Errors
    ///
    /// [`ApplicationError::InvalidInput`]; [`ApplicationError::NotFound`];
    /// [`ApplicationError::Storage`].
    pub fn record(&self, request: &ExportRequest) -> Result<ExportRecord, ApplicationError> {
        let export = ExportRecord::new(
            ExportId::new(),
            request.session_id,
            request.formats.clone(),
            request.content.clone(),
            self.deps.clock.now(),
        )?;
        self.deps.exports.insert(&export)?;
        tracing::info!(
            session = %request.session_id,
            export = %export.id(),
            formats = ?request.formats.iter().map(|f| f.code()).collect::<Vec<_>>(),
            content = ?request.content.iter().map(|c| c.code()).collect::<Vec<_>>(),
            "session exported"
        );
        Ok(export)
    }

    /// Exports of `session`, oldest first.
    ///
    /// # Errors
    ///
    /// [`ApplicationError::Storage`].
    pub fn history(&self, session: SessionId) -> Result<Vec<ExportRecord>, ApplicationError> {
        Ok(self.deps.exports.list_by_session(session)?)
    }

    fn snapshot(&self, request: &ExportRequest) -> Result<Snapshot, ApplicationError> {
        let deps = &self.deps;
        let id = request.session_id;
        let session = deps.sessions.get(id)?;
        // Segments are always read: plan items cite them by timestamp.
        let transcript = deps.segments.list_by_session(id)?;
        let (records, questions) = if request.content.contains(&ExportContent::Plan) {
            let records = deps
                .records
                .list_by_session(id)?
                .into_iter()
                .map(|record| {
                    let history = deps.records.history(record.id())?;
                    Ok((record, history))
                })
                .collect::<Result<Vec<_>, ApplicationError>>()?;
            let questions = deps
                .questions
                .list_by_session(id)?
                .into_iter()
                .filter(|question| question.status() == QuestionStatus::Answered)
                .collect();
            (records, questions)
        } else {
            (Vec::new(), Vec::new())
        };
        Ok(Snapshot {
            session,
            transcript,
            records,
            questions,
        })
    }
}

fn validate(request: &ExportRequest) -> Result<(), ApplicationError> {
    if request.formats.is_empty() {
        return Err(ApplicationError::InvalidInput {
            field: "formats",
            problem: "empty",
        });
    }
    if request.content.is_empty() {
        return Err(ApplicationError::InvalidInput {
            field: "content",
            problem: "empty",
        });
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// File names and time formats
// ---------------------------------------------------------------------------------------------

/// `<normalized-title>-<YYYY-MM-DD>.<ext>`, dated by the start of capture (or creation).
fn file_name(session: &Session, format: ExportFormat) -> String {
    let date = date(session.started_at().unwrap_or_else(|| session.created_at()));
    format!(
        "{}-{date}.{}",
        slug(session.title().as_str()),
        format.extension()
    )
}

/// Lowercase ASCII letters and digits separated by single dashes; common Latin accents are
/// folded, anything else becomes a separator.
fn slug(title: &str) -> String {
    let mut slug = String::new();
    for c in title.chars().flat_map(char::to_lowercase) {
        let folded = match c {
            'á' | 'à' | 'â' | 'ã' | 'ä' | 'å' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            'ñ' => 'n',
            c if c.is_ascii_alphanumeric() => c,
            _ => '-',
        };
        if folded == '-' && (slug.is_empty() || slug.ends_with('-')) {
            continue;
        }
        if folded != '-' && slug.len() >= MAX_SLUG_CHARS {
            break;
        }
        slug.push(folded);
    }
    let slug = slug.trim_end_matches('-');
    if slug.is_empty() {
        "session".to_owned()
    } else {
        slug.to_owned()
    }
}

/// `YYYY-MM-DD` (UTC).
fn date(at: Timestamp) -> String {
    let at = at.as_datetime();
    format!(
        "{:04}-{:02}-{:02}",
        at.year(),
        u8::from(at.month()),
        at.day()
    )
}

/// RFC 3339 in UTC, with milliseconds only when there are some.
fn rfc3339(at: Timestamp) -> String {
    let dt = at.as_datetime();
    let mut out = format!(
        "{}T{:02}:{:02}:{:02}",
        date(at),
        dt.hour(),
        dt.minute(),
        dt.second()
    );
    if dt.millisecond() != 0 {
        let _ = write!(out, ".{:03}", dt.millisecond());
    }
    out.push('Z');
    out
}

/// `HH:MM:SS` from the session start.
fn offset(at: Duration) -> String {
    let secs = at.as_millis() / 1_000;
    format!(
        "{:02}:{:02}:{:02}",
        secs / 3_600,
        (secs / 60) % 60,
        secs % 60
    )
}

/// `1 h 5 min`, `2 min 30 s`, `45 s`.
fn human(duration: Duration) -> String {
    let secs = duration.as_millis() / 1_000;
    let (h, m, s) = (secs / 3_600, (secs / 60) % 60, secs % 60);
    let mut parts = Vec::new();
    if h > 0 {
        parts.push(format!("{h} h"));
    }
    if m > 0 {
        parts.push(format!("{m} min"));
    }
    if s > 0 || parts.is_empty() {
        parts.push(format!("{s} s"));
    }
    parts.join(" ")
}

// ---------------------------------------------------------------------------------------------
// JSON — devcopilot.session-export v1
// ---------------------------------------------------------------------------------------------

fn structured(
    snapshot: &Snapshot,
    content: &BTreeSet<ExportContent>,
    exported_at: Timestamp,
) -> String {
    let session = &snapshot.session;
    let mut document = json!({
        "format": JSON_FORMAT,
        "version": JSON_VERSION,
        "exportedAt": rfc3339(exported_at),
        "session": {
            "id": session.id().to_string(),
            "title": session.title().as_str(),
            "language": session.language().code(),
            "state": session.state().code(),
            "startedAt": session.started_at().map(rfc3339),
            "endedAt": session.ended_at().map(rfc3339),
            "activeDurationSecs": session.active_duration().as_millis() / 1_000,
            "sources": session.sources().iter().map(|s| s.code()).collect::<Vec<_>>(),
        },
    });
    if content.contains(&ExportContent::Transcript) {
        document["transcript"] = snapshot.transcript.iter().map(segment_json).collect();
    }
    if content.contains(&ExportContent::Plan) {
        document["records"] = snapshot
            .records
            .iter()
            .map(|(record, history)| record_json(record, history))
            .collect();
        document["questions"] = snapshot.questions.iter().map(question_json).collect();
    }
    let mut out = serde_json::to_string_pretty(&document)
        .unwrap_or_else(|_| unreachable!("a JSON value always serializes"));
    out.push('\n');
    out
}

fn segment_json(segment: &TranscriptSegment) -> Value {
    match segment.kind() {
        SegmentKind::Final { text } => json!({
            "id": segment.id().to_string(),
            "seq": segment.seq(),
            "type": "final",
            "source": segment.source().map(AudioSourceKind::code),
            "startMs": segment.start().as_millis(),
            "endMs": segment.end().as_millis(),
            "text": text.expose().as_str(),
        }),
        SegmentKind::Gap { reason } => json!({
            "id": segment.id().to_string(),
            "seq": segment.seq(),
            "type": "gap",
            "reason": reason.code(),
            "startMs": segment.start().as_millis(),
            "durationMs": segment.duration().as_millis(),
        }),
    }
}

fn record_json(record: &TechnicalRecord, history: &[RecordChange]) -> Value {
    // `providerId` is left out on purpose: it may reveal configuration.
    let origin = match record.origin() {
        Origin::Ai {
            analysis_run_id, ..
        } => json!({ "type": "ai", "analysisRunId": analysis_run_id.to_string() }),
        Origin::User => json!({ "type": "user" }),
    };
    let flags = record.flags();
    json!({
        "id": record.id().to_string(),
        "kind": record.kind().code(),
        "text": record.text().expose().as_str(),
        "nature": record.nature().code(),
        "status": record.status().code(),
        "rationale": record.rationale().expose().as_str(),
        "origin": origin,
        "evidenceSegmentIds": record.evidence().iter().map(ToString::to_string).collect::<Vec<_>>(),
        "acceptanceCriteria": record.acceptance_criteria().iter().map(|i| i.expose().as_str()).collect::<Vec<_>>(),
        "alternatives": record.alternatives().iter().map(|i| i.expose().as_str()).collect::<Vec<_>>(),
        "flags": {
            "conflictsWith": flags.conflicts_with.map(|id| id.to_string()),
            "duplicateOf": flags.duplicate_of.map(|id| id.to_string()),
        },
        "createdAt": rfc3339(record.created_at()),
        "updatedAt": rfc3339(record.updated_at()),
        "history": history.iter().map(|change| json!({
            "field": change.field().code(),
            "actor": change.actor().code(),
            "oldValue": change.old_value().map(|v| v.expose().as_str()),
            "newValue": change.new_value().map(|v| v.expose().as_str()),
            "at": rfc3339(change.at()),
        })).collect::<Vec<_>>(),
    })
}

fn question_json(question: &ContextQuestion) -> Value {
    json!({
        "id": question.id().to_string(),
        "question": question.question().expose().as_str(),
        "answer": question.answer_text().map(|a| a.expose().as_str()),
        "certainty": question.certainty().map(Certainty::code),
        "evidence": question.evidence().iter().map(|evidence| match evidence {
            EvidenceRef::Segment(id) => json!({ "type": "segment", "id": id.to_string() }),
            EvidenceRef::Record(id) => json!({ "type": "record", "id": id.to_string() }),
        }).collect::<Vec<_>>(),
        "askedAt": rfc3339(question.asked_at()),
    })
}

// ---------------------------------------------------------------------------------------------
// Markdown
// ---------------------------------------------------------------------------------------------

/// Markdown labels in one interface language.
#[derive(Debug, Clone, Copy)]
struct Labels {
    plan: &'static str,
    no_items: &'static str,
    questions: &'static str,
    transcript: &'static str,
    active_duration: &'static str,
    language: &'static str,
    sources: &'static str,
    evidence: &'static str,
    rationale: &'static str,
    acceptance_criteria: &'static str,
    alternatives: &'static str,
    certainty: &'static str,
    question: &'static str,
    answer: &'static str,
    kind: fn(RecordKind) -> &'static str,
    status: fn(RecordStatus) -> &'static str,
    nature: fn(Nature) -> &'static str,
    origin: fn(Origin) -> &'static str,
    certainty_value: fn(Certainty) -> &'static str,
    source: fn(AudioSourceKind) -> &'static str,
    gap: fn(GapReason) -> &'static str,
}

const PT_BR: Labels = Labels {
    plan: "Plano",
    no_items: "Nenhum item.",
    questions: "Perguntas e respostas",
    transcript: "Transcrição",
    active_duration: "duração ativa",
    language: "idioma",
    sources: "fontes",
    evidence: "evidência",
    rationale: "Justificativa",
    acceptance_criteria: "Critérios de aceitação",
    alternatives: "Alternativas",
    certainty: "certeza",
    question: "P:",
    answer: "R:",
    kind: |kind| match kind {
        RecordKind::Task => "Tarefas",
        RecordKind::Requirement => "Requisitos",
        RecordKind::Decision => "Decisões",
        RecordKind::Assumption => "Premissas",
        RecordKind::Risk => "Riscos",
        RecordKind::OpenQuestion => "Perguntas em aberto",
    },
    status: |status| match status {
        RecordStatus::Proposed => "proposto",
        RecordStatus::Confirmed => "confirmado",
        RecordStatus::Rejected => "rejeitado",
        RecordStatus::Pending => "pendente",
    },
    nature: |nature| match nature {
        Nature::Observed => "observado",
        Nature::Inferred => "inferido",
        Nature::Suggested => "sugerido",
    },
    origin: |origin| match origin {
        Origin::Ai { .. } => "IA",
        Origin::User => "usuário",
    },
    certainty_value: |certainty| match certainty {
        Certainty::Supported => "apoiada",
        Certainty::Partial => "parcial",
        Certainty::NoEvidence => "sem evidência",
    },
    source: |source| match source {
        AudioSourceKind::Microphone => "microfone",
        AudioSourceKind::System => "áudio do sistema",
    },
    gap: |reason| match reason {
        GapReason::Paused => "pausa",
        GapReason::Overload => "áudio descartado por sobrecarga",
        GapReason::ProviderFailure => "falha do provedor",
        GapReason::CancelledOnStop => "transcrição cancelada ao parar",
        GapReason::SourceLost => "fonte perdida",
        GapReason::Interrupted => "sessão interrompida",
    },
};

const EN: Labels = Labels {
    plan: "Plan",
    no_items: "No items.",
    questions: "Questions and answers",
    transcript: "Transcript",
    active_duration: "active duration",
    language: "language",
    sources: "sources",
    evidence: "evidence",
    rationale: "Rationale",
    acceptance_criteria: "Acceptance criteria",
    alternatives: "Alternatives",
    certainty: "certainty",
    question: "Q:",
    answer: "A:",
    kind: |kind| match kind {
        RecordKind::Task => "Tasks",
        RecordKind::Requirement => "Requirements",
        RecordKind::Decision => "Decisions",
        RecordKind::Assumption => "Assumptions",
        RecordKind::Risk => "Risks",
        RecordKind::OpenQuestion => "Open questions",
    },
    status: |status| match status {
        RecordStatus::Proposed => "proposed",
        RecordStatus::Confirmed => "confirmed",
        RecordStatus::Rejected => "rejected",
        RecordStatus::Pending => "pending",
    },
    nature: |nature| match nature {
        Nature::Observed => "observed",
        Nature::Inferred => "inferred",
        Nature::Suggested => "suggested",
    },
    origin: |origin| match origin {
        Origin::Ai { .. } => "AI",
        Origin::User => "user",
    },
    certainty_value: |certainty| match certainty {
        Certainty::Supported => "supported",
        Certainty::Partial => "partial",
        Certainty::NoEvidence => "no evidence",
    },
    source: |source| match source {
        AudioSourceKind::Microphone => "microphone",
        AudioSourceKind::System => "system audio",
    },
    gap: |reason| match reason {
        GapReason::Paused => "pause",
        GapReason::Overload => "audio dropped under overload",
        GapReason::ProviderFailure => "provider failure",
        GapReason::CancelledOnStop => "transcription cancelled on stop",
        GapReason::SourceLost => "source lost",
        GapReason::Interrupted => "session interrupted",
    },
};

impl Labels {
    const fn for_language(language: UiLanguage) -> Self {
        match language {
            UiLanguage::PtBr => PT_BR,
            UiLanguage::En => EN,
        }
    }
}

/// Session text on one line, with Markdown and HTML syntax escaped.
fn inline(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for word in text.split_whitespace() {
        if !out.is_empty() {
            out.push(' ');
        }
        for c in word.chars() {
            if matches!(
                c,
                '\\' | '`' | '*' | '_' | '[' | ']' | '<' | '>' | '#' | '|' | '!' | '~'
            ) {
                out.push('\\');
            }
            out.push(c);
        }
    }
    out
}

/// At most `max` characters of `text`, with `…` when cut.
fn shorten(text: &str, max: usize) -> String {
    let mut chars = text.chars();
    let head: String = chars.by_ref().take(max).collect();
    if chars.next().is_some() {
        format!("{head}…")
    } else {
        head
    }
}

fn markdown(snapshot: &Snapshot, content: &BTreeSet<ExportContent>, labels: Labels) -> String {
    let session = &snapshot.session;
    let starts: HashMap<SegmentId, Duration> = snapshot
        .transcript
        .iter()
        .map(|segment| (segment.id(), segment.start()))
        .collect();
    let mut out = String::new();
    let _ = writeln!(out, "# {}\n", inline(session.title().as_str()));
    let sources: Vec<&str> = session
        .sources()
        .iter()
        .map(|s| (labels.source)(*s))
        .collect();
    let _ = writeln!(
        out,
        "{} · {}: {} · {}: {} · {}: {}",
        date(session.started_at().unwrap_or_else(|| session.created_at())),
        labels.active_duration,
        offset(session.active_duration()),
        labels.language,
        session.language().code(),
        labels.sources,
        sources.join(", "),
    );
    if content.contains(&ExportContent::Plan) {
        plan_markdown(&mut out, snapshot, &starts, labels);
        questions_markdown(&mut out, snapshot, &starts, labels);
    }
    if content.contains(&ExportContent::Transcript) {
        transcript_markdown(&mut out, snapshot, labels);
    }
    out
}

fn evidence_offsets(ids: &[SegmentId], starts: &HashMap<SegmentId, Duration>) -> Vec<String> {
    ids.iter()
        .filter_map(|id| starts.get(id).copied().map(offset))
        .collect()
}

fn plan_markdown(
    out: &mut String,
    snapshot: &Snapshot,
    starts: &HashMap<SegmentId, Duration>,
    labels: Labels,
) {
    let _ = writeln!(out, "\n## {}", labels.plan);
    let plan = DevelopmentPlan::assemble(
        snapshot.session.id(),
        snapshot.records.iter().map(|(r, _)| r.clone()).collect(),
    );
    let mut empty = true;
    for section in plan.sections.iter().filter(|s| !s.items.is_empty()) {
        empty = false;
        let _ = writeln!(out, "\n### {}\n", (labels.kind)(section.kind));
        for record in &section.items {
            let _ = write!(
                out,
                "- [{}] [{}] [{}] {}",
                (labels.status)(record.status()),
                (labels.nature)(record.nature()),
                (labels.origin)(record.origin()),
                inline(record.text().expose().as_str()),
            );
            let evidence = evidence_offsets(record.evidence(), starts);
            if !evidence.is_empty() {
                let _ = write!(out, " — {}: {}", labels.evidence, evidence.join(", "));
            }
            out.push('\n');
            let rationale = record.rationale().expose();
            if !rationale.is_empty() {
                let _ = writeln!(
                    out,
                    "  - {}: {}",
                    labels.rationale,
                    inline(rationale.as_str())
                );
            }
            for (label, items) in [
                (labels.acceptance_criteria, record.acceptance_criteria()),
                (labels.alternatives, record.alternatives()),
            ] {
                if !items.is_empty() {
                    let items: Vec<String> =
                        items.iter().map(|i| inline(i.expose().as_str())).collect();
                    let _ = writeln!(out, "  - {label}: {}", items.join("; "));
                }
            }
        }
    }
    if empty {
        let _ = writeln!(out, "\n{}", labels.no_items);
    }
}

fn questions_markdown(
    out: &mut String,
    snapshot: &Snapshot,
    starts: &HashMap<SegmentId, Duration>,
    labels: Labels,
) {
    let _ = writeln!(out, "\n## {}", labels.questions);
    if snapshot.questions.is_empty() {
        let _ = writeln!(out, "\n{}", labels.no_items);
        return;
    }
    let texts: HashMap<RecordId, &str> = snapshot
        .records
        .iter()
        .map(|(record, _)| (record.id(), record.text().expose().as_str()))
        .collect();
    for question in &snapshot.questions {
        let _ = write!(
            out,
            "\n**{}** {}  \n**{}** {}",
            labels.question,
            inline(question.question().expose().as_str()),
            labels.answer,
            inline(question.answer_text().map_or("", |a| a.expose().as_str())),
        );
        let mut notes = Vec::new();
        if let Some(certainty) = question.certainty() {
            notes.push(format!(
                "{}: {}",
                labels.certainty,
                (labels.certainty_value)(certainty)
            ));
        }
        let cited: Vec<String> = question
            .evidence()
            .iter()
            .filter_map(|evidence| match evidence {
                EvidenceRef::Segment(id) => starts.get(id).copied().map(offset),
                EvidenceRef::Record(id) => texts
                    .get(id)
                    .map(|text| format!("“{}”", inline(&shorten(text, QUOTE_CHARS)))),
            })
            .collect();
        if !cited.is_empty() {
            notes.push(format!("{}: {}", labels.evidence, cited.join(", ")));
        }
        if !notes.is_empty() {
            let _ = write!(out, " ({})", notes.join("; "));
        }
        out.push('\n');
    }
}

fn transcript_markdown(out: &mut String, snapshot: &Snapshot, labels: Labels) {
    let _ = writeln!(out, "\n## {}\n", labels.transcript);
    if snapshot.transcript.is_empty() {
        let _ = writeln!(out, "{}", labels.no_items);
        return;
    }
    for segment in &snapshot.transcript {
        match segment.kind() {
            SegmentKind::Final { text } => {
                let _ = write!(out, "\\[{}\\]", offset(segment.start()));
                if let Some(source) = segment.source() {
                    let _ = write!(out, " ({})", (labels.source)(source));
                }
                let _ = writeln!(out, " {}  ", inline(text.expose().as_str()));
            }
            SegmentKind::Gap { reason } => {
                let _ = writeln!(
                    out,
                    "\\[{}\\] — {} ({}) —  ",
                    offset(segment.start()),
                    (labels.gap)(*reason),
                    human(segment.duration()),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_folds_accents_and_collapses_separators() {
        assert_eq!(
            slug("Revisão da API de exportação"),
            "revisao-da-api-de-exportacao"
        );
        assert_eq!(slug("  ../../etc/passwd  "), "etc-passwd");
        assert_eq!(slug("日本語"), "session");
        assert_eq!(slug(&"a".repeat(200)).len(), MAX_SLUG_CHARS);
    }

    #[test]
    fn times_are_formatted_in_utc() {
        let at = Timestamp::from_unix_millis(1_793_124_000_000).unwrap();
        assert_eq!(rfc3339(at), "2026-10-27T18:00:00Z");
        let at = Timestamp::from_unix_millis(1_793_124_000_042).unwrap();
        assert_eq!(rfc3339(at), "2026-10-27T18:00:00.042Z");
        assert_eq!(offset(Duration::from_millis(3_723_999)), "01:02:03");
        assert_eq!(human(Duration::from_secs(60)), "1 min");
        assert_eq!(human(Duration::from_secs(3_930)), "1 h 5 min 30 s");
        assert_eq!(human(Duration::from_millis(400)), "0 s");
    }

    #[test]
    fn inline_escapes_markdown_and_joins_lines() {
        assert_eq!(
            inline("# título\n<script>alert(1)</script> [x](http://e)"),
            "\\# título \\<script\\>alert(1)\\</script\\> \\[x\\](http://e)"
        );
    }

    #[test]
    fn shorten_marks_cut_text() {
        assert_eq!(shorten("abc", 3), "abc");
        assert_eq!(shorten("abcd", 3), "abc…");
    }
}
