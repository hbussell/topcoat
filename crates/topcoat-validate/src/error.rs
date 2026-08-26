//! The validation error types.

use std::{borrow::Cow, error::Error, fmt};

/// A single validation failure.
#[derive(Debug, Clone, PartialEq)]
pub struct ValidationError {
    code: &'static str,
    message: Cow<'static, str>,
}

impl ValidationError {
    /// Create a new validation error with the default `"custom"` code.
    #[must_use]
    pub fn new(message: impl Into<Cow<'static, str>>) -> Self {
        Self {
            code: "custom",
            message: message.into(),
        }
    }

    /// Create a new validation error with a specific machine-readable code.
    #[must_use]
    pub fn with_code(code: &'static str, message: impl Into<Cow<'static, str>>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    /// The error code, such as `"required"`, `"email"`, or `"custom"`.
    #[must_use]
    pub fn code(&self) -> &'static str {
        self.code
    }

    /// The human-readable error message.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.message.fmt(f)
    }
}

impl Error for ValidationError {}

/// A collection of validation errors, keyed by field path.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct ValidationErrors {
    errors: Vec<(String, ValidationError)>,
}

impl ValidationErrors {
    /// Create an empty error collection.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add an error for a field.
    pub fn push(&mut self, field: impl Into<String>, error: ValidationError) {
        self.errors.push((field.into(), error));
    }

    /// Return the first error for a field, if any.
    #[must_use]
    pub fn get(&self, field: &str) -> Option<&ValidationError> {
        self.errors
            .iter()
            .find(|(name, _)| name == field)
            .map(|(_, err)| err)
    }

    /// Return the number of field errors.
    #[must_use]
    pub fn len(&self) -> usize {
        self.errors.len()
    }

    /// Return true if there are no errors.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.errors.is_empty()
    }

    /// Iterate over `(field, error)` pairs.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &ValidationError)> {
        self.errors.iter().map(|(field, err)| (field.as_str(), err))
    }
}

impl fmt::Display for ValidationErrors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, (field, error)) in self.errors.iter().enumerate() {
            if i > 0 {
                f.write_str("\n")?;
            }
            write!(f, "- {field}: {error}")?;
        }
        Ok(())
    }
}

impl Error for ValidationErrors {}

impl IntoIterator for ValidationErrors {
    type Item = (String, ValidationError);
    type IntoIter = std::vec::IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.errors.into_iter()
    }
}

impl<'a> IntoIterator for &'a ValidationErrors {
    type Item = (&'a str, &'a ValidationError);
    type IntoIter = Box<dyn Iterator<Item = Self::Item> + 'a>;

    fn into_iter(self) -> Self::IntoIter {
        Box::new(self.iter())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validation_error_default_code_is_custom() {
        let err = ValidationError::new("bad");
        assert_eq!(err.code(), "custom");
        assert_eq!(err.message(), "bad");
        assert_eq!(err.to_string(), "bad");
    }

    #[test]
    fn validation_error_with_code() {
        let err = ValidationError::with_code("required", "Name is required");
        assert_eq!(err.code(), "required");
        assert_eq!(err.message(), "Name is required");
    }

    #[test]
    fn validation_errors_empty_by_default() {
        let errors = ValidationErrors::new();
        assert!(errors.is_empty());
        assert_eq!(errors.len(), 0);
        assert_eq!(errors.to_string(), "");
    }

    #[test]
    fn validation_errors_push_and_get() {
        let mut errors = ValidationErrors::new();
        errors.push("email", ValidationError::new("invalid email"));
        assert!(!errors.is_empty());
        assert_eq!(errors.len(), 1);
        assert_eq!(errors.get("email").unwrap().message(), "invalid email");
        assert!(errors.get("password").is_none());
    }

    #[test]
    fn validation_errors_display_format() {
        let mut errors = ValidationErrors::new();
        errors.push("email", ValidationError::new("invalid email"));
        errors.push("age", ValidationError::with_code("min", "too young"));
        let output = errors.to_string();
        assert_eq!(output, "- email: invalid email\n- age: too young");
    }

    #[test]
    fn validation_errors_iteration() {
        let mut errors = ValidationErrors::new();
        errors.push("a", ValidationError::new("one"));
        errors.push("b", ValidationError::new("two"));

        let collected: Vec<_> = errors
            .iter()
            .map(|(f, e)| (f.to_string(), e.message().to_string()))
            .collect();
        assert_eq!(
            collected,
            vec![
                ("a".to_string(), "one".to_string()),
                ("b".to_string(), "two".to_string())
            ]
        );

        let collected_by_value: Vec<_> = errors
            .into_iter()
            .map(|(f, e)| (f, e.message().to_string()))
            .collect();
        assert_eq!(
            collected_by_value,
            vec![
                ("a".to_string(), "one".to_string()),
                ("b".to_string(), "two".to_string())
            ]
        );
    }

    #[test]
    fn validation_errors_clone_and_eq() {
        let mut errors = ValidationErrors::new();
        errors.push("x", ValidationError::new("oops"));
        let cloned = errors.clone();
        assert_eq!(errors, cloned);
    }
}
