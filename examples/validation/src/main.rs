use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use topcoat::{
    Result,
    context::{Cx, app_context},
    router::{
        RouterBuilderDiscoverExt, StatusCode,
        content::{Form, Json},
        module_router, page, route,
        validation::Validated,
    },
    validation::{Validate, ValidationErrors, rules},
    view::{View, ViewExt as _, component, view},
};

#[derive(Debug)]
struct UserDb {
    names: HashSet<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct Address {
    city: String,
    zip: String,
}

impl Validate for Address {
    #[allow(clippy::unused_async_trait_impl)]
    async fn validate(&self, _cx: &Cx) -> Result<(), ValidationErrors> {
        let mut errors = ValidationErrors::new();
        errors.check_with_code(
            "city",
            rules::required(&self.city),
            rules::CODE_REQUIRED,
            "city is required",
        );
        errors.check_with_code(
            "zip",
            rules::required(&self.zip),
            rules::CODE_REQUIRED,
            "zip is required",
        );
        errors.into_result()
    }
}

#[derive(Debug, Default, Deserialize)]
struct SignupForm {
    name: String,
    email: String,
}

impl Validate for SignupForm {
    #[allow(clippy::unused_async_trait_impl)]
    async fn validate(&self, cx: &Cx) -> Result<(), ValidationErrors> {
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

        if app_context::<UserDb>(cx).names.contains(&self.name) {
            errors.add_with_code("name", "taken", "name is already taken");
        }

        errors.into_result()
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct NewUser {
    name: String,
    email: String,
    address: Address,
}

impl Validate for NewUser {
    async fn validate(&self, cx: &Cx) -> Result<(), ValidationErrors> {
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
        errors.nest("address", &self.address, cx).await;

        if app_context::<UserDb>(cx).names.contains(&self.name) {
            errors.add_with_code("name", "taken", "name is already taken");
        }

        errors.into_result()
    }
}

#[component]
async fn signup_form(
    cx: &Cx,
    input: &SignupForm,
    errors: Option<&ValidationErrors>,
) -> Result<impl View> {
    Ok(view! {
        cx =>
        <form method="post" action="/signup">
            <div>
                <input name="name" value=(input.name.clone()) placeholder="Name">
                if let Some(message) = errors.and_then(
                    |errors| errors.first_message("name"),
                ) {
                    <p class="error">(message)</p>
                }
            </div>
            <div>
                <input name="email" value=(input.email.clone()) placeholder="Email">
                if let Some(message) = errors.and_then(
                    |errors| errors.first_message("email"),
                ) {
                    <p class="error">(message)</p>
                }
            </div>
            <button type="submit">"Sign up"</button>
        </form>
    })
}

#[page(GET "/signup")]
async fn signup_get(cx: &Cx) -> Result<impl View> {
    let input = SignupForm::default();
    Ok(view! { cx => signup_form(input: &input, errors: None) }.boxed())
}

#[page(POST "/signup")]
async fn signup_post(cx: &Cx, Form(input): Form<SignupForm>) -> Result<impl View> {
    match input.validate(cx).await {
        Ok(()) => Ok(view! {
            <h1>
                "Welcome, "
                (input.name)
                "!"
            </h1>
        }
        .boxed()),
        Err(errors) => Ok(view! {
            cx =>
            (StatusCode::UNPROCESSABLE_ENTITY)
            signup_form(input: &input, errors: Some(&errors))
        }
        .boxed()),
    }
}

#[route(POST "/api/users")]
async fn create_user(
    Validated(Json(user)): Validated<Json<NewUser>>,
) -> Result<Json<serde_json::Value>> {
    Ok(Json(serde_json::json!({ "name": user.name })))
}

#[tokio::main]
async fn main() {
    let mut names = HashSet::new();
    names.insert("taken".to_owned());

    topcoat::start(
        module_router!()
            .discover()
            .app_context(UserDb { names })
            .build(),
    )
    .await
    .unwrap();
}
