# Validation

Topcoat validates untrusted input, such as HTML form data or JSON bodies, into typed Rust structs declared with the `Schema` derive macro, and emits a runtime description of the schema for form tooling.

The validation types live under `topcoat::validate`, which is enabled by the `validate` feature. Use `validate-router` to get the `Valid<T>` request extractor, and `validate-regex` to enable the `regex` validator.

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

The `Schema` trait is also implemented for `serde_json::Value`, `HashMap<String, String>`, `HashMap<String, Value>`, `Vec<(String, String)>`, and `Value` itself. See the [`topcoat-validate` schema guide](https://github.com/tokio-rs/topcoat/blob/main/crates/topcoat-validate/docs/schema.md) for the full validator reference, custom validators, nested schemas, and runtime descriptors.

# Validating requests

With the `validate-router` feature, `Valid<T>` is a request body extractor. For `GET` and `HEAD` requests it parses the query string; for other methods it accepts `application/x-www-form-urlencoded` or `application/json`. Validation failures are turned into a `400 Bad Request` response carrying the field-level messages. When a handler should choose its own failure response, extract `Data` instead and pattern-match on `SignUp::validate(&data)` in the handler body; `Data::get(name)` returns a submitted string, for repopulating form fields on failure.

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

See [`examples/validate`](https://github.com/tokio-rs/topcoat/blob/main/examples/validate) for a complete form page showing both styles: the `Valid<T>` extractor, and pattern-matching on the validation result to redirect on success and re-render the form with its errors on failure.
