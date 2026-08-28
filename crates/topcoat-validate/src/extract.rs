//! Router extractors for request input: [`Valid<T>`] validates up front,
//! [`Validation<T>`] hands the validation result to the handler, and [`Input`]
//! buffers the raw input for manual validation.

use std::ops::{Deref, DerefMut};

use topcoat_core::{context::Cx, error::Result};
use topcoat_router::{
    Body, Method,
    content::{Form, Json, is_form_content_type, is_json_content_type},
    error::bad_request,
    request::{FromRequest, content_type, method},
};

use crate::{Schema, ValidationData, ValidationErrors, Value};

/// A request input extractor that validates against a [`Schema`] and rejects
/// invalid submissions before the handler runs.
///
/// For `GET` and `HEAD` requests the query string is parsed as URL-encoded form
/// data. For other methods the body is buffered and parsed as either
/// `application/x-www-form-urlencoded` or `application/json`, depending on the
/// `Content-Type` header. The parsed input is then validated with
/// [`Input::validate`]; on failure a `400 Bad Request` response carrying the
/// validation messages is returned.
///
/// For handlers that choose their own failure response, extract [`Input`] or
/// [`Validation<T>`] instead and pattern-match on the validation result in the
/// handler body.
#[derive(Debug, Clone, Copy, Default)]
#[must_use]
pub struct Valid<T>(pub T);

impl<T> From<T> for Valid<T> {
    fn from(value: T) -> Self {
        Self(value)
    }
}

impl<T> Deref for Valid<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> DerefMut for Valid<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<T> FromRequest for Valid<T>
where
    T: Schema,
{
    async fn from_request(cx: &Cx, body: Body) -> Result<Self> {
        match Input::from_request(cx, body).await?.validate::<T>() {
            Ok(value) => Ok(Valid(value)),
            Err(invalid) => Err(bad_request(invalid.errors.to_string()).into()),
        }
    }
}

/// A request input extractor that validates against a [`Schema`] and hands
/// the handler the outcome to pattern-match on.
///
/// Where [`Valid<T>`] rejects invalid submissions before the handler runs,
/// `Validation<T>` defers the decision: the handler receives
/// `Result<T, Invalid>` and chooses the failure response. On failure the
/// [`Invalid`] still holds the submitted input, for re-rendering the form.
///
/// Requests are read exactly like [`Valid<T>`]: the query string for `GET` and
/// `HEAD`, an `application/x-www-form-urlencoded` or `application/json` body
/// otherwise.
#[derive(Debug, Clone)]
#[must_use]
pub struct Validation<T>(pub std::result::Result<T, Invalid>);

impl<T> FromRequest for Validation<T>
where
    T: Schema,
{
    async fn from_request(cx: &Cx, body: Body) -> Result<Self> {
        let input = Input::from_request(cx, body).await?;
        Ok(Self(input.validate::<T>()))
    }
}

/// Untrusted request input that has been parsed but not yet validated.
///
/// `Input` buffers the same data as [`Valid<T>`]: the query string for `GET`
/// and `HEAD`, an `application/x-www-form-urlencoded` or `application/json`
/// body otherwise. Call [`Input::validate`] to attempt schema validation, or
/// pass the input directly to [`Schema::validate`] for full control.
#[derive(Debug, Clone, PartialEq)]
pub struct Input(InputSource);

#[derive(Debug, Clone, PartialEq)]
enum InputSource {
    Pairs(Vec<(String, String)>),
    Json(serde_json::Value),
}

impl Input {
    /// Validate this input against a [`Schema`].
    ///
    /// On failure the original input is moved into [`Invalid`] so the handler
    /// can recover submitted values for redisplay.
    ///
    /// # Errors
    ///
    /// Returns [`Invalid`] when the input does not satisfy the schema.
    pub fn validate<T: Schema>(self) -> std::result::Result<T, Invalid> {
        match T::validate(&self) {
            Ok(value) => Ok(value),
            Err(errors) => Err(Invalid { errors, input: self }),
        }
    }

    /// The string submitted under `name`, if the value is a string. Handy for
    /// re-rendering a form with the submitted values.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&str> {
        match &self.0 {
            InputSource::Pairs(pairs) => pairs
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.as_str()),
            InputSource::Json(json) => json.get(name)?.as_str(),
        }
    }
}

impl FromRequest for Input {
    async fn from_request(cx: &Cx, body: Body) -> Result<Self> {
        if matches!(method(cx), &Method::GET | &Method::HEAD) {
            let Form(pairs) = Form::<Vec<(String, String)>>::from_request(cx, body).await?;
            return Ok(Self(InputSource::Pairs(pairs)));
        }

        match content_type(cx) {
            Some(content_type) if is_form_content_type(content_type) => {
                let Form(pairs) = Form::<Vec<(String, String)>>::from_request(cx, body).await?;
                Ok(Self(InputSource::Pairs(pairs)))
            }
            Some(content_type) if is_json_content_type(content_type) => {
                let Json(value) = Json::<serde_json::Value>::from_request(cx, body).await?;
                Ok(Self(InputSource::Json(value)))
            }
            _ => Err(bad_request(
                "expected request with `Content-Type: application/x-www-form-urlencoded` or `application/json`",
            )
            .into()),
        }
    }
}

impl ValidationData for Input {
    fn field(&self, name: &str) -> Option<Value> {
        match &self.0 {
            InputSource::Pairs(pairs) => pairs.field(name),
            InputSource::Json(json) => json.field(name),
        }
    }

    fn nested(&self, name: &str) -> Option<Value> {
        match &self.0 {
            InputSource::Pairs(pairs) => pairs.nested(name),
            InputSource::Json(json) => json.nested(name),
        }
    }
}

/// A schema validation failure, carrying both the errors and the original
/// input so submitted values can be recovered.
#[derive(Debug, Clone, PartialEq)]
pub struct Invalid {
    pub errors: ValidationErrors,
    pub input: Input,
}

#[cfg(test)]
mod tests {
    use http::{Method, Request, header::CONTENT_TYPE};
    use topcoat_core::context::{Cx, CxTestBuilder};
    use topcoat_router::{Body, request::FromRequest};
    use topcoat_validate_macro::Schema;

    use super::*;

    const FORM_CONTENT_TYPE: &str = "application/x-www-form-urlencoded";
    const JSON_CONTENT_TYPE: &str = "application/json";

    #[derive(Debug, Schema, PartialEq)]
    struct SignUp {
        #[schema(email)]
        email: String,

        #[schema(min_length = 8)]
        password: String,
    }

    fn cx(method: Method, uri: &str, content_type: Option<&str>) -> Cx {
        let mut builder = Request::builder().method(method).uri(uri);
        if let Some(content_type) = content_type {
            builder = builder.header(CONTENT_TYPE, content_type);
        }

        let (parts, ()) = builder.body(()).expect("request should build").into_parts();
        CxTestBuilder::new().request_context(parts).build()
    }

    #[tokio::test]
    async fn from_request_parses_form_body() {
        let cx = cx(Method::POST, "/signup", Some(FORM_CONTENT_TYPE));
        let Valid(form) =
            Valid::<SignUp>::from_request(&cx, Body::from("email=a%40b.com&password=secret123"))
                .await
                .expect("valid form body");

        assert_eq!(form.email, "a@b.com");
        assert_eq!(form.password, "secret123");
    }

    #[tokio::test]
    async fn from_request_parses_json_body() {
        let cx = cx(Method::POST, "/signup", Some(JSON_CONTENT_TYPE));
        let Valid(form) = Valid::<SignUp>::from_request(
            &cx,
            Body::from(r#"{"email":"a@b.com","password":"secret123"}"#),
        )
        .await
        .expect("valid json body");

        assert_eq!(form.email, "a@b.com");
        assert_eq!(form.password, "secret123");
    }

    #[tokio::test]
    async fn from_request_parses_query_string_for_get() {
        let cx = cx(
            Method::GET,
            "/signup?email=a%40b.com&password=secret123",
            None,
        );
        let Valid(form) = Valid::<SignUp>::from_request(&cx, Body::empty())
            .await
            .expect("valid query string");

        assert_eq!(form.email, "a@b.com");
        assert_eq!(form.password, "secret123");
    }

    #[tokio::test]
    async fn from_request_rejects_missing_content_type() {
        let cx = cx(Method::POST, "/signup", None);
        let error =
            Valid::<SignUp>::from_request(&cx, Body::from("email=a@b.com&password=secret123"))
                .await
                .expect_err("missing content type is rejected");

        assert!(
            error
                .downcast_ref::<topcoat_router::error::BadRequestError>()
                .is_some()
        );
    }

    #[tokio::test]
    async fn from_request_returns_validation_errors() {
        let cx = cx(Method::POST, "/signup", Some(FORM_CONTENT_TYPE));
        let error =
            Valid::<SignUp>::from_request(&cx, Body::from("email=not-an-email&password=short"))
                .await
                .expect_err("invalid form body is rejected");

        let bad_request = error.downcast_ref::<topcoat_router::error::BadRequestError>();
        assert!(bad_request.is_some());
        let description = bad_request.unwrap().description();
        assert!(description.contains("email"));
        assert!(description.contains("password"));
    }

    #[tokio::test]
    async fn from_request_passes_through_content_too_large() {
        let cx = cx(Method::POST, "/signup", Some(FORM_CONTENT_TYPE));
        let body = vec![b'x'; 2 * 1024 * 1024 + 1];
        let error = Valid::<SignUp>::from_request(&cx, Body::from(body))
            .await
            .expect_err("oversized body is rejected");

        assert!(
            error
                .downcast_ref::<topcoat_router::error::ContentTooLargeError>()
                .is_some()
        );
    }

    #[tokio::test]
    async fn validation_reaches_handler_with_the_value() {
        let cx = cx(Method::POST, "/signup", Some(FORM_CONTENT_TYPE));
        let Validation(result) = Validation::<SignUp>::from_request(
            &cx,
            Body::from("email=a%40b.com&password=secret123"),
        )
        .await
        .expect("valid input reaches the handler");

        let form = result.expect("the value is present");
        assert_eq!(form.email, "a@b.com");
        assert_eq!(form.password, "secret123");
    }

    #[tokio::test]
    async fn validation_reaches_handler_with_errors() {
        let cx = cx(Method::POST, "/signup", Some(FORM_CONTENT_TYPE));
        let Validation(result) = Validation::<SignUp>::from_request(
            &cx,
            Body::from("email=not-an-email&password=short"),
        )
        .await
        .expect("invalid input still reaches the handler");

        let invalid = result.expect_err("the schema failure is visible");
        assert_eq!(invalid.errors.get("email").unwrap().code(), "email");
        assert_eq!(invalid.errors.get("password").unwrap().code(), "min_length");
        assert_eq!(invalid.input.get("email"), Some("not-an-email"));
    }

    #[tokio::test]
    async fn validation_rejects_malformed_json_before_the_handler() {
        let cx = cx(Method::POST, "/signup", Some(JSON_CONTENT_TYPE));
        let error = Validation::<SignUp>::from_request(&cx, Body::from("{"))
            .await
            .expect_err("malformed JSON is rejected");

        assert!(
            error
                .downcast_ref::<topcoat_router::error::BadRequestError>()
                .is_some()
        );
    }

    #[tokio::test]
    async fn input_buffers_form_body_for_manual_validation() {
        let cx = cx(Method::POST, "/signup", Some(FORM_CONTENT_TYPE));
        let input = Input::from_request(&cx, Body::from("email=a%40b.com&password=secret123"))
            .await
            .expect("form body is buffered");

        assert!(SignUp::validate(&input).is_ok());
        assert_eq!(input.get("email"), Some("a@b.com"));
        assert_eq!(input.get("missing"), None);
    }

    #[tokio::test]
    async fn input_buffers_json_body_for_manual_validation() {
        let cx = cx(Method::POST, "/signup", Some(JSON_CONTENT_TYPE));
        let input = Input::from_request(
            &cx,
            Body::from(r#"{"email":"a@b.com","password":"secret123"}"#),
        )
        .await
        .expect("json body is buffered");

        assert!(SignUp::validate(&input).is_ok());
        assert_eq!(input.get("email"), Some("a@b.com"));
    }

    #[tokio::test]
    async fn input_reads_query_string_for_get() {
        let cx = cx(
            Method::GET,
            "/signup?email=a%40b.com&password=secret123",
            None,
        );
        let input = Input::from_request(&cx, Body::empty())
            .await
            .expect("query string is buffered");

        assert!(SignUp::validate(&input).is_ok());
    }

    #[tokio::test]
    async fn input_reports_validation_errors_in_the_handler() {
        let cx = cx(Method::POST, "/signup", Some(FORM_CONTENT_TYPE));
        let input = Input::from_request(&cx, Body::from("email=not-an-email&password=short"))
            .await
            .expect("invalid input is still buffered");

        let errors = SignUp::validate(&input).expect_err("validation fails in the handler");
        assert_eq!(errors.get("email").unwrap().code(), "email");
        assert_eq!(errors.get("password").unwrap().code(), "min_length");
    }

    #[tokio::test]
    async fn input_validate_returns_invalid_with_input() {
        let cx = cx(Method::POST, "/signup", Some(FORM_CONTENT_TYPE));
        let input = Input::from_request(&cx, Body::from("email=not-an-email&password=short"))
            .await
            .expect("invalid input is buffered");

        let invalid = input
            .validate::<SignUp>()
            .expect_err("validation fails");
        assert_eq!(invalid.errors.get("email").unwrap().code(), "email");
        assert_eq!(invalid.input.get("email"), Some("not-an-email"));
    }
}
