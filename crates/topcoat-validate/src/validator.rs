//! Built-in validators and coercion helpers.

use std::{borrow::Cow, cmp::Ordering};

use crate::{ValidationError, Value, value::Number};

/// A user-defined validator type.
///
/// Implement this trait on a unit struct and reference the type in
/// `#[schema(custom = MyValidator)]`. The validator is invoked through its
/// associated functions, so no instance is required.
pub trait CustomValidator: Send + Sync {
    /// Validate or transform the value.
    ///
    /// # Errors
    ///
    /// Returns a `ValidationError` when the value fails the custom validation.
    fn validate(value: &Value) -> Result<Value, ValidationError>;

    /// Machine-readable name used in the descriptor.
    fn name() -> &'static str;

    /// Default human-readable message shown when validation fails.
    #[must_use]
    fn message() -> Cow<'static, str> {
        Cow::Borrowed("Invalid value")
    }
}

/// Trim whitespace from a string value.
///
/// Other values are returned unchanged. For a list, each element is trimmed.
pub fn trim(value: Value) -> Value {
    match value {
        Value::String(s) => Value::String(s.trim().to_string()),
        Value::List(list) => Value::List(list.into_iter().map(trim).collect()),
        other => other,
    }
}

/// Return true if the value is missing, looking through a single-element
/// list.
///
/// Pair-list data sources such as `Vec<(String, String)>` wrap every field in
/// a list, so a scalar field checks for a missing value through that wrapper.
/// List fields use [`Value::is_missing`] directly, since for them the wrapper
/// is the value.
#[must_use]
pub fn is_missing(value: &Value) -> bool {
    match value {
        Value::List(list) if list.len() == 1 => list[0].is_missing(),
        value => value.is_missing(),
    }
}

/// Coerce a value into a string.
///
/// A single-element list is unwrapped first, so scalar fields from form pair
/// lists work as expected.
///
/// # Errors
///
/// Returns a `ValidationError` when the value cannot be coerced to a string.
pub fn string(value: &Value) -> Result<Value, ValidationError> {
    let value = one(value)?;
    match value {
        Value::String(s) => Ok(Value::String(s.clone())),
        _ => Err(ValidationError::with_code(
            "string",
            "Value must be a string",
        )),
    }
}

/// Coerce a value into an integer.
///
/// A single-element list is unwrapped first.
///
/// # Errors
///
/// Returns a `ValidationError` when the value cannot be coerced to an integer.
#[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)] // intentional integer range check and conversion
pub fn integer(value: &Value) -> Result<Value, ValidationError> {
    let value = one(value)?;
    match value {
        Value::Number(Number::Integer(i)) => Ok(Value::Number(Number::Integer(*i))),
        Value::Number(Number::Float(f)) => {
            if f.is_finite() && f.fract() == 0.0 && *f >= i64::MIN as f64 && *f <= i64::MAX as f64 {
                return Ok(Value::Number(Number::Integer(*f as i64)));
            }
            Err(ValidationError::with_code(
                "integer",
                "Value must be an integer",
            ))
        }
        Value::String(s) => match s.parse::<i64>() {
            Ok(i) => Ok(Value::Number(Number::Integer(i))),
            Err(_) => Err(ValidationError::with_code(
                "integer",
                "Value must be an integer",
            )),
        },
        _ => Err(ValidationError::with_code(
            "integer",
            "Value must be an integer",
        )),
    }
}

/// Coerce a value into a floating-point number.
///
/// A single-element list is unwrapped first.
///
/// # Errors
///
/// Returns a `ValidationError` when the value cannot be coerced to a number.
#[allow(clippy::cast_precision_loss)] // intentional integer-to-float conversion
pub fn float(value: &Value) -> Result<Value, ValidationError> {
    let value = one(value)?;
    match value {
        Value::Number(Number::Integer(i)) => Ok(Value::Number(Number::Float(*i as f64))),
        Value::Number(Number::Float(f)) => Ok(Value::Number(Number::Float(*f))),
        Value::String(s) => match s.parse::<f64>() {
            Ok(f) if f.is_finite() => Ok(Value::Number(Number::Float(f))),
            _ => Err(ValidationError::with_code(
                "float",
                "Value must be a number",
            )),
        },
        _ => Err(ValidationError::with_code(
            "float",
            "Value must be a number",
        )),
    }
}

/// Coerce a value into a boolean.
///
/// Strings `"on"`, `"true"`, and `"1"` are true; `"off"`, `"false"`, and
/// `"0"` are false. JSON booleans are accepted directly. A single-element
/// list is unwrapped first.
///
/// # Errors
///
/// Returns a `ValidationError` when the value cannot be coerced to a boolean.
pub fn bool(value: &Value) -> Result<Value, ValidationError> {
    let value = one(value)?;
    match value {
        Value::Bool(b) => Ok(Value::Bool(*b)),
        Value::String(s) => match s.trim().to_ascii_lowercase().as_str() {
            "on" | "true" | "1" => Ok(Value::Bool(true)),
            "off" | "false" | "0" => Ok(Value::Bool(false)),
            _ => Err(ValidationError::with_code(
                "bool",
                "Value must be a boolean",
            )),
        },
        _ => Err(ValidationError::with_code(
            "bool",
            "Value must be a boolean",
        )),
    }
}

/// Validate that a value is a valid email address.
///
/// # Errors
///
/// Returns a `ValidationError` when the value is not a valid email address.
pub fn email(value: &Value) -> Result<(), ValidationError> {
    let s = require_string(value)?;
    if is_email(s) {
        Ok(())
    } else {
        Err(ValidationError::with_code(
            "email",
            "Value must be a valid email address",
        ))
    }
}

/// Validate that a string value has at least `min` characters.
///
/// # Errors
///
/// Returns a `ValidationError` when the value is shorter than `min`.
pub fn min_length(value: &Value, min: usize) -> Result<(), ValidationError> {
    let s = require_string(value)?;
    if s.chars().count() >= min {
        Ok(())
    } else {
        Err(ValidationError::with_code(
            "min_length",
            format!("Value must be at least {min} characters"),
        ))
    }
}

/// Validate that a string value has at most `max` characters.
///
/// # Errors
///
/// Returns a `ValidationError` when the value is longer than `max`.
pub fn max_length(value: &Value, max: usize) -> Result<(), ValidationError> {
    let s = require_string(value)?;
    if s.chars().count() <= max {
        Ok(())
    } else {
        Err(ValidationError::with_code(
            "max_length",
            format!("Value must be at most {max} characters"),
        ))
    }
}

/// Validate that a numeric value is at least `min`.
///
/// # Errors
///
/// Returns a `ValidationError` when the value is smaller than `min`.
pub fn min(value: &Value, min: Number) -> Result<(), ValidationError> {
    let number = require_number(value)?;
    if number >= min {
        Ok(())
    } else {
        Err(ValidationError::with_code("min", "Value is too small"))
    }
}

/// Validate that a numeric value is at most `max`.
///
/// # Errors
///
/// Returns a `ValidationError` when the value is larger than `max`.
pub fn max(value: &Value, max: Number) -> Result<(), ValidationError> {
    let number = require_number(value)?;
    if number <= max {
        Ok(())
    } else {
        Err(ValidationError::with_code("max", "Value is too large"))
    }
}

/// Validate that a numeric value is within an inclusive range.
///
/// # Errors
///
/// Returns a `ValidationError` when the value is outside the range.
pub fn range(value: &Value, min: Number, max: Number) -> Result<(), ValidationError> {
    let number = require_number(value)?;
    if number >= min && number <= max {
        Ok(())
    } else {
        Err(ValidationError::with_code(
            "range",
            "Value is outside the allowed range",
        ))
    }
}

/// Validate that a string value is one of the allowed choices.
///
/// # Errors
///
/// Returns a `ValidationError` when the value is not in `allowed`.
pub fn one_of(value: &Value, allowed: &'static [&'static str]) -> Result<(), ValidationError> {
    let s = require_string(value)?;
    if allowed.contains(&s) {
        Ok(())
    } else {
        Err(ValidationError::with_code(
            "one_of",
            "Value is not an allowed choice",
        ))
    }
}

/// Validate a string value against a regular expression pattern.
///
/// Patterns are compiled once per process and cached, so validation calls do
/// not pay for recompilation.
///
/// Available only when the `regex` feature is enabled.
///
/// # Errors
///
/// Returns a `ValidationError` when the pattern is invalid or the value does
/// not match.
#[cfg(feature = "regex")]
pub fn regex(value: &Value, pattern: &'static str) -> Result<(), ValidationError> {
    let s = require_string(value)?;
    if compile_regex(pattern)?.is_match(s) {
        Ok(())
    } else {
        Err(ValidationError::with_code(
            "regex",
            "Value does not match the required pattern",
        ))
    }
}

/// Compile `pattern`, caching the outcome. Schema patterns are static
/// strings, so each is compiled at most once per process.
#[cfg(feature = "regex")]
fn compile_regex(pattern: &'static str) -> Result<::regex::Regex, ValidationError> {
    use std::{
        collections::HashMap,
        sync::{Mutex, OnceLock},
    };

    use ::regex::Regex;

    static CACHE: OnceLock<Mutex<HashMap<&'static str, Result<Regex, ()>>>> = OnceLock::new();

    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut cache = cache
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    cache
        .entry(pattern)
        .or_insert_with(|| Regex::new(pattern).map_err(drop))
        .clone()
        .map_err(|()| ValidationError::with_code("regex", "Invalid regular expression"))
}

fn one(value: &Value) -> Result<&Value, ValidationError> {
    match value {
        Value::List(list) if list.len() == 1 => Ok(&list[0]),
        Value::List(_) => Err(ValidationError::with_code(
            "value",
            "Expected a single value",
        )),
        _ => Ok(value),
    }
}

fn require_string(value: &Value) -> Result<&str, ValidationError> {
    let value = one(value)?;
    match value {
        Value::String(s) => Ok(s),
        _ => Err(ValidationError::with_code(
            "string",
            "Value must be a string",
        )),
    }
}

fn require_number(value: &Value) -> Result<Number, ValidationError> {
    let value = one(value)?;
    match value {
        Value::Number(n) => Ok(*n),
        _ => Err(ValidationError::with_code(
            "number",
            "Value must be a number",
        )),
    }
}

fn is_email(s: &str) -> bool {
    let mut parts = s.splitn(2, '@');
    let local = parts.next();
    let domain = parts.next();

    if let (Some(local), Some(domain)) = (local, domain) {
        !local.is_empty()
            && !domain.is_empty()
            && domain.contains('.')
            && !domain.starts_with('.')
            && !domain.ends_with('.')
            && !local.contains(' ')
            && !domain.contains(' ')
    } else {
        false
    }
}

#[allow(clippy::cast_precision_loss)] // intentional integer-to-float comparison
impl PartialOrd for Number {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        match (self, other) {
            (Number::Integer(a), Number::Integer(b)) => a.partial_cmp(b),
            (Number::Float(a), Number::Float(b)) => a.partial_cmp(b),
            (Number::Integer(a), Number::Float(b)) => (*a as f64).partial_cmp(b),
            (Number::Float(a), Number::Integer(b)) => a.partial_cmp(&(*b as f64)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_missing_looks_through_single_element_lists() {
        assert!(is_missing(&Value::Missing));
        assert!(is_missing(&Value::List(vec![Value::Missing])));
        assert!(!is_missing(&Value::from("x")));
        assert!(!is_missing(&Value::List(vec![Value::from("x")])));
        assert!(!is_missing(&Value::List(vec![])));
        assert!(!is_missing(&Value::List(vec![
            Value::Missing,
            Value::Missing
        ])));
    }

    #[test]
    fn string_coerces_from_single_element_list() {
        assert_eq!(
            string(&Value::List(vec![Value::from("hello")])).unwrap(),
            Value::from("hello")
        );
        assert!(string(&Value::List(vec![Value::from("a"), Value::from("b")])).is_err());
    }

    #[test]
    fn string_rejects_non_string() {
        assert!(string(&Value::from(42)).is_err());
    }

    #[test]
    fn integer_coerces_strings() {
        assert_eq!(
            integer(&Value::from("42")).unwrap(),
            Value::Number(Number::Integer(42))
        );
        assert_eq!(
            integer(&Value::Number(Number::Float(42.0))).unwrap(),
            Value::Number(Number::Integer(42))
        );
        assert!(integer(&Value::from("13.5")).is_err());
        assert!(integer(&Value::from("not a number")).is_err());
    }

    #[test]
    fn float_coerces_strings() {
        assert_eq!(
            float(&Value::from("1.5")).unwrap(),
            Value::Number(Number::Float(1.5))
        );
        assert_eq!(
            float(&Value::Number(Number::Integer(2))).unwrap(),
            Value::Number(Number::Float(2.0))
        );
    }

    #[test]
    fn bool_coerces_strings() {
        assert_eq!(bool(&Value::from("on")).unwrap(), Value::Bool(true));
        assert_eq!(bool(&Value::from("true")).unwrap(), Value::Bool(true));
        assert_eq!(bool(&Value::from("1")).unwrap(), Value::Bool(true));
        assert_eq!(bool(&Value::from("false")).unwrap(), Value::Bool(false));
        assert_eq!(bool(&Value::from("0")).unwrap(), Value::Bool(false));
        assert!(bool(&Value::from("maybe")).is_err());
    }

    #[test]
    fn trim_before_validation() {
        let trimmed = trim(Value::from("  hello  "));
        assert_eq!(trimmed, Value::from("hello"));
        assert_eq!(
            trim(Value::List(vec![trimmed.clone()])),
            Value::List(vec![Value::from("hello")])
        );
    }

    #[test]
    fn email_validation() {
        assert!(email(&Value::from("user@example.com")).is_ok());
        assert!(email(&Value::from("not an email")).is_err());
        assert!(email(&Value::from("missing@domain")).is_err());
    }

    #[test]
    fn length_validators() {
        assert!(min_length(&Value::from("hi"), 2).is_ok());
        assert!(min_length(&Value::from("h"), 2).is_err());
        assert!(max_length(&Value::from("hello"), 5).is_ok());
        assert!(max_length(&Value::from("hello world"), 5).is_err());
    }

    #[test]
    fn numeric_bounds() {
        assert!(min(&Value::Number(Number::Integer(10)), Number::Integer(5)).is_ok());
        assert!(min(&Value::Number(Number::Integer(3)), Number::Integer(5)).is_err());
        assert!(max(&Value::Number(Number::Integer(3)), Number::Integer(5)).is_ok());
        assert!(max(&Value::Number(Number::Integer(10)), Number::Integer(5)).is_err());
        assert!(
            range(
                &Value::Number(Number::Integer(7)),
                Number::Integer(5),
                Number::Integer(10)
            )
            .is_ok()
        );
        assert!(
            range(
                &Value::Number(Number::Integer(12)),
                Number::Integer(5),
                Number::Integer(10)
            )
            .is_err()
        );
    }

    #[test]
    fn one_of_validator() {
        assert!(one_of(&Value::from("a"), &["a", "b", "c"]).is_ok());
        assert!(one_of(&Value::from("d"), &["a", "b", "c"]).is_err());
    }

    #[test]
    #[cfg(feature = "regex")]
    fn regex_validator() {
        assert!(regex(&Value::from("abc123"), r"^[a-z0-9]+$").is_ok());
        assert!(regex(&Value::from("ABC"), r"^[a-z0-9]+$").is_err());
    }

    #[test]
    #[cfg(feature = "regex")]
    fn regex_caches_patterns_and_invalid_outcomes() {
        // Repeated calls with the same pattern reuse the compiled regex.
        for _ in 0..2 {
            assert!(regex(&Value::from("abc"), r"^[a-z]+$").is_ok());
            assert!(regex(&Value::from("ABC"), r"^[a-z]+$").is_err());
        }

        // An invalid pattern reports an error on every call.
        assert_eq!(
            regex(&Value::from("abc"), r"(").unwrap_err().code(),
            "regex"
        );
        assert_eq!(
            regex(&Value::from("abc"), r"(").unwrap_err().code(),
            "regex"
        );
    }

    #[test]
    fn custom_validator_trait() {
        struct AlwaysTrue;
        impl CustomValidator for AlwaysTrue {
            fn validate(_value: &Value) -> Result<Value, ValidationError> {
                Ok(Value::Bool(true))
            }
            fn name() -> &'static str {
                "always_true"
            }
            fn message() -> Cow<'static, str> {
                Cow::Borrowed("This always passes")
            }
        }

        assert_eq!(
            AlwaysTrue::validate(&Value::from(42)).unwrap(),
            Value::Bool(true)
        );
        assert_eq!(AlwaysTrue::name(), "always_true");
    }

    #[test]
    fn number_ordering() {
        assert!(Number::Integer(5) >= Number::Integer(5));
        assert!(Number::Integer(5) < Number::Float(5.5));
        assert_eq!(Number::Float(5.0), Number::Integer(5));
    }
}
