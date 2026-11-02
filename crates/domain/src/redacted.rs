//! Wrapper that keeps sensitive values out of logs and debug output (FR-051, R13).

use std::fmt;

/// Holds session content or secrets; `Debug` and `Display` print `[redacted]`.
///
/// The value is only reachable through [`Redacted::expose`] or [`Redacted::into_inner`], which
/// makes every read explicit at the call site.
#[derive(Clone, PartialEq, Eq, Default)]
pub struct Redacted<T>(T);

impl<T> Redacted<T> {
    /// Wraps a sensitive value.
    #[must_use]
    pub const fn new(value: T) -> Self {
        Self(value)
    }

    /// Borrows the sensitive value.
    #[must_use]
    pub const fn expose(&self) -> &T {
        &self.0
    }

    /// Unwraps the sensitive value.
    #[must_use]
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T> From<T> for Redacted<T> {
    fn from(value: T) -> Self {
        Self(value)
    }
}

impl<T> fmt::Debug for Redacted<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[redacted]")
    }
}

impl<T> fmt::Display for Redacted<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[redacted]")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    #[allow(dead_code)]
    struct Holder {
        title: Redacted<String>,
    }

    #[test]
    fn debug_and_display_hide_the_value() {
        let secret = Redacted::new(String::from("frase confidencial"));
        assert_eq!(format!("{secret:?}"), "[redacted]");
        assert_eq!(format!("{secret}"), "[redacted]");
        assert_eq!(format!("{secret:#?}"), "[redacted]");
    }

    #[test]
    fn nested_debug_output_stays_redacted() {
        let holder = Holder {
            title: Redacted::new(String::from("frase confidencial")),
        };
        let printed = format!("{holder:?}");
        assert!(!printed.contains("confidencial"));
        assert!(printed.contains("[redacted]"));
    }

    #[test]
    fn value_is_reachable_explicitly() {
        let secret = Redacted::new(vec![1u8, 2, 3]);
        assert_eq!(secret.expose(), &[1, 2, 3]);
        assert_eq!(secret.into_inner(), vec![1, 2, 3]);
    }
}
