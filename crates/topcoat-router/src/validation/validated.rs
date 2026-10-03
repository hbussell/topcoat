use std::{
    future::Future,
    ops::{Deref, DerefMut},
};

use topcoat_core::{context::Cx, error::Result};

use crate::{
    Body,
    request::{FromRequest, OptionalFromRequest},
    validation::ValidationErrors,
};

/// Validates a value using request context.
///
/// Implement this trait for types parsed by [`Form`](crate::content::Form),
/// [`Json`](crate::content::Json), or the [`Validated`] extractor. Rules can
/// read `cx` to access app context, sessions, cookies, headers, and more.
///
/// The returned future must be [`Send`], which means every implementor must
/// satisfy `Self: Sync` because it captures `&self`.
pub trait Validate {
    /// Validates `self` and returns all failures at once.
    fn validate(&self, cx: &Cx) -> impl Future<Output = Result<(), ValidationErrors>> + Send;
}

/// Validates the inner value when present, otherwise succeeds.
#[allow(clippy::unused_async_trait_impl)]
impl<T: Validate + Sync> Validate for Option<T> {
    async fn validate(&self, cx: &Cx) -> Result<(), ValidationErrors> {
        match self {
            Some(value) => value.validate(cx).await,
            None => Ok(()),
        }
    }
}

/// Forwards validation to the wrapped value.
#[allow(clippy::unused_async_trait_impl)]
impl<T: Validate + Sync> Validate for Validated<T> {
    async fn validate(&self, cx: &Cx) -> Result<(), ValidationErrors> {
        self.0.validate(cx).await
    }
}

/// Extractor that parses a request body and then validates it.
///
/// Wrap [`Form`](crate::content::Form) or [`Json`](crate::content::Json) to
/// reject invalid input with `422 Unprocessable Entity` while leaving parse
/// failures as `400 Bad Request`.
///
/// ```rust
/// use topcoat::{
///     Result,
///     router::{content::Json, route, validation::Validated},
/// };
///
/// # #[derive(serde::Deserialize, serde::Serialize)]
/// # struct User { name: String }
/// # impl topcoat::validation::Validate for User {
/// #     async fn validate(&self, _cx: &topcoat::context::Cx) -> topcoat::Result<(), topcoat::validation::ValidationErrors> { Ok(()) }
/// # }
/// #[route(POST "/api/users")]
/// async fn create_user(Validated(Json(user)): Validated<Json<User>>) -> Result<Json<User>> {
///     Ok(Json(user))
/// }
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[must_use]
pub struct Validated<T>(pub T);

impl<T> From<T> for Validated<T> {
    fn from(value: T) -> Self {
        Self(value)
    }
}

impl<T> Deref for Validated<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> DerefMut for Validated<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<T> FromRequest for Validated<T>
where
    T: FromRequest + Validate + Send + Sync,
{
    async fn from_request(cx: &Cx, body: Body) -> Result<Self> {
        let inner = T::from_request(cx, body).await?;
        inner.validate(cx).await?;
        Ok(Self(inner))
    }
}

impl<T> OptionalFromRequest for Validated<T>
where
    T: OptionalFromRequest + Validate + Send + Sync,
{
    async fn from_request(cx: &Cx, body: Body) -> Result<Option<Self>> {
        match T::from_request(cx, body).await? {
            Some(inner) => {
                inner.validate(cx).await?;
                Ok(Some(Self(inner)))
            }
            None => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use http::{Method, Request, header::CONTENT_TYPE};
    use serde::Deserialize;
    use topcoat_core::context::{Cx, CxTestBuilder};

    use super::*;
    use crate::{
        content::{Form, Json},
        error::BadRequestError,
        request::FromRequest,
    };

    #[derive(Debug, Deserialize)]
    struct Signup {
        name: String,
    }

    impl Validate for Signup {
        #[allow(clippy::unused_async_trait_impl)]
        async fn validate(&self, _cx: &Cx) -> Result<(), ValidationErrors> {
            let mut errors = ValidationErrors::new();
            errors.check("name", !self.name.is_empty(), "name is required");
            errors.into_result()
        }
    }

    #[derive(Debug, Deserialize)]
    #[allow(dead_code)]
    struct Numbered {
        count: u32,
    }

    impl Validate for Numbered {
        #[allow(clippy::unused_async_trait_impl)]
        async fn validate(&self, _cx: &Cx) -> Result<(), ValidationErrors> {
            Ok(())
        }
    }

    #[derive(Debug, Deserialize)]
    struct Address {
        city: String,
    }

    #[derive(Debug, Deserialize)]
    struct Person {
        address: Address,
    }

    impl Validate for Address {
        #[allow(clippy::unused_async_trait_impl)]
        async fn validate(&self, _cx: &Cx) -> Result<(), ValidationErrors> {
            let mut errors = ValidationErrors::new();
            errors.check("city", !self.city.is_empty(), "city is required");
            errors.into_result()
        }
    }

    impl Validate for Person {
        async fn validate(&self, cx: &Cx) -> Result<(), ValidationErrors> {
            let mut errors = ValidationErrors::new();
            errors.nest("address", &self.address, cx).await;
            errors.into_result()
        }
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
    async fn validated_form_returns_400_on_parse_error() {
        let cx = cx(
            Method::POST,
            "/count",
            Some("application/x-www-form-urlencoded"),
        );
        let error =
            <Validated<Form<Numbered>> as FromRequest>::from_request(&cx, Body::from("count="))
                .await
                .expect_err("an empty required number is a parse error");

        assert!(error.downcast_ref::<BadRequestError>().is_some());
    }

    #[tokio::test]
    async fn validated_form_returns_422_on_validation_error() {
        let cx = cx(
            Method::POST,
            "/signup",
            Some("application/x-www-form-urlencoded"),
        );
        let error =
            <Validated<Form<Signup>> as FromRequest>::from_request(&cx, Body::from("name="))
                .await
                .expect_err("empty name is rejected");

        let errors = error
            .downcast::<ValidationErrors>()
            .expect("a validation error");
        assert_eq!(errors.first_message("name"), Some("name is required"));
    }

    #[tokio::test]
    async fn validated_json_rejects_invalid_input() {
        let cx = cx(Method::POST, "/api/users", Some("application/json"));
        let error = <Validated<Json<Signup>> as FromRequest>::from_request(
            &cx,
            Body::from(r#"{"name":""}"#),
        )
        .await
        .expect_err("empty name is rejected");

        let errors = error
            .downcast::<ValidationErrors>()
            .expect("a validation error");
        assert_eq!(errors.first_message("name"), Some("name is required"));
    }

    #[tokio::test]
    async fn optional_validated_form_is_none_without_content_type() {
        let cx = cx(Method::POST, "/signup", None);
        let form =
            <Option<Validated<Form<Signup>>> as FromRequest>::from_request(&cx, Body::empty())
                .await
                .expect("an absent body is not an error");

        assert!(form.is_none());
    }

    #[tokio::test]
    async fn optional_validated_form_validates_when_present() {
        let cx = cx(
            Method::POST,
            "/signup",
            Some("application/x-www-form-urlencoded"),
        );
        let result = <Option<Validated<Form<Signup>>> as FromRequest>::from_request(
            &cx,
            Body::from("name=Ada"),
        )
        .await;

        assert!(result.is_ok());
        assert!(result.unwrap().is_some());
    }

    #[tokio::test]
    async fn nested_validation_prefixes_field_paths() {
        let cx = cx(Method::POST, "/person", Some("application/json"));
        let result = <Validated<Json<Person>> as FromRequest>::from_request(
            &cx,
            Body::from(r#"{"address":{"city":""}}"#),
        )
        .await;

        let errors = result
            .expect_err("empty city is rejected")
            .downcast::<ValidationErrors>()
            .expect("a validation error");

        assert_eq!(
            errors.first_message("address.city"),
            Some("city is required")
        );
    }
}
