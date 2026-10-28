//! Identifier newtypes over UUID v7 (time-ordered).

use std::fmt;
use std::str::FromStr;

use uuid::Uuid;

use crate::error::{DomainError, InputProblem};

macro_rules! id_newtype {
    ($(#[$doc:meta])* $name:ident, $field:literal) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(Uuid);

        impl $name {
            /// Generates a new time-ordered identifier.
            #[must_use]
            pub fn new() -> Self {
                Self(Uuid::now_v7())
            }

            /// Wraps an existing UUID (e.g. read back from storage).
            #[must_use]
            pub const fn from_uuid(uuid: Uuid) -> Self {
                Self(uuid)
            }

            /// Parses the hyphenated string form used over IPC.
            ///
            /// # Errors
            ///
            /// Returns [`DomainError::InvalidInput`] when `value` is not a UUID.
            pub fn parse(value: &str) -> Result<Self, DomainError> {
                Uuid::try_parse(value)
                    .map(Self)
                    .map_err(|_| DomainError::invalid($field, InputProblem::Malformed))
            }

            /// The underlying UUID.
            #[must_use]
            pub const fn as_uuid(&self) -> &Uuid {
                &self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.hyphenated().fmt(f)
            }
        }

        impl FromStr for $name {
            type Err = DomainError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Self::parse(value)
            }
        }
    };
}

id_newtype!(
    /// Identifies a meeting session.
    SessionId, "session_id"
);
id_newtype!(
    /// Identifies a final transcript segment or gap.
    SegmentId, "segment_id"
);
id_newtype!(
    /// Identifies a technical record.
    RecordId, "record_id"
);
id_newtype!(
    /// Identifies an entry of a record's change history.
    ChangeId, "change_id"
);
id_newtype!(
    /// Identifies a contextual question.
    QuestionId, "question_id"
);
id_newtype!(
    /// Identifies an analysis run.
    AnalysisRunId, "analysis_run_id"
);
id_newtype!(
    /// Identifies a configured transcription or analysis provider.
    ProviderId, "provider_id"
);
id_newtype!(
    /// Identifies an export event.
    ExportId, "export_id"
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_ids_are_v7_and_time_ordered() {
        let first = SessionId::new();
        let second = SessionId::new();
        assert_eq!(first.as_uuid().get_version_num(), 7);
        assert!(first < second);
    }

    #[test]
    fn round_trips_through_string() {
        let id = RecordId::new();
        assert_eq!(RecordId::parse(&id.to_string()), Ok(id));
    }

    #[test]
    fn rejects_malformed_ids_naming_the_field() {
        assert_eq!(
            ProviderId::parse("not-a-uuid"),
            Err(DomainError::invalid("provider_id", InputProblem::Malformed))
        );
    }
}
