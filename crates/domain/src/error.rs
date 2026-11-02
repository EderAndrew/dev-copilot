//! Domain errors.
//!
//! Messages name fields and states only; they never carry session content (FR-051).

use thiserror::Error;

/// Why a field value was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputProblem {
    /// Empty or whitespace-only where content is required.
    Empty,
    /// Longer than the allowed maximum.
    TooLong,
    /// Contains control characters that are not allowed for this field.
    ControlCharacter,
    /// Not one of the accepted values or not in the expected format.
    Malformed,
    /// Combination with another field is not allowed.
    Inconsistent,
}

impl InputProblem {
    /// Stable snake_case code, safe to log and to send over IPC.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Empty => "empty",
            Self::TooLong => "too_long",
            Self::ControlCharacter => "control_character",
            Self::Malformed => "malformed",
            Self::Inconsistent => "inconsistent",
        }
    }
}

/// Errors raised by domain rules.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DomainError {
    /// A state machine received an action that is not allowed in its current state.
    #[error("transition `{action}` is not allowed from state `{from}`")]
    InvalidTransition {
        /// Current state code.
        from: &'static str,
        /// Attempted action or target state code.
        action: &'static str,
    },
    /// A value failed validation.
    #[error("invalid input for `{field}`: {}", problem.code())]
    InvalidInput {
        /// Field name as exposed in contracts (camelCase-free, snake_case).
        field: &'static str,
        /// What was wrong with it.
        problem: InputProblem,
    },
    /// A cross-field or cross-entity rule was broken.
    #[error("invariant violated: {0}")]
    InvariantViolation(&'static str),
}

impl DomainError {
    /// Shorthand for [`DomainError::InvalidInput`].
    #[must_use]
    pub const fn invalid(field: &'static str, problem: InputProblem) -> Self {
        Self::InvalidInput { field, problem }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_name_fields_and_codes_only() {
        let err = DomainError::invalid("title", InputProblem::TooLong);
        assert_eq!(err.to_string(), "invalid input for `title`: too_long");
        let err = DomainError::InvalidTransition {
            from: "ended",
            action: "start",
        };
        assert_eq!(
            err.to_string(),
            "transition `start` is not allowed from state `ended`"
        );
    }
}
