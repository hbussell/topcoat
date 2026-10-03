use std::fmt;

use http::{StatusCode, header::CONTENT_TYPE};
use serde::Serialize;
use topcoat_core::{context::Cx, error::Result};

use crate::{
    Body,
    response::{IntoResponse, Response},
    validation::validated::Validate,
};

/// A single validation failure for a field.
///
/// Stores the field path, a human-readable message, and an optional rule code.
/// Field paths use dot notation for nesting (`address.zip`) and bracket indices
/// for lists (`items[0].name`). There is currently no way to escape `.` or `[`
/// inside a field name, so such characters are always treated as path
/// separators when the path is converted to a JSON Pointer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationError {
    field: String,
    message: String,
    code: Option<String>,
}

impl ValidationError {
    /// Builds a validation error for `field` with `message` and no code.
    #[must_use]
    pub fn new(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            message: message.into(),
            code: None,
        }
    }

    /// Builds a validation error for `field` with `code` and `message`.
    #[must_use]
    pub fn with_code(
        field: impl Into<String>,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            field: field.into(),
            message: message.into(),
            code: Some(code.into()),
        }
    }

    /// Returns the field path of the error.
    #[must_use]
    pub fn field(&self) -> &str {
        &self.field
    }

    /// Returns the human-readable message of the error.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Returns the rule code, if one was provided.
    #[must_use]
    pub fn code(&self) -> Option<&str> {
        self.code.as_deref()
    }
}

/// A collection of validation failures.
///
/// Build errors with `add`, `check`, and nested validation helpers, then call
/// [`into_result`](Self::into_result) to finish. Handlers can catch the error
/// and render a view, or let it fall back to a `422 Unprocessable Entity`
/// response with RFC 9457 problem details.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ValidationErrors(Vec<ValidationError>);

impl ValidationErrors {
    /// Builds an empty error collection.
    #[must_use]
    pub fn new() -> Self {
        Self(Vec::new())
    }

    /// Returns whether there are no errors.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Returns the number of errors.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns the contained errors.
    #[must_use]
    pub fn errors(&self) -> &[ValidationError] {
        &self.0
    }

    /// Returns the contained errors, consuming `self`.
    #[must_use]
    pub fn into_inner(self) -> Vec<ValidationError> {
        self.0
    }

    /// Adds a message for `field`.
    pub fn add(&mut self, field: impl Into<String>, message: impl Into<String>) {
        self.0.push(ValidationError::new(field, message));
    }

    /// Adds a message for `field` tagged with `code`.
    pub fn add_with_code(
        &mut self,
        field: impl Into<String>,
        code: impl Into<String>,
        message: impl Into<String>,
    ) {
        self.0
            .push(ValidationError::with_code(field, code, message));
    }

    /// Adds a pre-built [`ValidationError`].
    pub fn add_error(&mut self, error: ValidationError) {
        self.0.push(error);
    }

    /// Adds a message for `field` when `condition` is `false`.
    pub fn check(&mut self, field: impl Into<String>, condition: bool, message: impl Into<String>) {
        if !condition {
            self.add(field, message);
        }
    }

    /// Adds a coded message for `field` when `condition` is `false`.
    pub fn check_with_code(
        &mut self,
        field: impl Into<String>,
        condition: bool,
        code: impl Into<String>,
        message: impl Into<String>,
    ) {
        if !condition {
            self.add_with_code(field, code, message);
        }
    }

    /// Runs [`Validate`] on `value` and prefixes each inner error with `field`.
    ///
    /// Inner paths compose: validating `address` produces `address.zip`, and
    /// validating `items` with `nest_each` produces `items[0].name`.
    pub async fn nest<V>(&mut self, field: impl Into<String>, value: &V, cx: &Cx)
    where
        V: Validate + Sync,
    {
        let field = field.into();
        if let Err(errors) = value.validate(cx).await {
            self.merge_nested(errors, |inner_field| {
                if inner_field.is_empty() {
                    field.clone()
                } else {
                    format!("{field}.{inner_field}")
                }
            });
        }
    }

    /// Runs [`Validate`] on every item in `values` and prefixes each inner error
    /// with `field[index]`.
    pub async fn nest_each<V>(&mut self, field: impl Into<String>, values: &[V], cx: &Cx)
    where
        V: Validate + Sync,
    {
        let field = field.into();
        for (index, value) in values.iter().enumerate() {
            if let Err(errors) = value.validate(cx).await {
                self.merge_nested(errors, |inner_field| {
                    if inner_field.is_empty() {
                        format!("{field}[{index}]")
                    } else {
                        format!("{field}[{index}].{inner_field}")
                    }
                });
            }
        }
    }

    /// Adds nested errors after rewriting their field paths with `prefix`.
    fn merge_nested<F>(&mut self, errors: ValidationErrors, prefix: F)
    where
        F: Fn(&str) -> String,
    {
        for error in errors.0 {
            self.0.push(ValidationError {
                field: prefix(&error.field),
                message: error.message,
                code: error.code,
            });
        }
    }

    /// Returns `Ok(())` when empty, otherwise `Err(self)`.
    ///
    /// # Errors
    ///
    /// Returns `Err(self)` when any validation failure has been collected.
    pub fn into_result(self) -> Result<(), Self> {
        if self.is_empty() { Ok(()) } else { Err(self) }
    }

    /// Returns whether at least one error is for `field`.
    #[must_use]
    pub fn has_field(&self, field: &str) -> bool {
        self.0.iter().any(|error| error.field == field)
    }

    /// Returns all errors for `field`.
    pub fn field<'a, 'b>(&'a self, field: &'b str) -> impl Iterator<Item = &'a ValidationError> + 'b
    where
        'a: 'b,
    {
        self.0.iter().filter(move |error| error.field == field)
    }

    /// Returns every message for `field`.
    pub fn messages<'a, 'b>(&'a self, field: &'b str) -> impl Iterator<Item = &'a str> + 'b
    where
        'a: 'b,
    {
        self.field(field).map(|error| error.message.as_str())
    }

    /// Returns the first message for `field`, if any.
    #[must_use]
    pub fn first_message<'a>(&'a self, field: &str) -> Option<&'a str> {
        self.messages(field).next()
    }
}

impl fmt::Display for ValidationErrors {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "validation failed with {} error(s)", self.len())
    }
}

impl std::error::Error for ValidationErrors {}

impl IntoResponse for ValidationErrors {
    fn into_response(self, _cx: &Cx) -> Result<Response> {
        let problem = Problem::from(&self);
        let body = serde_json::to_vec(&problem)?;

        Ok(Response::builder()
            .status(StatusCode::UNPROCESSABLE_ENTITY)
            .header(CONTENT_TYPE, PROBLEM_JSON)
            .body(Body::from(body))?)
    }
}

/// Media type of RFC 9457 problem details.
const PROBLEM_JSON: &str = "application/problem+json";

/// RFC 9457 problem details produced by [`ValidationErrors`] responses.
///
/// The `errors` member is an extension that lists each field failure.
#[derive(Serialize)]
struct Problem<'a> {
    r#type: &'static str,
    title: &'static str,
    status: u16,
    detail: String,
    errors: Vec<ProblemError<'a>>,
}

impl<'a> From<&'a ValidationErrors> for Problem<'a> {
    fn from(errors: &'a ValidationErrors) -> Self {
        Self {
            r#type: "about:blank",
            title: "Unprocessable Content",
            status: StatusCode::UNPROCESSABLE_ENTITY.as_u16(),
            detail: errors.to_string(),
            errors: errors.0.iter().map(ProblemError::from).collect(),
        }
    }
}

/// A single field failure in the `errors` extension member.
#[derive(Serialize)]
struct ProblemError<'a> {
    /// JSON Pointer (RFC 6901) to the invalid value in the request.
    pointer: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<&'a str>,
    detail: &'a str,
}

impl<'a> From<&'a ValidationError> for ProblemError<'a> {
    fn from(error: &'a ValidationError) -> Self {
        Self {
            pointer: field_to_json_pointer(&error.field),
            code: error.code.as_deref(),
            detail: &error.message,
        }
    }
}

/// Converts a dot/bracket field path into a JSON Pointer.
///
/// `address.zip` becomes `/address/zip`, `items[0].name` becomes
/// `/items/0/name`, and tildes and slashes in field names are escaped.
///
/// # Limitations
///
/// There is no way to escape `.` or `[` inside a field name, so a field named
/// `meta.data` is always split into `meta` and `data`. Only `~` and `/` are
/// escaped for JSON Pointer compatibility.
fn field_to_json_pointer(field: &str) -> String {
    let segments = split_field_path(field);
    if segments.is_empty() {
        return String::new();
    }

    let mut pointer = String::new();
    for segment in segments {
        pointer.push('/');
        for character in segment.chars() {
            match character {
                '~' => pointer.push_str("~0"),
                '/' => pointer.push_str("~1"),
                _ => pointer.push(character),
            }
        }
    }
    pointer
}

/// Splits a field path such as `items[0].name` into its segments.
fn split_field_path(field: &str) -> Vec<String> {
    let mut segments = Vec::new();
    let mut current = String::new();
    let mut in_bracket = false;

    for character in field.chars() {
        match character {
            '.' if !in_bracket => {
                if !current.is_empty() {
                    segments.push(std::mem::take(&mut current));
                }
            }
            '[' if !in_bracket => {
                if !current.is_empty() {
                    segments.push(std::mem::take(&mut current));
                }
                in_bracket = true;
            }
            ']' if in_bracket => {
                if !current.is_empty() {
                    segments.push(std::mem::take(&mut current));
                }
                in_bracket = false;
            }
            _ => current.push(character),
        }
    }

    if !current.is_empty() {
        segments.push(current);
    }

    segments
}

#[cfg(test)]
mod tests {
    use http::{StatusCode, header::CONTENT_TYPE};
    use topcoat_core::context::Cx;

    use super::*;
    use crate::to_bytes;

    #[test]
    fn new_error_stores_field_and_message() {
        let error = ValidationError::new("name", "name is required");

        assert_eq!(error.field(), "name");
        assert_eq!(error.message(), "name is required");
        assert!(error.code().is_none());
    }

    #[test]
    fn with_code_stores_code() {
        let error = ValidationError::with_code("email", "invalid_email", "email is invalid");

        assert_eq!(error.code(), Some("invalid_email"));
    }

    #[test]
    fn check_adds_error_on_failure() {
        let mut errors = ValidationErrors::new();
        errors.check("name", false, "name is required");

        assert!(errors.has_field("name"));
        assert_eq!(errors.len(), 1);
    }

    #[test]
    fn check_skips_error_on_success() {
        let mut errors = ValidationErrors::new();
        errors.check("name", true, "name is required");

        assert!(errors.is_empty());
    }

    #[test]
    fn check_with_code_adds_coded_error() {
        let mut errors = ValidationErrors::new();
        errors.check_with_code("email", false, "invalid_email", "email is invalid");

        let error = errors.field("email").next().unwrap();
        assert_eq!(error.code(), Some("invalid_email"));
    }

    #[test]
    fn into_result_ok_when_empty() {
        let errors = ValidationErrors::new();
        assert!(errors.into_result().is_ok());
    }

    #[test]
    fn into_result_err_when_not_empty() {
        let mut errors = ValidationErrors::new();
        errors.add("name", "name is required");
        assert!(errors.into_result().is_err());
    }

    #[test]
    fn view_helpers_match_exact_field() {
        let mut errors = ValidationErrors::new();
        errors.add("name", "name is required");
        errors.add("email", "email is invalid");
        errors.add("email", "email is too short");

        assert!(errors.has_field("email"));
        assert!(!errors.has_field("missing"));
        assert_eq!(errors.first_message("name"), Some("name is required"));
        assert_eq!(errors.messages("email").count(), 2);
    }

    #[test]
    fn split_field_path_handles_dots_and_brackets() {
        assert_eq!(
            split_field_path("address.zip"),
            vec!["address".to_owned(), "zip".to_owned()]
        );
        assert_eq!(
            split_field_path("items[0].name"),
            vec!["items".to_owned(), "0".to_owned(), "name".to_owned()]
        );
        assert_eq!(
            split_field_path("order.items[2].address.city"),
            vec![
                "order".to_owned(),
                "items".to_owned(),
                "2".to_owned(),
                "address".to_owned(),
                "city".to_owned(),
            ]
        );
    }

    #[test]
    fn field_to_json_pointer_escapes_special_characters() {
        assert_eq!(field_to_json_pointer("address.zip"), "/address/zip");
        assert_eq!(field_to_json_pointer("items[0].name"), "/items/0/name");
        assert_eq!(field_to_json_pointer("a/b.c~d"), "/a~1b/c~0d");
    }

    #[test]
    fn problem_matches_rfc_9457() {
        let mut errors = ValidationErrors::new();
        errors.add_with_code("address.zip", "required", "zip is required");

        let json = serde_json::to_value(Problem::from(&errors)).unwrap();

        assert_eq!(json["type"], "about:blank");
        assert_eq!(json["title"], "Unprocessable Content");
        assert_eq!(json["status"], 422);
        assert_eq!(json["detail"], "validation failed with 1 error(s)");
        assert_eq!(json["errors"].as_array().unwrap().len(), 1);
        assert_eq!(json["errors"][0]["pointer"], "/address/zip");
        assert_eq!(json["errors"][0]["code"], "required");
        assert_eq!(json["errors"][0]["detail"], "zip is required");
    }

    #[test]
    fn problem_error_omits_missing_code() {
        let mut errors = ValidationErrors::new();
        errors.add("name", "name is required");

        let json = serde_json::to_value(Problem::from(&errors)).unwrap();

        assert!(json["errors"][0].get("code").is_none());
    }

    #[tokio::test]
    async fn into_response_renders_422_problem_details() {
        let mut errors = ValidationErrors::new();
        errors.add_with_code("address.zip", "required", "zip is required");

        let response = errors.into_response(&Cx::default()).unwrap();
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(
            response.headers().get(CONTENT_TYPE).unwrap(),
            "application/problem+json"
        );

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["status"], 422);
        assert_eq!(json["errors"][0]["pointer"], "/address/zip");
        assert_eq!(json["errors"][0]["code"], "required");
        assert_eq!(json["errors"][0]["detail"], "zip is required");
    }
}
