//! Built-in validation rules.
//!
//! Each rule is a pure function that returns `true` when the value is valid.
//! Use them with [`ValidationErrors::check_with_code`](super::ValidationErrors::check_with_code)
//! and the matching `CODE_*` constant.

/// Code for the [`required`](fn@required) rule.
pub const CODE_REQUIRED: &str = "required";

/// Code for the [`min_length`](fn@min_length) rule.
pub const CODE_TOO_SHORT: &str = "too_short";

/// Code for the [`max_length`](fn@max_length) rule.
pub const CODE_TOO_LONG: &str = "too_long";

/// Code for the [`range`](fn@range) rule.
pub const CODE_OUT_OF_RANGE: &str = "out_of_range";

/// Code for the [`email`](fn@email) rule.
pub const CODE_INVALID_EMAIL: &str = "invalid_email";

/// Returns `true` when `value` is not empty after trimming whitespace.
#[must_use]
pub fn required(value: &str) -> bool {
    !value.trim().is_empty()
}

/// Returns `true` when `value` has at least `min` Unicode scalars.
#[must_use]
pub fn min_length(value: &str, min: usize) -> bool {
    value.chars().count() >= min
}

/// Returns `true` when `value` has at most `max` Unicode scalars.
#[must_use]
pub fn max_length(value: &str, max: usize) -> bool {
    value.chars().count() <= max
}

/// Returns `true` when `value` is between `min` and `max`, inclusive.
#[must_use]
pub fn range<T>(value: T, min: T, max: T) -> bool
where
    T: PartialOrd + Copy,
{
    value >= min && value <= max
}

/// Returns `true` when `value` looks like an email address.
///
/// This is a deliberately simple check: it requires a non-empty local part, an
/// `@` separator, and a domain part that contains at least one dot and no
/// leading, trailing, or doubled dots.
#[must_use]
pub fn email(value: &str) -> bool {
    let Some((local, domain)) = value.split_once('@') else {
        return false;
    };

    if local.is_empty() || domain.is_empty() {
        return false;
    }

    if domain.starts_with('.') || domain.ends_with('.') || domain.contains("..") {
        return false;
    }

    domain.contains('.')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn required_rejects_empty_or_whitespace_only_string() {
        assert!(!required(""));
        assert!(!required(" "));
        assert!(!required("  "));
        assert!(required("Ada"));
        assert!(required(" Ada "));
    }

    #[test]
    fn min_length_counts_unicode_scalars() {
        assert!(min_length("hello", 5));
        assert!(!min_length("hi", 5));
        assert!(min_length("🎉", 1));
    }

    #[test]
    fn max_length_counts_unicode_scalars() {
        assert!(max_length("hello", 5));
        assert!(!max_length("hello", 4));
        assert!(max_length("🎉", 1));
    }

    #[test]
    fn range_includes_bounds() {
        assert!(range(5, 1, 10));
        assert!(range(1, 1, 10));
        assert!(range(10, 1, 10));
        assert!(!range(0, 1, 10));
        assert!(!range(11, 1, 10));
    }

    #[test]
    fn email_accepts_valid_addresses() {
        assert!(email("ada@example.com"));
        assert!(email("a+b@example.co.uk"));
    }

    #[test]
    fn email_rejects_invalid_addresses() {
        assert!(!email(""));
        assert!(!email("ada"));
        assert!(!email("ada@"));
        assert!(!email("@example.com"));
        assert!(!email("ada@example"));
        assert!(!email("ada@.example.com"));
        assert!(!email("ada@example.com."));
        assert!(!email("ada@example..com"));
    }
}
