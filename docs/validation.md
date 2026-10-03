# Validation

Validate request input on top of the existing [`Form`](crate::router::content::Form) and [`Json`](crate::router::content::Json) extractors.

The validation module is feature-gated. Enable it with the `validation` feature.

## Basic usage

Implement [`Validate`](crate::validation::Validate) for the type you parse. Use the built-in rules in [`rules`](crate::validation::rules) and collect failures into a [`ValidationErrors`](crate::validation::ValidationErrors):

```rust
use serde::Deserialize;
use topcoat::{
    Result,
    context::Cx,
    validation::{Validate, ValidationErrors, rules},
};

#[derive(Debug, Deserialize)]
struct NewUser {
    name: String,
    email: String,
}

impl Validate for NewUser {
    async fn validate(&self, _cx: &Cx) -> Result<(), ValidationErrors> {
        let mut errors = ValidationErrors::new();

        errors.check_with_code(
            "name",
            rules::required(&self.name),
            rules::CODE_REQUIRED,
            "name is required",
        );
        errors.check_with_code(
            "email",
            rules::email(&self.email),
            rules::CODE_INVALID_EMAIL,
            "email is invalid",
        );

        errors.into_result()
    }
}
```

The future returned by [`validate`](Validate::validate) must be [`Send`], so any value held across an `.await` inside `validate` must be [`Send`] as well. The implementor must also be [`Sync`] because the future captures `&self`.

## Extractor

Wrap [`Form`](crate::router::content::Form) or [`Json`](crate::router::content::Json) with [`Validated`](crate::validation::Validated) to validate after parsing. Parse failures stay `400 Bad Request`; validation failures become `422 Unprocessable Entity`.

```rust
# use serde::{Deserialize, Serialize};
# use topcoat::{Result, context::Cx, validation::{Validate, ValidationErrors}};
# #[derive(Debug, Deserialize, Serialize)]
# struct NewUser { name: String }
# impl Validate for NewUser {
#     async fn validate(&self, _cx: &Cx) -> Result<(), ValidationErrors> { Ok(()) }
# }
use topcoat::{
    router::{content::Json, route, validation::Validated},
};

#[route(POST "/api/users")]
async fn create_user(Validated(Json(user)): Validated<Json<NewUser>>) -> Result<Json<NewUser>> {
    Ok(Json(user))
}
```

Handlers can also call `validate` manually and return any response, such as a re-rendered HTML form.

## Nested values

Use [`nest`](ValidationErrors::nest) and [`nest_each`](ValidationErrors::nest_each) to validate nested structs and lists. Inner field paths are prefixed automatically.

```rust
use serde::Deserialize;
use topcoat::{
    Result,
    context::Cx,
    validation::{Validate, ValidationErrors, rules},
};

#[derive(Debug, Deserialize)]
struct Address {
    city: String,
    zip: String,
}

#[derive(Debug, Deserialize)]
struct Tag {
    name: String,
}

#[derive(Debug, Deserialize)]
struct Signup {
    name: String,
    address: Address,
    tags: Vec<Tag>,
}

impl Validate for Address {
    async fn validate(&self, _cx: &Cx) -> Result<(), ValidationErrors> {
        let mut errors = ValidationErrors::new();
        errors.check("city", rules::required(&self.city), "city is required");
        errors.check("zip", rules::required(&self.zip), "zip is required");
        errors.into_result()
    }
}

impl Validate for Tag {
    async fn validate(&self, _cx: &Cx) -> Result<(), ValidationErrors> {
        let mut errors = ValidationErrors::new();
        errors.check("name", rules::required(&self.name), "name is required");
        errors.into_result()
    }
}

impl Validate for Signup {
    async fn validate(&self, cx: &Cx) -> Result<(), ValidationErrors> {
        let mut errors = ValidationErrors::new();
        errors.check("name", rules::required(&self.name), "name is required");
        errors.nest("address", &self.address, cx).await;
        errors.nest_each("tags", &self.tags, cx).await;
        errors.into_result()
    }
}
```

An error on `zip` becomes `address.zip`. An error on a tag name becomes `tags[0].name`.

`Option<Nested>` works through the blanket [`Validate`] impl for [`Option`], so `nest` also accepts optional nested values.

## Optional fields

The built-in rules take `&str`, so optional fields need a conditional check: `None` passes, `Some` is validated.

```rust
use serde::Deserialize;
use topcoat::{
    Result,
    context::Cx,
    validation::{Validate, ValidationErrors, rules},
};

#[derive(Debug, Deserialize)]
struct Profile {
    email: Option<String>,
}

impl Validate for Profile {
    async fn validate(&self, _cx: &Cx) -> Result<(), ValidationErrors> {
        let mut errors = ValidationErrors::new();

        if let Some(email) = &self.email {
            errors.check_with_code(
                "email",
                rules::email(email),
                rules::CODE_INVALID_EMAIL,
                "email is invalid",
            );
        }

        errors.into_result()
    }
}
```

## Context checks

Because [`Validate`](Validate::validate) receives `&Cx`, rules can access app context, sessions, cookies, headers, and more.

```rust
use std::collections::HashSet;
use topcoat::{
    Result,
    context::{Cx, app_context},
    validation::{Validate, ValidationErrors},
};

#[derive(Debug)]
struct NewUser {
    name: String,
}

#[derive(Debug, Default)]
struct UserDb {
    names: HashSet<String>,
}

impl Validate for NewUser {
    async fn validate(&self, cx: &Cx) -> Result<(), ValidationErrors> {
        let mut errors = ValidationErrors::new();

        if app_context::<UserDb>(cx).names.contains(&self.name) {
            errors.add_with_code("name", "taken", "name is already taken");
        }

        errors.into_result()
    }
}
```

## Form rendering

Catch [`ValidationErrors`](ValidationErrors) manually to render a form again with inline errors.

```rust
use serde::Deserialize;
use topcoat::{
    Result,
    context::Cx,
    router::{StatusCode, content::Form, page},
    validation::{Validate, ValidationErrors, rules},
    view::{View, ViewExt as _, view},
};

#[derive(Debug, Deserialize)]
struct NewUser {
    name: String,
}

impl Validate for NewUser {
    async fn validate(&self, _cx: &Cx) -> Result<(), ValidationErrors> {
        let mut errors = ValidationErrors::new();
        errors.check("name", rules::required(&self.name), "name is required");
        errors.into_result()
    }
}

#[page(POST "/signup")]
async fn signup(cx: &Cx, Form(input): Form<NewUser>) -> Result<impl View> {
    match input.validate(cx).await {
        Ok(()) => Ok(view! { <h1>"Welcome, " (input.name) "!"</h1> }.boxed()),
        Err(errors) => Ok(view! {
            (StatusCode::UNPROCESSABLE_ENTITY)
            <form method="post">
                <input name="name" value=(input.name.clone())>
                if let Some(message) = errors.first_message("name") {
                    <p class="error">(message)</p>
                }
            </form>
        }.boxed()),
    }
}
```

Use the view helpers on [`ValidationErrors`](ValidationErrors) to look up messages by field path:

```rust
use serde::Deserialize;
use topcoat::{
    context::Cx,
    validation::ValidationErrors,
    view::{View, view},
};

#[derive(Debug, Deserialize)]
struct NewUser {
    name: String,
}

fn signup_form(cx: &Cx, input: &NewUser, errors: &ValidationErrors) -> impl View {
    view! { cx =>
        <form method="post">
            <input name="name" value=(input.name.clone())>
            if let Some(message) = errors.first_message("name") {
                <p class="error">(message)</p>
            }
        </form>
    }
}
```

## API responses

When a validation error is not caught, the [`IntoResponse`](crate::router::response::IntoResponse) implementation of [`ValidationErrors`] renders a `422 Unprocessable Entity` response with [RFC 9457](https://www.rfc-editor.org/rfc/rfc9457) problem details and the `application/problem+json` media type. This applies anywhere a handler returns a `Result` with a `ValidationErrors` error, not only through the [`Validated`] extractor.

```json
{
  "type": "about:blank",
  "title": "Unprocessable Content",
  "status": 422,
  "detail": "validation failed with 1 error(s)",
  "errors": [
    {
      "pointer": "/address/zip",
      "code": "required",
      "detail": "zip is required"
    }
  ]
}
```

`errors` is an RFC 9457 extension member with one entry per field failure. `pointer` is a JSON Pointer (RFC 6901) converted from the Rust field path, `code` is the rule code and is omitted when none was given, and `detail` is the human-readable message. Clients should match errors on `pointer` and `code`.

## Field path limitations

Field paths are split on `.` for nesting and `[` / `]` for list indices before they are converted to JSON Pointers. There is currently no way to escape these characters inside a field name, so a field named `meta.data` is always turned into the pointer `/meta/data`. The conversion does escape `~` and `/` according to JSON Pointer rules, but it cannot preserve a `.` or `[` that is part of a field name. Avoid these characters in field names when clients rely on `pointer`.

## Rules

The initial rule set lives in [`rules`](crate::validation::rules):

- [`required`](crate::validation::rules::required): rejects empty and whitespace-only strings
- [`min_length`](crate::validation::rules::min_length)
- [`max_length`](crate::validation::rules::max_length)
- [`range`](crate::validation::rules::range)
- [`email`](crate::validation::rules::email)

Each rule has a matching `CODE_*` constant for use with `check_with_code`.
