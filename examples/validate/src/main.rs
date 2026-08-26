use serde::Deserialize;
use topcoat::{
    Result, context::Cx, router::{
        Router, RouterBuilderDiscoverExt, StatusCode, error::{SeeOther, see_other}, page, response::{IntoResponse, Response}, route,
    }, validate::{Data, Schema, Valid, ValidationErrors}, view::{View, component, view},
};

#[derive(Schema, Deserialize)]
#[allow(dead_code)] // the example stops at validation; a real app would use the data
struct SignUp {
    #[schema(email, max_length = 254)]
    email: String,

    #[schema(min_length = 8)]
    password: String,
}

#[tokio::main]
async fn main() {
    topcoat::start(Router::builder().discover().build())
        .await
        .unwrap();
}

// A plain component rather than a layout, so the manual route's error path
// can reuse the page shell: route handlers do not pass through layouts.
#[component]
async fn shell(child: View) -> Result {
    view! {
        <!DOCTYPE html>
        <html>
            <head>
                <title>"Validate"</title>
                topcoat::dev::script()
            </head>
            <body>(child)</body>
        </html>
    }
}

#[page("/")]
async fn home() -> Result {
    view! {
        shell(
            <h1>"Validate"</h1>
            <ul>
                <li>
                    <a href="/extractor">"Extractor"</a>
                    ": `Valid<SignUp>` in the handler signature rejects invalid submissions automatically."
                </li>
                <li>
                    <a href="/manual">"Pattern match"</a>
                    ": match on the validation result inside the handler to choose the failure response."
                </li>
            </ul>
        )
    }
}

#[page("/welcome")]
async fn welcome() -> Result {
    view! {
        shell(
            <h1>"Welcome!"</h1>
            <p><a href="/">"Back"</a></p>
        )
    }
}

// --- 1. The `Valid<T>` extractor --------------------------------------------

#[page("/extractor")]
async fn extractor_page() -> Result {
    view! {
        shell(
            <h1>"Sign up"</h1>
            signup_form(action: "/extractor", email: "", errors: None)
        )
    }
}

// The extractor validates before the handler runs, so an invalid submission
// never reaches this code: it gets a 400 Bad Request with one
// "- field: message" line per error.
#[route(POST "/extractor")]
async fn extractor_signup(Valid(_form): Valid<SignUp>) -> Result<SeeOther> {
    // A real application would create the account here.
    Ok(see_other("/welcome"))
}

// --- 2. Pattern-matching on the validation result ----------------------------

#[page("/manual")]
async fn manual_page() -> Result {
    view! {
        shell(
            <h1>"Sign up"</h1>
            signup_form(action: "/manual", email: "", errors: None)
        )
    }
}

// The `Data` extractor buffers the body without validating, so the handler
// can pattern-match on the result and decide what a failure looks like: here
// the form is re-rendered with the errors next to their fields.
#[route(POST "/manual")]
async fn manual_signup(cx: &Cx, data: Data) -> Result<Response> {
    match SignUp::validate(&data) {
        // A real application would create the account here.
        Ok(_) => see_other("/welcome").into_response(cx),
        Err(errors) => {
            let email = data.get("email").unwrap_or_default();
            view! {
                (StatusCode::BAD_REQUEST)
                shell(
                    <h1>"Sign up"</h1>
                    signup_form(action: "/manual", email: email, errors: Some(&errors))
                )
            }
            .into_response(cx)
        }
    }
}


#[component]
async fn signup_form(action: &str, email: &str, errors: Option<&ValidationErrors>) -> Result {
    let email_error = errors.and_then(|errors| errors.get("email"));
    let password_error = errors.and_then(|errors| errors.get("password"));

    view! {
        <form method="POST" action=(action)>
            <div>
                <input name="email" type="email" placeholder="Email" value=(email)>
                if let Some(error) = email_error {
                    <p style="color: red;">(error.message())</p>
                }
            </div>
            <div>
                <input name="password" type="password" placeholder="Password">
                if let Some(error) = password_error {
                    <p style="color: red;">(error.message())</p>
                }
            </div>
            <button>"Sign up"</button>
        </form>
    }
}
