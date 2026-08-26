//! Router extractors for request data: [`Valid<T>`] validates up front,
//! [`Input<T>`] hands the validation result to the handler, and [`Data`]
//! buffers the raw data.

use std::ops::{Deref, DerefMut};

use topcoat_core::{context::Cx, error::Result};
use topcoat_router::{
    Body, Method,
    error::bad_request,
    request::{Bytes, FromRequest, content_type, method, uri},
};

use crate::{Schema, ValidationData, ValidationErrors, Value};

/// A request body extractor that validates the body against a [`Schema`].
///
/// For `GET` and `HEAD` requests the query string is parsed as URL-encoded form
/// data. For other methods the body is buffered and parsed as either
/// `application/x-www-form-urlencoded` or `application/json`, depending on the
/// `Content-Type` header. The parsed data is then validated with `T::validate`;
/// on failure a `400 Bad Request` response carrying the validation messages is
/// returned.
///
/// For handlers that choose their own failure response, extract [`Input<T>`]
/// instead and pattern-match on the validation result in the handler body.
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
        match T::validate(&Data::from_request(cx, body).await?) {
            Ok(value) => Ok(Valid(value)),
            Err(errors) => Err(bad_request(errors.to_string()).into()),
        }
    }
}

/// A request body extractor that validates against a [`Schema`] and hands the
/// handler the result to pattern-match on.
///
/// Where [`Valid<T>`] rejects invalid submissions before the handler runs,
/// `Input<T>` defers the decision: the handler receives
/// `Result<T, (ValidationErrors, Data)>` and chooses the failure response. On
/// failure the [`Data`] still holds the submitted values, for re-rendering the
/// form.
///
/// Requests are read exactly like [`Valid<T>`]: the query string for `GET` and
/// `HEAD`, an `application/x-www-form-urlencoded` or `application/json` body
/// otherwise.
#[derive(Debug)]
pub struct Input<T>(pub Result<T, (ValidationErrors, Data)>);

impl<T> FromRequest for Input<T>
where
    T: Schema,
{
    async fn from_request(cx: &Cx, body: Body) -> Result<Self> {
        let data = Data::from_request(cx, body).await?;
        let input = match T::validate(&data) {
            Ok(value) => Ok(value),
            Err(errors) => Err((errors, data)),
        };
        Ok(Self(input))
    }
}

/// Request data buffered for validation by hand.
///
/// Where [`Valid<T>`] validates before the handler runs, `Data` only reads the
/// request: the query string for `GET` and `HEAD`, an
/// `application/x-www-form-urlencoded` or `application/json` body otherwise.
/// The handler can then pattern-match on the result of [`Schema::validate`]
/// and choose its own failure response. [`Input<T>`] packages exactly this as
/// a typed extractor.
#[derive(Debug, Clone)]
pub struct Data(DataSource);

#[derive(Debug, Clone)]
enum DataSource {
    Pairs(Vec<(String, String)>),
    Json(serde_json::Value),
}

impl Data {
    /// The string submitted under `name`, if the value is a string. Handy for
    /// re-rendering a form with the submitted values.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&str> {
        match &self.0 {
            DataSource::Pairs(pairs) => pairs
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.as_str()),
            DataSource::Json(json) => json.get(name)?.as_str(),
        }
    }
}

impl FromRequest for Data {
    async fn from_request(cx: &Cx, body: Body) -> Result<Self> {
        if matches!(method(cx), &Method::GET | &Method::HEAD) {
            let query = uri(cx).query().unwrap_or_default();
            let pairs = form_urlencoded::parse(query.as_bytes())
                .map(|(key, value)| (key.into_owned(), value.into_owned()))
                .collect();
            return Ok(Self(DataSource::Pairs(pairs)));
        }

        let bytes = Bytes::from_request(cx, body).await?;

        if json_content_type(content_type(cx)) {
            let value = serde_json::from_slice::<serde_json::Value>(&bytes)
                .map_err(|error| bad_request(format!("invalid JSON: {error}")))?;
            return Ok(Self(DataSource::Json(value)));
        }

        if form_content_type(content_type(cx)) {
            let pairs = form_urlencoded::parse(&bytes)
                .map(|(key, value)| (key.into_owned(), value.into_owned()))
                .collect();
            return Ok(Self(DataSource::Pairs(pairs)));
        }

        Err(bad_request(
            "expected request with `Content-Type: application/x-www-form-urlencoded` or `application/json`",
        )
        .into())
    }
}

impl ValidationData for Data {
    fn field(&self, name: &str) -> Option<Value> {
        match &self.0 {
            DataSource::Pairs(pairs) => pairs.field(name),
            DataSource::Json(json) => json.field(name),
        }
    }

    fn nested(&self, name: &str) -> Option<Value> {
        match &self.0 {
            DataSource::Pairs(pairs) => pairs.nested(name),
            DataSource::Json(json) => json.nested(name),
        }
    }
}

fn form_content_type(content_type: Option<&str>) -> bool {
    let Some(content_type) = content_type else {
        return false;
    };

    content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .eq_ignore_ascii_case("application/x-www-form-urlencoded")
}

fn json_content_type(content_type: Option<&str>) -> bool {
    let Some(content_type) = content_type else {
        return false;
    };

    content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .eq_ignore_ascii_case("application/json")
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
    async fn data_buffers_form_body_for_manual_validation() {
        let cx = cx(Method::POST, "/signup", Some(FORM_CONTENT_TYPE));
        let data = Data::from_request(&cx, Body::from("email=a%40b.com&password=secret123"))
            .await
            .expect("form body is buffered");

        assert!(SignUp::validate(&data).is_ok());
        assert_eq!(data.get("email"), Some("a@b.com"));
        assert_eq!(data.get("missing"), None);
    }

    #[tokio::test]
    async fn data_buffers_json_body_for_manual_validation() {
        let cx = cx(Method::POST, "/signup", Some(JSON_CONTENT_TYPE));
        let data = Data::from_request(
            &cx,
            Body::from(r#"{"email":"a@b.com","password":"secret123"}"#),
        )
        .await
        .expect("json body is buffered");

        assert!(SignUp::validate(&data).is_ok());
        assert_eq!(data.get("email"), Some("a@b.com"));
    }

    #[tokio::test]
    async fn data_reads_query_string_for_get() {
        let cx = cx(
            Method::GET,
            "/signup?email=a%40b.com&password=secret123",
            None,
        );
        let data = Data::from_request(&cx, Body::empty())
            .await
            .expect("query string is buffered");

        assert!(SignUp::validate(&data).is_ok());
    }

    #[tokio::test]
    async fn data_reports_validation_errors_in_the_handler() {
        let cx = cx(Method::POST, "/signup", Some(FORM_CONTENT_TYPE));
        let data = Data::from_request(&cx, Body::from("email=not-an-email&password=short"))
            .await
            .expect("invalid data is still buffered");

        let errors = SignUp::validate(&data).expect_err("validation fails in the handler");
        assert_eq!(errors.get("email").unwrap().code(), "email");
        assert_eq!(errors.get("password").unwrap().code(), "min_length");
    }

    #[test]
    fn form_content_type_recognizes_urlencoded_media_types() {
        assert!(form_content_type(Some(FORM_CONTENT_TYPE)));
        assert!(form_content_type(Some(
            "application/x-www-form-urlencoded; charset=utf-8"
        )));
        assert!(form_content_type(Some("APPLICATION/X-WWW-FORM-URLENCODED")));

        assert!(!form_content_type(None));
        assert!(!form_content_type(Some(JSON_CONTENT_TYPE)));
    }

    #[test]
    fn json_content_type_recognizes_json_media_types() {
        assert!(json_content_type(Some(JSON_CONTENT_TYPE)));
        assert!(json_content_type(Some("application/json; charset=utf-8")));
        assert!(json_content_type(Some("APPLICATION/JSON")));

        assert!(!json_content_type(None));
        assert!(!json_content_type(Some(FORM_CONTENT_TYPE)));
    }
}
