//! The `Value` type and its `Number` variant.

use std::collections::HashMap;

/// A runtime value coming from form, JSON, or another validation data source.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// The field was absent, empty, or JSON `null`.
    Missing,
    /// An explicit JSON `null`.
    Null,
    /// A boolean.
    Bool(bool),
    /// A number.
    Number(Number),
    /// A string.
    String(String),
    /// A list of values.
    List(Vec<Value>),
    /// A map of string keys to values.
    Map(HashMap<String, Value>),
}

/// A numeric value.
#[derive(Debug, Clone, Copy)]
pub enum Number {
    /// A signed integer.
    Integer(i64),
    /// A floating-point value.
    Float(f64),
}

#[allow(clippy::cast_precision_loss)] // intentional integer-to-float equality
impl PartialEq for Number {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Number::Integer(a), Number::Integer(b)) => a == b,
            (Number::Float(a), Number::Float(b)) => a == b,
            (Number::Integer(a), Number::Float(b)) => (*a as f64) == *b,
            (Number::Float(a), Number::Integer(b)) => *a == (*b as f64),
        }
    }
}

impl Value {
    /// Return true if the value represents a missing field.
    #[must_use]
    pub fn is_missing(&self) -> bool {
        matches!(self, Value::Missing)
    }

    /// View the value as a string.
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }

    /// View the value as a number.
    #[must_use]
    pub fn as_number(&self) -> Option<Number> {
        match self {
            Value::Number(n) => Some(*n),
            _ => None,
        }
    }

    /// View the value as a boolean.
    #[must_use]
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// View the value as a list.
    #[must_use]
    pub fn as_list(&self) -> Option<&Vec<Value>> {
        match self {
            Value::List(list) => Some(list),
            _ => None,
        }
    }

    /// View the value as a map.
    #[must_use]
    pub fn as_map(&self) -> Option<&HashMap<String, Value>> {
        match self {
            Value::Map(map) => Some(map),
            _ => None,
        }
    }
}

impl From<Number> for Value {
    fn from(value: Number) -> Self {
        Value::Number(value)
    }
}

impl From<&str> for Value {
    fn from(value: &str) -> Self {
        Value::String(value.to_string())
    }
}

impl From<String> for Value {
    fn from(value: String) -> Self {
        Value::String(value)
    }
}

impl From<bool> for Value {
    fn from(value: bool) -> Self {
        Value::Bool(value)
    }
}

impl From<i8> for Number {
    fn from(value: i8) -> Self {
        Number::Integer(value.into())
    }
}

impl From<i16> for Number {
    fn from(value: i16) -> Self {
        Number::Integer(value.into())
    }
}

impl From<i32> for Number {
    fn from(value: i32) -> Self {
        Number::Integer(value.into())
    }
}

impl From<i64> for Number {
    fn from(value: i64) -> Self {
        Number::Integer(value)
    }
}

impl From<u8> for Number {
    fn from(value: u8) -> Self {
        Number::Integer(value.into())
    }
}

impl From<u16> for Number {
    fn from(value: u16) -> Self {
        Number::Integer(value.into())
    }
}

impl From<u32> for Number {
    fn from(value: u32) -> Self {
        Number::Integer(value.into())
    }
}

impl From<u64> for Number {
    fn from(value: u64) -> Self {
        Number::Integer(
            i64::try_from(value).unwrap_or_else(|_| {
                panic!("u64 value {value} is too large for a validation number")
            }),
        )
    }
}

impl From<f32> for Number {
    fn from(value: f32) -> Self {
        Number::Float(value.into())
    }
}

impl From<f64> for Number {
    fn from(value: f64) -> Self {
        Number::Float(value)
    }
}

macro_rules! value_from_int {
    ($ty:ty) => {
        impl From<$ty> for Value {
            fn from(value: $ty) -> Self {
                Value::Number(value.into())
            }
        }
    };
}

value_from_int!(i8);
value_from_int!(i16);
value_from_int!(i32);
value_from_int!(i64);
value_from_int!(u8);
value_from_int!(u16);
value_from_int!(u32);
value_from_int!(u64);

impl From<f32> for Value {
    fn from(value: f32) -> Self {
        Value::Number(Number::Float(value.into()))
    }
}

impl From<f64> for Value {
    fn from(value: f64) -> Self {
        Value::Number(Number::Float(value))
    }
}

impl From<Vec<Value>> for Value {
    fn from(value: Vec<Value>) -> Self {
        Value::List(value)
    }
}

impl From<HashMap<String, Value>> for Value {
    fn from(value: HashMap<String, Value>) -> Self {
        Value::Map(value)
    }
}

impl<T: Into<Value>> From<Option<T>> for Value {
    fn from(value: Option<T>) -> Self {
        match value {
            Some(v) => v.into(),
            None => Value::Missing,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_constructors() {
        let v = Value::String("hello".to_string());
        assert_eq!(v.as_str(), Some("hello"));
        assert!(v.as_number().is_none());

        assert_eq!(Value::Bool(true).as_bool(), Some(true));
        assert!(Value::Missing.is_missing());
        assert!(!Value::Null.is_missing());
    }

    #[test]
    fn number_integer_from_primitives() {
        assert_eq!(Number::from(5i8), Number::Integer(5));
        assert_eq!(Number::from(5i16), Number::Integer(5));
        assert_eq!(Number::from(5i32), Number::Integer(5));
        assert_eq!(Number::from(5i64), Number::Integer(5));
        assert_eq!(Number::from(5u8), Number::Integer(5));
        assert_eq!(Number::from(5u16), Number::Integer(5));
        assert_eq!(Number::from(5u32), Number::Integer(5));
        assert_eq!(Number::from(5u64), Number::Integer(5));
    }

    #[test]
    fn number_float_from_primitives() {
        assert_eq!(Number::from(1.5f32), Number::Float(1.5));
        assert_eq!(Number::from(1.5f64), Number::Float(1.5));
    }

    #[test]
    fn value_from_conversions() {
        assert_eq!(Value::from("text"), Value::String("text".to_string()));
        assert_eq!(
            Value::from("text".to_string()),
            Value::String("text".to_string())
        );
        assert_eq!(Value::from(true), Value::Bool(true));
        assert_eq!(Value::from(42i32), Value::Number(Number::Integer(42)));
        assert_eq!(Value::from(1.5f64), Value::Number(Number::Float(1.5)));
    }

    #[test]
    fn value_from_option() {
        assert_eq!(Value::from(Some(7u32)), Value::Number(Number::Integer(7)));
        let none: Option<i32> = None;
        assert_eq!(Value::from(none), Value::Missing);
    }

    #[test]
    fn value_from_list_and_map() {
        let list = Value::from(vec![Value::from(1), Value::from(2)]);
        assert_eq!(list.as_list().unwrap().len(), 2);

        let mut map = HashMap::new();
        map.insert("key".to_string(), Value::from("value"));
        let map_value = Value::from(map);
        assert_eq!(map_value.as_map().unwrap()["key"].as_str(), Some("value"));
    }

    #[test]
    fn accessors_return_none_for_wrong_variant() {
        let value = Value::from(42);
        assert!(value.as_str().is_none());
        assert!(value.as_bool().is_none());
        assert!(value.as_list().is_none());
        assert!(value.as_map().is_none());
    }

    #[test]
    #[should_panic(expected = "is too large for a validation number")]
    fn u64_too_large_panics() {
        let _ = Number::from(u64::MAX);
    }
}
