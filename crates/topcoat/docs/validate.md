# Validation

Topcoat validates untrusted input, such as HTML form data or JSON bodies, into typed Rust structs declared with the `Schema` derive macro, and emits a runtime description of the schema for form tooling.

The validation types live under `topcoat::validate`, which is enabled by the `validate` feature. Use `validate-router` to get the request input extractors, and `validate-regex` to enable the `regex` validator.

# Declaring a schema

```rust
use topcoat::validate::Schema;

#[derive(Schema)]
struct SignUp {
    #[schema(email, max_length = 254)]
    email: String,

    #[schema(min_length = 8)]
    password: String,
}
```

The derive emits an inherent `validate` method and a `descriptor` method, so no trait import is needed at call sites. Missing, empty, and JSON `null` values are normalized to "missing" and handled according to the Rust type: `Option<T>` becomes `None`, `#[schema(default = expr)]` uses the expression, and any other field reports a `"required"` error.

# Validating data

```rust
use topcoat::validate::Schema;
# #[derive(Schema)] struct SignUp { #[schema(email)] email: String }

let data = vec![
    ("email".to_string(), "user@example.com".to_string()),
];
let user = SignUp::validate(&data).unwrap();
```

`Schema::validate` accepts any data source implementing `ValidationData`: `serde_json::Value`, `HashMap<String, String>`, `HashMap<String, Value>`, `Vec<(String, String)>`, and `Value` itself. See the [`topcoat-validate` schema guide](https://github.com/tokio-rs/topcoat/blob/main/crates/topcoat-validate/docs/schema.md) for the full validator reference, custom validators, nested schemas, and runtime descriptors.

# Validating requests

With the `validate-router` feature, request input is extracted and validated in three stages. `Input` is the first: for `GET` and `HEAD` requests it parses the query string, and for other methods it accepts an `application/x-www-form-urlencoded` or `application/json` body. Extraction failures never reach a handler: a malformed body, a missing or unsupported media type, or a body over the size limit is rejected while extracting. Schema failures are the only validation failures a handler sees.

## Automatic rejection with `Valid<T>`

`Valid<T>` validates the input against the schema `T` and rejects invalid submissions with `400 Bad Request`, a plain-text response carrying the field-level messages. The handler receives the typed value only for valid input:

```rust,ignore
use topcoat::{
    Result,
    router::route,
    validate::{Schema, Valid},
    view::view,
};

#[derive(Schema)]
struct SignUp {
    #[schema(email)]
    email: String,
}

#[route(POST "/signup")]
async fn signup(Valid(form): Valid<SignUp>) -> Result {
    view! {
        <h1>"Welcome, "(form.email)"!"</h1>
    }
}
```

## Custom failure responses with `Validation<T>`

`Validation<T>` performs the same validation but hands the outcome to the handler, which chooses the failure response. On failure, the `Invalid` carries both the errors and the submitted input, so a form can be re-rendered with its errors and values:

```rust,ignore
use topcoat::{
    Result,
    context::Cx,
    router::{
        error::see_other,
        response::{IntoResponse, Response},
        route,
    },
    validate::{Invalid, Validation},
};

#[route(POST "/signup")]
async fn signup(cx: &Cx, validation: Validation<SignUp>) -> Result<Response> {
    match validation {
        // A real application would create the account here.
        Validation(Ok(_)) => see_other("/welcome").into_response(cx),
        // `input.get("email")` returns the submitted string, for
        // repopulating form fields on failure.
        Validation(Err(Invalid { errors, input })) => {
            my_signup_form(&errors, &input).into_response(cx)
        }
    }
}
```

## Delayed validation with `Input`

`Input` stops at parsing: no validation happens during extraction. Where validation must be delayed, or the schema selected in code, call `input.validate::<T>()`:

```rust,ignore
use topcoat::{
    Result,
    context::Cx,
    router::{
        error::see_other,
        response::{IntoResponse, Response},
        route,
    },
    validate::{Input, Invalid},
};

#[route(POST "/signup")]
async fn signup(cx: &Cx, input: Input) -> Result<Response> {
    let form = match input.validate::<SignUp>() {
        Ok(form) => form,
        Err(Invalid { errors, input }) => {
            return my_signup_form(&errors, &input).into_response(cx);
        }
    };

    // A real application would create the account here.
    see_other("/welcome").into_response(cx)
}
```

See [`examples/validate`](https://github.com/tokio-rs/topcoat/blob/main/examples/validate) for a complete form page showing both workflows: the `Valid<T>` extractor rejecting invalid submissions on one route, and `Validation<T>` redirecting on success and re-rendering the form with its errors and values on failure.
