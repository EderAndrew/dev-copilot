//! Export events (data-model.md "ExportRecord", FR-041).
//!
//! An export is recorded with its session, formats, content and instant — never the file path,
//! which may reveal information and goes stale.

use std::collections::BTreeSet;

use crate::error::{DomainError, InputProblem};
use crate::ids::{ExportId, SessionId};
use crate::time::Timestamp;

/// File format of an export.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ExportFormat {
    /// Human-readable Markdown.
    Markdown,
    /// Structured JSON (`devcopilot.session-export` v1).
    Json,
}

code_enum!(ExportFormat, "formats", {
    Markdown => "markdown",
    Json => "json",
});

impl ExportFormat {
    /// File extension, without the dot.
    #[must_use]
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Markdown => "md",
            Self::Json => "json",
        }
    }
}

/// Part of a session included in an export.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ExportContent {
    /// Records (the development plan) and questions with their answers.
    Plan,
    /// Final segments and gaps.
    Transcript,
}

code_enum!(ExportContent, "content", {
    Plan => "plan",
    Transcript => "transcript",
});

/// One export performed by the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportRecord {
    id: ExportId,
    session_id: SessionId,
    formats: BTreeSet<ExportFormat>,
    content: BTreeSet<ExportContent>,
    at: Timestamp,
}

impl ExportRecord {
    /// Validates an export record.
    ///
    /// # Errors
    ///
    /// [`DomainError::InvalidInput`] (`formats` or `content`) when either set is empty.
    pub fn new(
        id: ExportId,
        session_id: SessionId,
        formats: BTreeSet<ExportFormat>,
        content: BTreeSet<ExportContent>,
        at: Timestamp,
    ) -> Result<Self, DomainError> {
        if formats.is_empty() {
            return Err(DomainError::invalid("formats", InputProblem::Empty));
        }
        if content.is_empty() {
            return Err(DomainError::invalid("content", InputProblem::Empty));
        }
        Ok(Self {
            id,
            session_id,
            formats,
            content,
            at,
        })
    }

    /// Identifier.
    #[must_use]
    pub const fn id(&self) -> ExportId {
        self.id
    }

    /// Exported session.
    #[must_use]
    pub const fn session_id(&self) -> SessionId {
        self.session_id
    }

    /// Formats written.
    #[must_use]
    pub const fn formats(&self) -> &BTreeSet<ExportFormat> {
        &self.formats
    }

    /// Content included.
    #[must_use]
    pub const fn content(&self) -> &BTreeSet<ExportContent> {
        &self.content
    }

    /// When the files were written.
    #[must_use]
    pub const fn at(&self) -> Timestamp {
        self.at
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_round_trip() {
        for format in [ExportFormat::Markdown, ExportFormat::Json] {
            assert_eq!(ExportFormat::parse(format.code()), Ok(format));
        }
        for content in [ExportContent::Plan, ExportContent::Transcript] {
            assert_eq!(ExportContent::parse(content.code()), Ok(content));
        }
        assert!(ExportFormat::parse("csv").is_err());
    }

    #[test]
    fn needs_a_format_and_content() {
        let at = Timestamp::from_unix_millis(0).unwrap();
        let formats = BTreeSet::from([ExportFormat::Json]);
        let content = BTreeSet::from([ExportContent::Plan]);
        assert!(
            ExportRecord::new(
                ExportId::new(),
                SessionId::new(),
                BTreeSet::new(),
                content.clone(),
                at
            )
            .is_err()
        );
        assert!(
            ExportRecord::new(
                ExportId::new(),
                SessionId::new(),
                formats.clone(),
                BTreeSet::new(),
                at
            )
            .is_err()
        );
        let record = ExportRecord::new(ExportId::new(), SessionId::new(), formats, content, at);
        assert!(record.is_ok());
    }
}
