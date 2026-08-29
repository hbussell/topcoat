//! Request input extractors for schema validation.
//!
//! Request input passes through three stages: [`Input`] parses and buffers
//! the unvalidated input, [`Validation<T>`] hands the schema validation
//! outcome to the handler, and [`Valid<T>`] rejects invalid submissions
//! before the handler runs. All three read the query string for `GET` and
//! `HEAD` requests, and an `application/x-www-form-urlencoded` or
//! `application/json` body otherwise.
//!
//! Only schema failures reach a handler: a malformed body, an unsupported
//! media type, or a body over the size limit is rejected during extraction.

use std::ops::{Deref, DerefMut};

use topcoat_core::{context::Cx, error::Result};
use topcoat_router::{
    Body, Method,
    content::{Form, Json, is_form_content_type, is_json_content_type},
    error::bad_request,
    request::{FromRequest, content_type, method},
};

use crate::{Schema, ValidationData, ValidationErrors, Value};

/// Untrusted request input that has been parsed but not yet validated.
///
/// For `GET` and `HEAD` requests the query string is parsed as URL-encoded
/// form data. For other methods the body is buffered and parsed as either
/// `application/x-www-form-urlencoded` or `application/json`, depending on
/// the `Content-Type` header.
///
/// Call [`Input::validate`] to validate this input against a [`Schema`], or
/// pass the input directly to [`Schema::validate`] for full control. The
/// [`Validation<T>`] and [`Valid<T>`] extractors run that validation
/// automatically.
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
            Err(errors) => Err(Invalid {
                errors,
                input: self,
            }),
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
///
/// Only schema validation produces an `Invalid`: a malformed body, an
/// unsupported media type, and other extraction failures are rejected before
/// a handler runs.
#[derive(Debug, Clone, PartialEq)]
pub struct Invalid {
    /// The errors reported by the schema validation.
    pub errors: ValidationErrors,
    /// The submitted input, for recovering what the user sent.
    pub input: Input,
}

/// A request input extractor that validates against a [`Schema`] and hands
/// the outcome to the handler.
///
/// The request is read like [`Input`] and validated with [`Input::validate`].
/// Instead of rejecting schema failures, the handler receives the outcome,
/// `Ok(value)` or `Err(invalid)`, and chooses the failure response. The
/// [`Invalid`] still carries the submitted input, for re-rendering the form
/// with its values.
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

/// A request input extractor that validates against a [`Schema`] and rejects
/// invalid submissions before the handler runs.
///
/// The request is read like [`Input`] and validated with [`Input::validate`];
/// on failure a `400 Bad Request` response carrying the validation messages
/// is returned. For handlers that choose their own failure response, extract
/// [`Validation<T>`] instead.
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

#[cfg(test)]
mod tests {
    use http::{Method, Request, header::CONTENT_TYPE};
    use topcoat_core::context::{Cx, CxTestBuilder};
    use topcoat_router::{
        Body,
        error::{BadRequestError, ContentTooLargeError},
        request::FromRequest,
    };
    use topcoat_validate_macro::Schema;

    use super::*;

    const FORM_CONTENT_TYPE: &str = "application/x-www-form-urlencoded";
    const JSON_CONTENT_TYPE: &str = "application/json";
    const DEFAULT_BODY_LIMIT: usize = 2 * 1024 * 1024;

    #[derive(Debug, Schema, PartialEq)]
    struct SignUp {
        #[schema(email)]
        email: String,

        #[schema(min_length = 8)]
        password: String,
    }

    #[derive(Debug, Schema)]
    struct Tags {
        tags: Vec<String>,
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
    async fn input_reads_query_string_for_get() {
        let cx = cx(
            Method::GET,
            "/signup?email=a%40b.com&password=secret123",
            None,
        );
        let input = Input::from_request(&cx, Body::empty())
            .await
            .expect("the query string is buffered");

        assert_eq!(input.get("email"), Some("a@b.com"));
        assert_eq!(input.get("password"), Some("secret123"));
    }

    #[tokio::test]
    async fn input_reads_query_string_for_head() {
        let cx = cx(
            Method::HEAD,
            "/signup?email=a%40b.com&password=secret123",
            None,
        );
        let input = Input::from_request(&cx, Body::empty())
            .await
            .expect("the query string is buffered");

        assert_eq!(input.get("email"), Some("a@b.com"));
        assert_eq!(input.get("password"), Some("secret123"));
    }

    #[tokio::test]
    async fn input_buffers_form_body() {
        let cx = cx(Method::POST, "/signup", Some(FORM_CONTENT_TYPE));
        let input = Input::from_request(&cx, Body::from("email=a%40b.com&password=secret123"))
            .await
            .expect("the form body is buffered");

        assert_eq!(input.get("email"), Some("a@b.com"));
        assert_eq!(input.get("missing"), None);
    }

    #[tokio::test]
    async fn input_buffers_json_body() {
        let cx = cx(Method::POST, "/signup", Some(JSON_CONTENT_TYPE));
        let input = Input::from_request(
            &cx,
            Body::from(r#"{"email":"a@b.com","password":"secret123"}"#),
        )
        .await
        .expect("the JSON body is buffered");

        assert_eq!(input.get("email"), Some("a@b.com"));
        assert_eq!(input.get("missing"), None);
    }

    #[tokio::test]
    async fn input_accepts_json_media_type_variants() {
        for content_type in [
            "application/vnd.api+json",
            "application/ld+json; charset=utf-8",
            "APPLICATION/JSON",
        ] {
            let cx = cx(Method::POST, "/signup", Some(content_type));
            let input = Input::from_request(
                &cx,
                Body::from(r#"{"email":"a@b.com","password":"secret123"}"#),
            )
            .await
            .expect("a JSON media type the router accepts is buffered");

            assert_eq!(input.get("email"), Some("a@b.com"));
        }
    }

    #[tokio::test]
    async fn input_accepts_form_media_type_variants() {
        for content_type in [
            "application/x-www-form-urlencoded; charset=utf-8",
            "APPLICATION/X-WWW-FORM-URLENCODED",
        ] {
            let cx = cx(Method::POST, "/signup", Some(content_type));
            let input = Input::from_request(&cx, Body::from("email=a%40b.com&password=secret123"))
                .await
                .expect("a form media type the router accepts is buffered");

            assert_eq!(input.get("email"), Some("a@b.com"));
        }
    }

    #[tokio::test]
    async fn input_rejects_missing_or_unsupported_content_types() {
        for content_type in [None, Some("text/plain")] {
            let cx = cx(Method::POST, "/signup", content_type);
            let error = Input::from_request(&cx, Body::from("email=a%40b.com&password=secret123"))
                .await
                .expect_err("a missing or unsupported content type is rejected");

            assert!(error.downcast_ref::<BadRequestError>().is_some());
        }
    }

    #[tokio::test]
    async fn input_rejects_unsupported_content_type_before_buffering() {
        let cx = cx(Method::POST, "/signup", Some("text/plain"));
        let body = vec![b'x'; DEFAULT_BODY_LIMIT + 1];
        let error = Input::from_request(&cx, Body::from(body))
            .await
            .expect_err("the content type is checked before the body is read");

        assert!(error.downcast_ref::<BadRequestError>().is_some());
        assert!(error.downcast_ref::<ContentTooLargeError>().is_none());
    }

    #[tokio::test]
    async fn input_rejects_trailing_json_data() {
        let cx = cx(Method::POST, "/signup", Some(JSON_CONTENT_TYPE));
        let error = Input::from_request(
            &cx,
            Body::from(r#"{"email":"a@b.com","password":"secret123"} trailing"#),
        )
        .await
        .expect_err("trailing data after the JSON value is rejected");

        assert!(error.downcast_ref::<BadRequestError>().is_some());
    }

    #[tokio::test]
    async fn input_validate_returns_the_typed_value() {
        let cx = cx(Method::POST, "/signup", Some(JSON_CONTENT_TYPE));
        let input = Input::from_request(
            &cx,
            Body::from(r#"{"email":"a@b.com","password":"secret123"}"#),
        )
        .await
        .expect("a JSON body is buffered");

        let form = input
            .validate::<SignUp>()
            .expect("a valid body validates against the schema");

        assert_eq!(form.email, "a@b.com");
        assert_eq!(form.password, "secret123");
    }

    #[tokio::test]
    async fn input_validate_returns_invalid_with_input() {
        let cx = cx(Method::POST, "/signup", Some(FORM_CONTENT_TYPE));
        let input = Input::from_request(&cx, Body::from("email=not-an-email&password=short"))
            .await
            .expect("invalid input is buffered");

        let invalid = input.validate::<SignUp>().expect_err("validation fails");
        assert_eq!(invalid.errors.get("email").unwrap().code(), "email");
        assert_eq!(invalid.input.get("email"), Some("not-an-email"));
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
    async fn validation_reaches_handler_with_the_value() {
        let cx = cx(
            Method::GET,
            "/signup?email=a%40b.com&password=secret123",
            None,
        );
        let Validation(result) = Validation::<SignUp>::from_request(&cx, Body::empty())
            .await
            .expect("a valid query string reaches the handler");

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

        assert!(error.downcast_ref::<BadRequestError>().is_some());
    }

    #[tokio::test]
    async fn valid_accepts_a_valid_form_submission() {
        let cx = cx(Method::POST, "/signup", Some(FORM_CONTENT_TYPE));
        let Valid(form) =
            Valid::<SignUp>::from_request(&cx, Body::from("email=a%40b.com&password=secret123"))
                .await
                .expect("a valid submission reaches the handler");

        assert_eq!(form.email, "a@b.com");
        assert_eq!(form.password, "secret123");
    }

    #[tokio::test]
    async fn valid_maps_schema_failures_to_bad_request() {
        let cx = cx(Method::POST, "/signup", Some(FORM_CONTENT_TYPE));
        let error =
            Valid::<SignUp>::from_request(&cx, Body::from("email=not-an-email&password=short"))
                .await
                .expect_err("an invalid submission is rejected");

        let bad_request = error.downcast_ref::<BadRequestError>();
        assert!(bad_request.is_some());
        let description = bad_request.unwrap().description();
        assert!(description.contains("email"));
        assert!(description.contains("password"));
    }

    #[tokio::test]
    async fn valid_collects_repeated_form_fields_into_lists() {
        let cx = cx(Method::POST, "/tags", Some(FORM_CONTENT_TYPE));
        let Valid(form) = Valid::<Tags>::from_request(&cx, Body::from("tags=a&tags=b"))
            .await
            .expect("a form with repeated fields reaches the handler");

        assert_eq!(form.tags, ["a", "b"]);
    }

    #[tokio::test]
    async fn valid_passes_through_content_too_large_errors() {
        let cx = cx(Method::POST, "/signup", Some(FORM_CONTENT_TYPE));
        let body = vec![b'x'; DEFAULT_BODY_LIMIT + 1];
        let error = Valid::<SignUp>::from_request(&cx, Body::from(body))
            .await
            .expect_err("an oversized body is rejected");

        assert!(error.downcast_ref::<ContentTooLargeError>().is_some());
    }
}
