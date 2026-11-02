//! Languages for session content (FR-053b) and for the interface (FR-053a).

use crate::error::{DomainError, InputProblem};

macro_rules! language_enum {
    ($(#[$doc:meta])* $name:ident, field = $field:literal) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
        pub enum $name {
            /// Brazilian Portuguese.
            #[default]
            PtBr,
            /// English.
            En,
        }

        impl $name {
            /// Every supported value.
            pub const ALL: [Self; 2] = [Self::PtBr, Self::En];

            /// BCP 47 tag used over IPC and in storage.
            #[must_use]
            pub const fn code(self) -> &'static str {
                match self {
                    Self::PtBr => "pt-BR",
                    Self::En => "en",
                }
            }

            /// Parses a BCP 47 tag.
            ///
            /// # Errors
            ///
            /// Returns [`DomainError::InvalidInput`] for unsupported tags.
            pub fn parse(code: &str) -> Result<Self, DomainError> {
                Self::ALL
                    .into_iter()
                    .find(|lang| lang.code() == code)
                    .ok_or(DomainError::invalid($field, InputProblem::Malformed))
            }
        }
    };
}

language_enum!(
    /// Language spoken in a session; drives transcription and generated content.
    SessionLanguage, field = "language"
);
language_enum!(
    /// Language of the user interface.
    UiLanguage, field = "ui_language"
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_round_trip() {
        for lang in SessionLanguage::ALL {
            assert_eq!(SessionLanguage::parse(lang.code()), Ok(lang));
        }
        for lang in UiLanguage::ALL {
            assert_eq!(UiLanguage::parse(lang.code()), Ok(lang));
        }
    }

    #[test]
    fn rejects_unknown_or_differently_cased_tags() {
        for code in ["pt", "pt-br", "EN", "es", ""] {
            assert!(SessionLanguage::parse(code).is_err(), "{code}");
        }
        assert_eq!(
            UiLanguage::parse("fr"),
            Err(DomainError::invalid("ui_language", InputProblem::Malformed))
        );
    }
}
