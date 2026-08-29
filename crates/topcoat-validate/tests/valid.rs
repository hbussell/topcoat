#![cfg(feature = "router")]

use http::{Method, Request, header::CONTENT_TYPE};
use topcoat_core::context::{Cx, CxTestBuilder};
use topcoat_router::{Body, request::FromRequest};
use topcoat_validate::{Valid, Validation};
use topcoat_validate_macro::Schema as SchemaMacro;

const FORM_CONTENT_TYPE: &str = "application/x-www-form-urlencoded";
const JSON_CONTENT_TYPE: &str = "application/json";

#[derive(Debug, SchemaMacro, PartialEq)]
struct SignUp {
    #[schema(email)]
    email: String,

    #[schema(min_length = 8)]
    password: String,
}

#[derive(Debug, SchemaMacro, PartialEq)]
struct Address {
    street: String,
    city: String,
}

#[derive(Debug, SchemaMacro, PartialEq)]
struct Contact {
    name: String,
    addresses: Vec<Address>,
}

#[derive(Debug, SchemaMacro, PartialEq)]
struct BulkContact {
    contacts: Vec<Contact>,
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
async fn form_happy_path() {
    let cx = cx(Method::POST, "/signup", Some(FORM_CONTENT_TYPE));
    let Valid(form) =
        Valid::<SignUp>::from_request(&cx, Body::from("email=a%40b.com&password=secret123"))
            .await
            .expect("valid form body");

    assert_eq!(form.email, "a@b.com");
    assert_eq!(form.password, "secret123");
}

#[tokio::test]
async fn json_happy_path() {
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
async fn get_query_string() {
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
async fn missing_content_type_is_bad_request() {
    let cx = cx(Method::POST, "/signup", None);
    let error = Valid::<SignUp>::from_request(&cx, Body::from("email=a@b.com&password=secret123"))
        .await
        .expect_err("missing content type is rejected");

    assert!(
        error
            .downcast_ref::<topcoat_router::error::BadRequestError>()
            .is_some()
    );
}

#[tokio::test]
async fn validation_errors_are_bad_request() {
    let cx = cx(Method::POST, "/signup", Some(FORM_CONTENT_TYPE));
    let error = Valid::<SignUp>::from_request(&cx, Body::from("email=not-an-email&password=short"))
        .await
        .expect_err("invalid form body is rejected");

    let bad_request = error.downcast_ref::<topcoat_router::error::BadRequestError>();
    assert!(bad_request.is_some());
    let description = bad_request.unwrap().description();
    assert!(description.contains("email"));
    assert!(description.contains("password"));
}

#[tokio::test]
async fn json_nested_list_of_contacts() {
    let cx = cx(Method::POST, "/contacts", Some(JSON_CONTENT_TYPE));
    let body = r#"{
        "contacts": [
            {
                "name": "Alice",
                "addresses": [
                    { "street": "1 Main St", "city": "Sydney" },
                    { "street": "2 High St", "city": "Melbourne" }
                ]
            },
            {
                "name": "Bob",
                "addresses": [
                    { "street": "3 Queen St", "city": "Brisbane" }
                ]
            }
        ]
    }"#;

    let Valid(form) = Valid::<BulkContact>::from_request(&cx, Body::from(body))
        .await
        .expect("valid nested json body");

    assert_eq!(form.contacts.len(), 2);

    let alice = &form.contacts[0];
    assert_eq!(alice.name, "Alice");
    assert_eq!(alice.addresses.len(), 2);
    assert_eq!(alice.addresses[0].street, "1 Main St");
    assert_eq!(alice.addresses[0].city, "Sydney");
    assert_eq!(alice.addresses[1].street, "2 High St");
    assert_eq!(alice.addresses[1].city, "Melbourne");

    let bob = &form.contacts[1];
    assert_eq!(bob.name, "Bob");
    assert_eq!(bob.addresses.len(), 1);
    assert_eq!(bob.addresses[0].street, "3 Queen St");
    assert_eq!(bob.addresses[0].city, "Brisbane");
}

#[tokio::test]
async fn form_nested_list_of_contacts() {
    let cx = cx(Method::POST, "/contacts", Some(FORM_CONTENT_TYPE));
    let body = "contacts.0.name=Alice\
        &contacts.0.addresses.0.street=1+Main+St\
        &contacts.0.addresses.0.city=Sydney\
        &contacts.0.addresses.1.street=2+High+St\
        &contacts.0.addresses.1.city=Melbourne\
        &contacts.1.name=Bob\
        &contacts.1.addresses.0.street=3+Queen+St\
        &contacts.1.addresses.0.city=Brisbane";

    let Valid(form) = Valid::<BulkContact>::from_request(&cx, Body::from(body))
        .await
        .expect("valid nested form body");

    assert_eq!(form.contacts.len(), 2);

    let alice = &form.contacts[0];
    assert_eq!(alice.name, "Alice");
    assert_eq!(alice.addresses.len(), 2);
    assert_eq!(alice.addresses[0].street, "1 Main St");
    assert_eq!(alice.addresses[0].city, "Sydney");
    assert_eq!(alice.addresses[1].street, "2 High St");
    assert_eq!(alice.addresses[1].city, "Melbourne");

    let bob = &form.contacts[1];
    assert_eq!(bob.name, "Bob");
    assert_eq!(bob.addresses.len(), 1);
    assert_eq!(bob.addresses[0].street, "3 Queen St");
    assert_eq!(bob.addresses[0].city, "Brisbane");
}

#[tokio::test]
async fn json_nested_list_of_contacts_reports_errors() {
    let cx = cx(Method::POST, "/contacts", Some(JSON_CONTENT_TYPE));
    let body = r#"{
        "contacts": [
            {
                "name": "Alice",
                "addresses": [
                    { "street": "1 Main St", "city": "" }
                ]
            },
            {
                "name": "",
                "addresses": [
                    { "street": "", "city": "Brisbane" }
                ]
            }
        ]
    }"#;

    let Validation(result) = Validation::<BulkContact>::from_request(&cx, Body::from(body))
        .await
        .expect("invalid input reaches the handler");

    let invalid = result.expect_err("validation fails");
    assert_eq!(
        invalid
            .errors
            .get("contacts.0.addresses.0.city")
            .unwrap()
            .code(),
        "required"
    );
    assert_eq!(
        invalid.errors.get("contacts.1.name").unwrap().code(),
        "required"
    );
    assert_eq!(
        invalid
            .errors
            .get("contacts.1.addresses.0.street")
            .unwrap()
            .code(),
        "required"
    );
}

#[tokio::test]
async fn body_over_limit_is_content_too_large() {
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
