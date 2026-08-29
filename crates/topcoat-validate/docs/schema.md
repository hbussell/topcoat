# Schema validation

Derive `Schema` on a Rust struct to validate untrusted input, such as HTML form data or a JSON body, into typed values, and to describe the schema at runtime for form tooling.

The derive macro lives in the `topcoat_validate_macro` crate. Facade users import both the trait and the macro from `topcoat::validate::Schema`; standalone users import them separately.

# Basic usage

```rust
use topcoat_validate::Schema;
use topcoat_validate_macro::Schema;

#[derive(Schema)]
struct SignUp {
    #[schema(email, max_length = 254)]
    email: String,

    #[schema(min_length = 8)]
    password: String,
}

let data = vec![
    ("email".to_string(), "user@example.com".to_string()),
    ("password".to_string(), "secret123".to_string()),
];
let user = SignUp::validate(&data).unwrap();
assert_eq!(user.email, "user@example.com");
```

The derive generates an inherent `validate` method as well as the `Schema` trait impl, so callers do not need to import the trait. It also generates `descriptor()` for introspecting the schema at runtime.

# Missing, empty, and null values

A field counts as missing when the key is absent, the value is an empty string, or the value is JSON `null`. The derive handles missing values according to the Rust type:

- `Option<T>` becomes `None`.
- A field with `#[schema(default = expr)]` uses `expr`.
- Any other field produces a `"required"` error.

```rust
use topcoat_validate::Schema;
use topcoat_validate_macro::Schema;

#[derive(Schema, Debug, PartialEq)]
struct Preferences {
    name: Option<String>,

    #[schema(default = false)]
    newsletter: bool,
}

let data = vec![];
let prefs = Preferences::validate(&data).unwrap();
assert_eq!(prefs, Preferences { name: None, newsletter: false });
```

# Supported types

The derive infers the expected shape from the Rust type.

| Rust type | Shape |
|-----------|-------|
| `String` | string |
| `u8`..`u64`, `i8`..`i64` | integer |
| `f32`, `f64` | float |
| `bool` | bool |
| `Option<T>` | optional `T` |
| `Vec<T>` | list of `T` |
| `T: Schema` | nested schema |

Integer strings are parsed; `u64` values larger than `i64::MAX` are rejected. `bool` accepts strings `"on"`, `"true"`, and `"1"` as true and `"off"`, `"false"`, and `"0"` as false, as well as JSON booleans.

Unsupported types, including `&str`, `i128`, `u128`, and enums, fail at compile time with a message naming the field and suggesting `String`, `i64`, or a custom validator.

# Validators

List validators inside `#[schema(...)]`. They run in declaration order and the first failure for a field wins.

- `email`: the value must look like an email address.
- `min_length = N`: string must have at least `N` characters.
- `max_length = N`: string must have at most `N` characters.
- `min = N`: number must be at least `N`.
- `max = N`: number must be at most `N`.
- `range(min = N, max = M)`: number must be inside the inclusive range.
- `one_of = "a, b, c"`: string must be one of the comma-separated choices.
- `regex = "..."`: string must match the pattern. Requires the `regex` feature.
- `trim`: trim whitespace before other checks.
- `string`, `number`, `bool`: explicit coercion hints, rarely needed.
- `custom = path::ToType`: run a `CustomValidator` implementation.
- `rename = "field_name"`: read the field under a different key.
- `default = expr`: use `expr` when the field is missing.

```rust
use topcoat_validate::Schema;
use topcoat_validate_macro::Schema;

#[derive(Schema)]
struct Profile {
    #[schema(trim, min_length = 1)]
    name: String,

    #[schema(range(min = 1, max = 120))]
    age: u32,

    #[schema(one_of = "user, admin")]
    role: String,
}
```

# Custom validators

Implement `CustomValidator` on a unit struct and reference it with `#[schema(custom = MyValidator)]`. The trait is invoked through associated functions, so no instance is needed. Every custom validator provides a machine-readable name and a default message, which surface in the runtime descriptor.

```rust
use std::borrow::Cow;

use topcoat_validate::{Schema, ValidationError, Value, validator::CustomValidator};
use topcoat_validate_macro::Schema;

struct StartsWithA;

impl CustomValidator for StartsWithA {
    fn validate(value: &Value) -> Result<Value, ValidationError> {
        match value.as_str() {
            Some(s) if s.starts_with('a') => Ok(value.clone()),
            _ => Err(ValidationError::with_code("starts_with_a", "Must start with a")),
        }
    }

    fn name() -> &'static str {
        "starts_with_a"
    }

    fn message() -> Cow<'static, str> {
        Cow::Borrowed("Must start with the letter a")
    }
}

#[derive(Schema)]
struct Named {
    #[schema(custom = StartsWithA)]
    name: String,
}
```

# Nested schemas

A field whose type implements `Schema` is validated recursively. From flat form data, dotted keys such as `address.city` are collected into the nested view automatically.

```rust
use topcoat_validate::Schema;
use topcoat_validate_macro::Schema;

#[derive(Schema, Debug, PartialEq)]
struct Address {
    city: String,
    zip: u32,
}

#[derive(Schema, Debug, PartialEq)]
struct Contact {
    name: String,
    address: Address,
}

let data = vec![
    ("name".to_string(), "Alice".to_string()),
    ("address.city".to_string(), "Sydney".to_string()),
    ("address.zip".to_string(), "2000".to_string()),
];
let contact = Contact::validate(&data).unwrap();
assert_eq!(contact.address.city, "Sydney");
```

Errors from nested fields are reported with dotted paths such as `address.city`.

# Lists

`Vec<T>` accepts repeated form keys or JSON arrays.

```rust
use topcoat_validate::Schema;
use topcoat_validate_macro::Schema;

#[derive(Schema, Debug, PartialEq)]
struct Tags {
    tags: Vec<String>,
}

let data = vec![
    ("tags".to_string(), "a".to_string()),
    ("tags".to_string(), "b".to_string()),
];
let value = Tags::validate(&data).unwrap();
assert_eq!(value.tags, vec!["a", "b"]);
```

Lists of nested schemas can also be supplied from flat form data using dotted numeric indices:

```rust
use topcoat_validate::Schema;
use topcoat_validate_macro::Schema;

#[derive(Schema, Debug, PartialEq)]
struct Address {
    city: String,
}

#[derive(Schema, Debug, PartialEq)]
struct Contact {
    addresses: Vec<Address>,
}

let data = vec![
    ("addresses.0.city".to_string(), "Sydney".to_string()),
    ("addresses.1.city".to_string(), "Melbourne".to_string()),
];
let value = Contact::validate(&data).unwrap();
assert_eq!(value.addresses[0].city, "Sydney");
assert_eq!(value.addresses[1].city, "Melbourne");
```

# Runtime descriptor

`Schema::descriptor()` returns a `SchemaDescriptor` that describes every field, its type, whether it is required, and the validators applied to it. This is the foundation for future form controls and client-side parity.

```rust
use topcoat_validate::{Number, Schema, descriptor::{FieldType, ValidatorDescriptor}};
use topcoat_validate_macro::Schema;

#[derive(Schema)]
struct Account {
    #[schema(email, max_length = 254)]
    email: String,

    #[schema(min = 13)]
    age: u32,
}

let descriptor = Account::descriptor();
assert_eq!(descriptor.fields[0].ty, FieldType::String);
assert!(descriptor.fields[0].validators.contains(&ValidatorDescriptor::Email));
assert_eq!(descriptor.fields[1].validators, vec![ValidatorDescriptor::Min(Number::Integer(13))]);
```

# Router integration

With the `router` feature enabled, `Valid<T>` is a request body extractor. For `GET` and `HEAD` requests it parses the query string; for other methods it accepts `application/x-www-form-urlencoded` or `application/json`. Validation failures become a `400 Bad Request` response carrying the field-level messages.

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

For handlers that choose their own failure response, the `Data` extractor buffers the same request data without validating, so the handler can pattern-match on the result itself:

```rust,ignore
use topcoat::{
    Result,
    router::{error::see_other, response::{IntoResponse, Response}, route},
    validate::{Data, Schema},
};

#[route(POST "/signup")]
async fn signup(cx: &topcoat::context::Cx, data: Data) -> Result<Response> {
    match SignUp::validate(&data) {
        // A real application would create the account here.
        Ok(_) => see_other("/welcome").into_response(cx),
        Err(errors) => {
            // Re-render the form with the errors; `data.get("email")` returns
            // the submitted string for repopulating the input.
            my_signup_form(Some(&errors)).into_response(cx)
        }
    }
}
```

See [`crates/topcoat/docs/validate.md`](https://github.com/tokio-rs/topcoat/blob/main/crates/topcoat/docs/validate.md) for the facade-level guide.
