//! Request input validation.
//!
//! Implement [`Validate`] for parsed types and use [`ValidationErrors`] to
//! collect failures. The [`Validated`] extractor wraps
//! [`Form`](crate::content::Form) and [`Json`](crate::content::Json) to reject
//! invalid input with `422`.

mod error;
pub mod rules;
mod validated;

pub use error::*;
pub use validated::*;
