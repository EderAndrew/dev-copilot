//! Contextual questions about a session and their grounded answers (data-model.md
//! "ContextQuestion", FR-034–FR-038, SC-011).
//!
//! Answers come from a provider and are untrusted: they reach the domain as an [`Answer`] whose
//! evidence is checked against the session before it is stored ([`Answer::grounded`]). An
//! answer never changes records; the user may turn it into a new `proposed` record by explicit
//! action ([`ContextQuestion::to_record`], FR-037).

use std::collections::BTreeSet;

use crate::Redacted;
use crate::error::{DomainError, InputProblem};
use crate::ids::{ProviderId, QuestionId, RecordId, SegmentId, SessionId};
use crate::record::{
    MAX_EVIDENCE, Nature, Origin, RecordFlags, RecordKind, RecordSpec, RecordStatus,
    TechnicalRecord,
};
use crate::text::{AnswerText, QuestionText, Rationale, RecordText};
use crate::time::Timestamp;

/// How well the session supports an answer (FR-036).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Certainty {
    /// The cited evidence states the answer.
    Supported,
    /// The cited evidence covers only part of the answer.
    Partial,
    /// The session holds no evidence for an answer; nothing is cited.
    NoEvidence,
}

code_enum!(Certainty, "certainty", {
    Supported => "supported",
    Partial => "partial",
    NoEvidence => "no_evidence",
});

/// Lifecycle of a question.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum QuestionStatus {
    /// Waiting for the provider.
    Pending,
    /// Answered (possibly with `no_evidence`).
    Answered,
    /// The provider failed; nothing was answered.
    Failed,
}

code_enum!(QuestionStatus, "status", {
    Pending => "pending",
    Answered => "answered",
    Failed => "failed",
});

/// Session content cited by an answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EvidenceRef {
    /// A final transcript segment, quoted from storage in its original language (FR-053b).
    Segment(SegmentId),
    /// A technical record.
    Record(RecordId),
}

/// An answer as returned by a provider, already schema-validated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answer {
    /// Text, in the session language (FR-053b).
    pub text: Redacted<AnswerText>,
    /// Certainty claimed by the model.
    pub certainty: Certainty,
    /// Evidence cited by the model.
    pub evidence: Vec<EvidenceRef>,
}

impl Answer {
    /// Keeps only evidence for which `in_session` holds, without repeats and at most
    /// [`MAX_EVIDENCE`]; a `no_evidence` answer cites nothing, and an answer left without
    /// evidence becomes `no_evidence` (data-model.md, SC-011).
    #[must_use]
    pub fn grounded(mut self, in_session: impl Fn(EvidenceRef) -> bool) -> Self {
        let mut seen = BTreeSet::new();
        self.evidence
            .retain(|evidence| in_session(*evidence) && seen.insert(*evidence));
        self.evidence.truncate(MAX_EVIDENCE);
        if self.certainty == Certainty::NoEvidence {
            self.evidence.clear();
        }
        if self.evidence.is_empty() {
            self.certainty = Certainty::NoEvidence;
        }
        self
    }
}

/// Fields of a question, also used to restore stored rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuestionSpec {
    /// Identifier.
    pub id: QuestionId,
    /// Session asked about.
    pub session_id: SessionId,
    /// The user's question.
    pub question: Redacted<QuestionText>,
    /// Answer; `None` while pending or after a failure.
    pub answer: Option<Redacted<AnswerText>>,
    /// Certainty; present iff answered.
    pub certainty: Option<Certainty>,
    /// Evidence cited by the answer.
    pub evidence: Vec<EvidenceRef>,
    /// Lifecycle.
    pub status: QuestionStatus,
    /// Provider asked.
    pub provider_id: Option<ProviderId>,
    /// When the user asked.
    pub asked_at: Timestamp,
    /// When the answer arrived or the provider failed.
    pub answered_at: Option<Timestamp>,
}

/// A contextual question.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextQuestion {
    spec: QuestionSpec,
}

impl ContextQuestion {
    /// A new `pending` question to `provider_id`.
    #[must_use]
    pub const fn ask(
        id: QuestionId,
        session_id: SessionId,
        question: Redacted<QuestionText>,
        provider_id: Option<ProviderId>,
        asked_at: Timestamp,
    ) -> Self {
        Self::restore(QuestionSpec {
            id,
            session_id,
            question,
            answer: None,
            certainty: None,
            evidence: Vec::new(),
            status: QuestionStatus::Pending,
            provider_id,
            asked_at,
            answered_at: None,
        })
    }

    /// Rebuilds a stored question without validation.
    #[must_use]
    pub const fn restore(spec: QuestionSpec) -> Self {
        Self { spec }
    }

    /// Stores a grounded answer.
    ///
    /// # Errors
    ///
    /// [`DomainError::InvalidTransition`] unless pending; [`DomainError::InvalidInput`] on
    /// `evidence` when the answer was not grounded (`no_evidence` citing something, or another
    /// certainty citing nothing, or too many references).
    pub fn answer(&mut self, answer: Answer, at: Timestamp) -> Result<(), DomainError> {
        self.check_pending("answer")?;
        let consistent = (answer.certainty == Certainty::NoEvidence) == answer.evidence.is_empty();
        if !consistent {
            return Err(DomainError::invalid("evidence", InputProblem::Inconsistent));
        }
        if answer.evidence.len() > MAX_EVIDENCE {
            return Err(DomainError::invalid("evidence", InputProblem::TooLong));
        }
        self.spec.answer = Some(answer.text);
        self.spec.certainty = Some(answer.certainty);
        self.spec.evidence = answer.evidence;
        self.spec.status = QuestionStatus::Answered;
        self.spec.answered_at = Some(at.max(self.spec.asked_at));
        Ok(())
    }

    /// Records that the provider failed.
    ///
    /// # Errors
    ///
    /// [`DomainError::InvalidTransition`] unless pending.
    pub fn fail(&mut self, at: Timestamp) -> Result<(), DomainError> {
        self.check_pending("fail")?;
        self.spec.status = QuestionStatus::Failed;
        self.spec.answered_at = Some(at.max(self.spec.asked_at));
        Ok(())
    }

    /// A new record of `kind` made from the answer by explicit user action (FR-037): origin
    /// `user`, status `proposed`, nature `suggested`, the question as rationale and the cited
    /// segments as evidence. Answers longer than a record are cut with `…`.
    ///
    /// # Errors
    ///
    /// [`DomainError::InvalidTransition`] unless answered.
    pub fn to_record(
        &self,
        id: RecordId,
        kind: RecordKind,
        now: Timestamp,
    ) -> Result<TechnicalRecord, DomainError> {
        let Some(answer) = self.spec.answer.as_ref().filter(|_| self.is_answered()) else {
            return Err(DomainError::InvalidTransition {
                from: self.spec.status.code(),
                action: "to_record",
            });
        };
        let answer = answer.expose().as_str();
        let text = if answer.chars().count() > RecordText::MAX_CHARS {
            let cut: String = answer.chars().take(RecordText::MAX_CHARS - 1).collect();
            format!("{}…", cut.trim_end())
        } else {
            answer.to_owned()
        };
        TechnicalRecord::new(RecordSpec {
            id,
            session_id: self.spec.session_id,
            kind,
            text: Redacted::new(RecordText::parse(text)?),
            nature: Nature::Suggested,
            status: RecordStatus::Proposed,
            rationale: Redacted::new(Rationale::parse(self.spec.question.expose().as_str())?),
            origin: Origin::User,
            evidence: self
                .spec
                .evidence
                .iter()
                .filter_map(|evidence| match evidence {
                    EvidenceRef::Segment(segment) => Some(*segment),
                    EvidenceRef::Record(_) => None,
                })
                .collect(),
            acceptance_criteria: Vec::new(),
            alternatives: Vec::new(),
            flags: RecordFlags::default(),
            created_at: now,
            updated_at: now,
        })
    }

    fn check_pending(&self, action: &'static str) -> Result<(), DomainError> {
        if self.spec.status == QuestionStatus::Pending {
            Ok(())
        } else {
            Err(DomainError::InvalidTransition {
                from: self.spec.status.code(),
                action,
            })
        }
    }

    fn is_answered(&self) -> bool {
        self.spec.status == QuestionStatus::Answered
    }

    /// All fields.
    #[must_use]
    pub const fn spec(&self) -> &QuestionSpec {
        &self.spec
    }

    /// Identifier.
    #[must_use]
    pub const fn id(&self) -> QuestionId {
        self.spec.id
    }

    /// Session asked about.
    #[must_use]
    pub const fn session_id(&self) -> SessionId {
        self.spec.session_id
    }

    /// The user's question.
    #[must_use]
    pub const fn question(&self) -> &Redacted<QuestionText> {
        &self.spec.question
    }

    /// Answer, once answered.
    #[must_use]
    pub const fn answer_text(&self) -> Option<&Redacted<AnswerText>> {
        self.spec.answer.as_ref()
    }

    /// Certainty, once answered.
    #[must_use]
    pub const fn certainty(&self) -> Option<Certainty> {
        self.spec.certainty
    }

    /// Evidence cited by the answer.
    #[must_use]
    pub fn evidence(&self) -> &[EvidenceRef] {
        &self.spec.evidence
    }

    /// Lifecycle.
    #[must_use]
    pub const fn status(&self) -> QuestionStatus {
        self.spec.status
    }

    /// Provider asked.
    #[must_use]
    pub const fn provider_id(&self) -> Option<ProviderId> {
        self.spec.provider_id
    }

    /// When the user asked.
    #[must_use]
    pub const fn asked_at(&self) -> Timestamp {
        self.spec.asked_at
    }

    /// When the answer arrived or the provider failed.
    #[must_use]
    pub const fn answered_at(&self) -> Option<Timestamp> {
        self.spec.answered_at
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(millis: i64) -> Timestamp {
        Timestamp::from_unix_millis(millis).unwrap()
    }

    fn pending() -> ContextQuestion {
        ContextQuestion::ask(
            QuestionId::new(),
            SessionId::new(),
            Redacted::new(QuestionText::parse("O que foi decidido sobre autenticação?").unwrap()),
            Some(ProviderId::new()),
            at(1_000),
        )
    }

    fn answer(text: &str, certainty: Certainty, evidence: Vec<EvidenceRef>) -> Answer {
        Answer {
            text: Redacted::new(AnswerText::parse(text).unwrap()),
            certainty,
            evidence,
        }
    }

    #[test]
    fn grounding_drops_foreign_and_repeated_evidence() {
        let known = SegmentId::new();
        let record = RecordId::new();
        let grounded = answer(
            "Usar OAuth2.",
            Certainty::Supported,
            vec![
                EvidenceRef::Segment(known),
                EvidenceRef::Segment(SegmentId::new()),
                EvidenceRef::Segment(known),
                EvidenceRef::Record(record),
                EvidenceRef::Record(RecordId::new()),
            ],
        )
        .grounded(|e| e == EvidenceRef::Segment(known) || e == EvidenceRef::Record(record));
        assert_eq!(
            grounded.evidence,
            [EvidenceRef::Segment(known), EvidenceRef::Record(record)]
        );
        assert_eq!(grounded.certainty, Certainty::Supported);
    }

    #[test]
    fn answers_left_without_evidence_become_no_evidence() {
        for certainty in [Certainty::Supported, Certainty::Partial] {
            let grounded = answer(
                "Inventado",
                certainty,
                vec![EvidenceRef::Segment(SegmentId::new())],
            )
            .grounded(|_| false);
            assert_eq!(grounded.certainty, Certainty::NoEvidence);
            assert!(grounded.evidence.is_empty());
        }
    }

    #[test]
    fn no_evidence_answers_cite_nothing() {
        let segment = SegmentId::new();
        let grounded = answer(
            "Não há evidência.",
            Certainty::NoEvidence,
            vec![EvidenceRef::Segment(segment)],
        )
        .grounded(|_| true);
        assert_eq!(grounded.certainty, Certainty::NoEvidence);
        assert!(grounded.evidence.is_empty());
    }

    #[test]
    fn evidence_is_capped() {
        let refs: Vec<EvidenceRef> = (0..30)
            .map(|_| EvidenceRef::Segment(SegmentId::new()))
            .collect();
        let grounded = answer("Muitos trechos", Certainty::Partial, refs).grounded(|_| true);
        assert_eq!(grounded.evidence.len(), MAX_EVIDENCE);
    }

    #[test]
    fn answering_requires_a_grounded_answer() {
        let mut question = pending();
        assert_eq!(
            question.answer(
                answer("Sem citação", Certainty::Supported, Vec::new()),
                at(2_000)
            ),
            Err(DomainError::invalid("evidence", InputProblem::Inconsistent))
        );
        assert_eq!(question.status(), QuestionStatus::Pending);
        let segment = SegmentId::new();
        question
            .answer(
                answer(
                    "Usar OAuth2.",
                    Certainty::Supported,
                    vec![EvidenceRef::Segment(segment)],
                ),
                at(2_000),
            )
            .unwrap();
        assert_eq!(question.status(), QuestionStatus::Answered);
        assert_eq!(question.certainty(), Some(Certainty::Supported));
        assert_eq!(question.evidence(), [EvidenceRef::Segment(segment)]);
        assert_eq!(question.answered_at(), Some(at(2_000)));
    }

    #[test]
    fn only_pending_questions_are_answered_or_failed() {
        let mut question = pending();
        question.fail(at(500)).unwrap();
        assert_eq!(question.status(), QuestionStatus::Failed);
        // Never before the question was asked.
        assert_eq!(question.answered_at(), Some(at(1_000)));
        assert!(question.answer_text().is_none());
        assert!(matches!(
            question.answer(
                answer("tarde demais", Certainty::NoEvidence, Vec::new()),
                at(3_000)
            ),
            Err(DomainError::InvalidTransition { .. })
        ));
        assert!(matches!(
            question.fail(at(3_000)),
            Err(DomainError::InvalidTransition { .. })
        ));
    }

    #[test]
    fn answers_become_proposed_suggested_user_records() {
        let mut question = pending();
        let segment = SegmentId::new();
        question
            .answer(
                answer(
                    "Sugiro trocar OAuth2 por SSO.",
                    Certainty::Partial,
                    vec![
                        EvidenceRef::Segment(segment),
                        EvidenceRef::Record(RecordId::new()),
                    ],
                ),
                at(2_000),
            )
            .unwrap();
        let record = question
            .to_record(RecordId::new(), RecordKind::Decision, at(3_000))
            .unwrap();
        assert_eq!(record.status(), RecordStatus::Proposed);
        assert_eq!(record.nature(), Nature::Suggested);
        assert_eq!(record.origin(), Origin::User);
        assert_eq!(record.kind(), RecordKind::Decision);
        assert_eq!(
            record.text().expose().as_str(),
            "Sugiro trocar OAuth2 por SSO."
        );
        assert_eq!(
            record.rationale().expose().as_str(),
            "O que foi decidido sobre autenticação?"
        );
        assert_eq!(record.evidence(), [segment]);
        assert_eq!(record.session_id(), question.session_id());
    }

    #[test]
    fn long_answers_are_cut_to_a_record() {
        let mut question = pending();
        question
            .answer(
                answer(&"x".repeat(4_000), Certainty::NoEvidence, Vec::new()),
                at(2_000),
            )
            .unwrap();
        let record = question
            .to_record(RecordId::new(), RecordKind::Task, at(3_000))
            .unwrap();
        let text = record.text().expose().as_str();
        assert_eq!(text.chars().count(), RecordText::MAX_CHARS);
        assert!(text.ends_with('…'));
    }

    #[test]
    fn unanswered_questions_cannot_become_records() {
        let mut question = pending();
        assert!(matches!(
            question.to_record(RecordId::new(), RecordKind::Task, at(2_000)),
            Err(DomainError::InvalidTransition { .. })
        ));
        question.fail(at(2_000)).unwrap();
        assert!(
            question
                .to_record(RecordId::new(), RecordKind::Task, at(2_000))
                .is_err()
        );
    }

    #[test]
    fn debug_output_hides_content() {
        let question = pending();
        assert!(!format!("{question:?}").contains("autenticação"));
    }

    #[test]
    fn codes_round_trip() {
        for certainty in [
            Certainty::Supported,
            Certainty::Partial,
            Certainty::NoEvidence,
        ] {
            assert_eq!(Certainty::parse(certainty.code()), Ok(certainty));
        }
        for status in [
            QuestionStatus::Pending,
            QuestionStatus::Answered,
            QuestionStatus::Failed,
        ] {
            assert_eq!(QuestionStatus::parse(status.code()), Ok(status));
        }
    }
}
