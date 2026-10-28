//! Bounded text value types for session content (data-model.md).
//!
//! Lengths are counted in Unicode scalar values. Titles are single-line; the other types accept
//! line breaks and tabs but no other control characters.

use std::fmt;

use crate::error::{DomainError, InputProblem};

fn validate(
    value: &str,
    field: &'static str,
    max_chars: usize,
    multiline: bool,
) -> Result<(), DomainError> {
    if value.trim().is_empty() {
        return Err(DomainError::invalid(field, InputProblem::Empty));
    }
    if value.chars().count() > max_chars {
        return Err(DomainError::invalid(field, InputProblem::TooLong));
    }
    let allowed = |c: char| multiline && matches!(c, '\n' | '\r' | '\t');
    if value.chars().any(|c| c.is_control() && !allowed(c)) {
        return Err(DomainError::invalid(field, InputProblem::ControlCharacter));
    }
    Ok(())
}

macro_rules! bounded_text {
    ($(#[$doc:meta])* $name:ident, field = $field:literal, max = $max:literal, multiline = $multi:literal) => {
        $(#[$doc])*
        ///
        /// `Debug` is redacted because the value is session content (FR-051).
        #[derive(Clone, PartialEq, Eq, Hash)]
        pub struct $name(String);

        impl $name {
            /// Maximum length in characters.
            pub const MAX_CHARS: usize = $max;

            /// Validates and wraps `value`.
            ///
            /// # Errors
            ///
            /// Returns [`DomainError::InvalidInput`] when the value is blank, too long or
            /// contains disallowed control characters.
            pub fn parse(value: impl Into<String>) -> Result<Self, DomainError> {
                let value = value.into();
                validate(&value, $field, $max, $multi)?;
                Ok(Self(value))
            }

            /// The validated text.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }

            /// Unwraps the validated text.
            #[must_use]
            pub fn into_inner(self) -> String {
                self.0
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(concat!(stringify!($name), "([redacted])"))
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }
    };
}

bounded_text!(
    /// Session title: 1–200 characters, single line.
    Title, field = "title", max = 200, multiline = false
);
bounded_text!(
    /// Technical record text: 1–2 000 characters.
    RecordText, field = "text", max = 2000, multiline = true
);
bounded_text!(
    /// Final transcript segment text: 1–10 000 characters.
    SegmentText, field = "text", max = 10000, multiline = true
);
bounded_text!(
    /// Contextual question: 1–2 000 characters.
    QuestionText, field = "question", max = 2000, multiline = true
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_bounds() {
        assert!(Title::parse("a").is_ok());
        assert!(Title::parse("é".repeat(200)).is_ok());
        assert!(RecordText::parse("x".repeat(2000)).is_ok());
        assert!(SegmentText::parse("x".repeat(10_000)).is_ok());
        assert!(QuestionText::parse("x".repeat(2000)).is_ok());
    }

    #[test]
    fn rejects_blank() {
        for value in ["", "   ", "\n\t"] {
            assert_eq!(
                Title::parse(value),
                Err(DomainError::invalid("title", InputProblem::Empty))
            );
        }
        assert_eq!(
            QuestionText::parse(" "),
            Err(DomainError::invalid("question", InputProblem::Empty))
        );
    }

    #[test]
    fn rejects_over_max() {
        assert_eq!(
            Title::parse("x".repeat(201)),
            Err(DomainError::invalid("title", InputProblem::TooLong))
        );
        assert!(RecordText::parse("x".repeat(2001)).is_err());
        assert!(SegmentText::parse("x".repeat(10_001)).is_err());
        assert!(QuestionText::parse("x".repeat(2001)).is_err());
    }

    #[test]
    fn title_rejects_any_control_character() {
        for value in ["a\nb", "a\tb", "a\u{0}b", "a\u{1b}[31m", "a\u{7f}"] {
            assert_eq!(
                Title::parse(value),
                Err(DomainError::invalid(
                    "title",
                    InputProblem::ControlCharacter
                )),
                "{value:?}"
            );
        }
    }

    #[test]
    fn multiline_types_allow_line_breaks_only() {
        assert!(RecordText::parse("linha 1\r\nlinha 2\tfim").is_ok());
        assert_eq!(
            RecordText::parse("a\u{0}b"),
            Err(DomainError::invalid("text", InputProblem::ControlCharacter))
        );
        assert!(SegmentText::parse("a\u{1b}b").is_err());
    }

    #[test]
    fn debug_is_redacted() {
        let title = Title::parse("Reunião confidencial").unwrap();
        assert_eq!(format!("{title:?}"), "Title([redacted])");
        assert_eq!(title.as_str(), "Reunião confidencial");
    }
}
