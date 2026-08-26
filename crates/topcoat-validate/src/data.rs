//! The `ValidationData` trait and implementations for common input sources.

use std::{collections::HashMap, hash::BuildHasher};

use crate::value::{Number, Value};

/// A source of data that can be validated against a schema.
pub trait ValidationData: Send + Sync {
    /// Return the value for a single field, if present.
    fn field(&self, name: &str) -> Option<Value>;

    /// Return a nested view of the data for a field.
    ///
    /// For flat sources this collects dotted keys (`address.city` becomes
    /// `city` inside the returned map). For naturally nested sources this
    /// defaults to `field`.
    fn nested(&self, name: &str) -> Option<Value> {
        self.field(name)
    }
}

impl<S: BuildHasher + Send + Sync> ValidationData for HashMap<String, String, S> {
    fn field(&self, name: &str) -> Option<Value> {
        self.get(name).map(|value| string_to_value(value))
    }

    fn nested(&self, name: &str) -> Option<Value> {
        nested_map(
            name,
            self.iter()
                .map(|(key, value)| (key.as_str(), string_to_value(value))),
        )
    }
}

impl<S: BuildHasher + Send + Sync> ValidationData for HashMap<String, Value, S> {
    fn field(&self, name: &str) -> Option<Value> {
        self.get(name).map(|value| value_to_value(value.clone()))
    }

    fn nested(&self, name: &str) -> Option<Value> {
        nested_map(
            name,
            self.iter()
                .map(|(key, value)| (key.as_str(), value_to_value(value.clone()))),
        )
        // No dotted keys: fall back to a directly nested value.
        .or_else(|| self.field(name))
    }
}

impl ValidationData for Vec<(String, String)> {
    fn field(&self, name: &str) -> Option<Value> {
        let values: Vec<Value> = self
            .iter()
            .filter(|(key, _)| key == name)
            .map(|(_, value)| string_to_value(value))
            .collect();

        if values.is_empty() {
            None
        } else {
            Some(Value::List(values))
        }
    }

    fn nested(&self, name: &str) -> Option<Value> {
        nested_map(
            name,
            self.iter()
                .map(|(key, value)| (key.as_str(), string_to_value(value))),
        )
    }
}

impl ValidationData for serde_json::Value {
    fn field(&self, name: &str) -> Option<Value> {
        self.as_object()?.get(name).map(json_to_value)
    }
}

impl ValidationData for Value {
    fn field(&self, name: &str) -> Option<Value> {
        match self {
            Value::Map(map) => map.get(name).cloned().map(value_to_value),
            _ => None,
        }
    }

    fn nested(&self, name: &str) -> Option<Value> {
        let Value::Map(map) = self else {
            return None;
        };

        // Values produced by an earlier `nested` hop are already lists;
        // flatten them back into single occurrences before collecting.
        let entries = map
            .iter()
            .flat_map(|(key, value)| match value_to_value(value.clone()) {
                Value::List(list) => list
                    .into_iter()
                    .map(|value| (key.as_str(), value))
                    .collect(),
                value => vec![(key.as_str(), value)],
            });

        // No dotted keys: fall back to a directly nested value.
        nested_map(name, entries).or_else(|| self.field(name))
    }
}

impl<D: ValidationData + ?Sized> ValidationData for &D {
    fn field(&self, name: &str) -> Option<Value> {
        (*self).field(name)
    }

    fn nested(&self, name: &str) -> Option<Value> {
        (*self).nested(name)
    }
}

impl<D: ValidationData + ?Sized> ValidationData for Box<D> {
    fn field(&self, name: &str) -> Option<Value> {
        self.as_ref().field(name)
    }

    fn nested(&self, name: &str) -> Option<Value> {
        self.as_ref().nested(name)
    }
}

fn string_to_value(value: &str) -> Value {
    if value.is_empty() {
        Value::Missing
    } else {
        Value::String(value.to_string())
    }
}

/// Collect the dotted keys under `name` into a nested map, for flat data
/// sources. Every nested value is a list, since flat sources may repeat keys.
fn nested_map<'a>(name: &str, entries: impl Iterator<Item = (&'a str, Value)>) -> Option<Value> {
    let prefix = format!("{name}.");
    let mut map: HashMap<String, Vec<Value>> = HashMap::new();

    for (key, value) in entries {
        if let Some(rest) = key.strip_prefix(&prefix) {
            map.entry(rest.to_string()).or_default().push(value);
        }
    }

    if map.is_empty() {
        None
    } else {
        Some(Value::Map(
            map.into_iter()
                .map(|(key, values)| (key, Value::List(values)))
                .collect(),
        ))
    }
}

fn value_to_value(value: Value) -> Value {
    match value {
        Value::String(ref s) if s.is_empty() => Value::Missing,
        Value::Null => Value::Missing,
        other => other,
    }
}

fn json_to_value(value: &serde_json::Value) -> Value {
    // u64 values that don't fit in i64 are intentionally stored as f64.
    #![allow(clippy::cast_precision_loss)]
    match value {
        serde_json::Value::Null => Value::Missing,
        serde_json::Value::Bool(b) => Value::Bool(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::Number(Number::Integer(i))
            } else if let Some(u) = n.as_u64() {
                if let Ok(i) = i64::try_from(u) {
                    Value::Number(Number::Integer(i))
                } else {
                    Value::Number(Number::Float(u as f64))
                }
            } else {
                Value::Number(Number::Float(n.as_f64().unwrap_or(f64::NAN)))
            }
        }
        serde_json::Value::String(s) => string_to_value(s),
        serde_json::Value::Array(items) => Value::List(items.iter().map(json_to_value).collect()),
        serde_json::Value::Object(map) => Value::Map(
            map.iter()
                .map(|(k, v)| (k.clone(), json_to_value(v)))
                .collect(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_map_string_fields() {
        let mut map = HashMap::new();
        map.insert("name".to_string(), "Alice".to_string());
        map.insert("empty".to_string(), String::new());

        assert_eq!(map.field("name"), Some(Value::from("Alice")));
        assert_eq!(map.field("empty"), Some(Value::Missing));
        assert_eq!(map.field("missing"), None);
    }

    #[test]
    fn hash_map_value_fields() {
        let mut map = HashMap::new();
        map.insert("present".to_string(), Value::from("ok"));
        map.insert("null".to_string(), Value::Null);
        map.insert("empty".to_string(), Value::from(""));
        map.insert(
            "list".to_string(),
            Value::List(vec![Value::from(1), Value::from(2)]),
        );

        assert_eq!(map.field("present"), Some(Value::from("ok")));
        assert_eq!(map.field("null"), Some(Value::Missing));
        assert_eq!(map.field("empty"), Some(Value::Missing));
        assert_eq!(
            map.field("list"),
            Some(Value::List(vec![Value::from(1), Value::from(2)]))
        );
    }

    #[test]
    fn pair_list_repeated_keys() {
        let data = vec![
            ("tag".to_string(), "a".to_string()),
            ("tag".to_string(), "b".to_string()),
            ("single".to_string(), "x".to_string()),
            ("empty".to_string(), String::new()),
        ];

        assert_eq!(
            data.field("tag"),
            Some(Value::List(vec![Value::from("a"), Value::from("b")]))
        );
        assert_eq!(
            data.field("single"),
            Some(Value::List(vec![Value::from("x")]))
        );
        assert_eq!(data.field("empty"), Some(Value::List(vec![Value::Missing])));
        assert_eq!(data.field("missing"), None);
    }

    #[test]
    fn pair_list_dotted_nested() {
        let data = vec![
            ("address.city".to_string(), "Sydney".to_string()),
            ("address.zip".to_string(), "2000".to_string()),
            ("name".to_string(), "Alice".to_string()),
        ];

        let nested = data.nested("address").expect("nested address data");
        let map = nested.as_map().expect("nested data is a map");
        assert_eq!(
            map.get("city"),
            Some(&Value::List(vec![Value::from("Sydney")]))
        );
        assert_eq!(
            map.get("zip"),
            Some(&Value::List(vec![Value::from("2000")]))
        );
        assert!(map.get("name").is_none());
    }

    #[test]
    fn pair_list_nested_missing() {
        let data: Vec<(String, String)> = vec![];
        assert_eq!(data.nested("address"), None);
    }

    #[test]
    fn pair_list_two_level_dotted_nested() {
        let data = vec![("address.geo.lat".to_string(), "-33.86".to_string())];

        let address = data.nested("address").expect("nested address data");
        let geo = address
            .nested("geo")
            .expect("nested geo data one level down");
        let map = geo.as_map().expect("nested geo is a map");
        assert_eq!(
            map.get("lat"),
            Some(&Value::List(vec![Value::from("-33.86")]))
        );
    }

    #[test]
    fn value_nested_collects_dotted_keys() {
        let mut map = HashMap::new();
        map.insert("geo.lat".to_string(), Value::from("-33.86"));
        let value = Value::Map(map);

        let geo = value.nested("geo").expect("nested geo data");
        let geo_map = geo.as_map().expect("nested geo is a map");
        assert_eq!(
            geo_map.get("lat"),
            Some(&Value::List(vec![Value::from("-33.86")]))
        );
    }

    #[test]
    fn value_nested_falls_back_to_direct_key() {
        let mut inner = HashMap::new();
        inner.insert("city".to_string(), Value::from("Sydney"));
        let mut map = HashMap::new();
        map.insert("address".to_string(), Value::Map(inner));
        let value = Value::Map(map);

        let address = value.nested("address").expect("nested address data");
        let address_map = address.as_map().expect("nested address is a map");
        assert_eq!(address_map.get("city"), Some(&Value::from("Sydney")));
    }

    #[test]
    fn value_nested_on_non_map_is_none() {
        assert_eq!(Value::from("text").nested("address"), None);
    }

    #[test]
    fn json_fields_and_normalization() {
        let json = serde_json::json!({
            "name": "Alice",
            "age": 30,
            "missing": null,
            "empty": "",
            "pi": 3.15,
        });

        assert_eq!(json.field("name"), Some(Value::from("Alice")));
        assert_eq!(json.field("age"), Some(Value::Number(Number::Integer(30))));
        assert_eq!(json.field("missing"), Some(Value::Missing));
        assert_eq!(json.field("empty"), Some(Value::Missing));
        assert_eq!(json.field("pi"), Some(Value::Number(Number::Float(3.15))));
        assert_eq!(json.field("absent"), None);
    }

    #[test]
    fn json_nested_object() {
        let json = serde_json::json!({
            "address": {
                "city": "Sydney",
                "zip": "2000",
            },
        });

        let nested = json.nested("address").expect("nested address");
        let map = nested.as_map().expect("nested map");
        assert_eq!(map.get("city"), Some(&Value::from("Sydney")));
        assert_eq!(map.get("zip"), Some(&Value::from("2000")));
    }

    #[test]
    fn hash_map_string_dotted_nested() {
        let mut map = HashMap::new();
        map.insert("address.city".to_string(), "Sydney".to_string());
        map.insert("address.zip".to_string(), "2000".to_string());
        map.insert("name".to_string(), "Alice".to_string());

        let nested = map.nested("address").expect("nested address data");
        let nested_map = nested.as_map().expect("nested data is a map");
        assert_eq!(
            nested_map.get("city"),
            Some(&Value::List(vec![Value::from("Sydney")]))
        );
        assert_eq!(
            nested_map.get("zip"),
            Some(&Value::List(vec![Value::from("2000")]))
        );
        assert!(nested_map.get("name").is_none());
    }

    #[test]
    fn hash_map_value_dotted_nested() {
        let mut map = HashMap::new();
        map.insert("address.city".to_string(), Value::from("Sydney"));
        map.insert("address.zip".to_string(), Value::from("2000"));

        let nested = map.nested("address").expect("nested address data");
        let nested_map = nested.as_map().expect("nested data is a map");
        assert_eq!(
            nested_map.get("city"),
            Some(&Value::List(vec![Value::from("Sydney")]))
        );
        assert_eq!(
            nested_map.get("zip"),
            Some(&Value::List(vec![Value::from("2000")]))
        );
    }

    #[test]
    fn hash_map_value_nested_falls_back_to_direct_key() {
        let mut inner = HashMap::new();
        inner.insert("city".to_string(), Value::from("Sydney"));
        let mut map: HashMap<String, Value> = HashMap::new();
        map.insert("address".to_string(), Value::Map(inner));

        let nested = map.nested("address").expect("nested address data");
        let nested_map = nested.as_map().expect("nested data is a map");
        assert_eq!(nested_map.get("city"), Some(&Value::from("Sydney")));
    }

    #[test]
    fn hash_map_nested_missing() {
        let map: HashMap<String, String> = HashMap::new();
        assert_eq!(map.nested("address"), None);
    }

    #[test]
    fn value_map_adapter() {
        let mut map = HashMap::new();
        map.insert("name".to_string(), Value::from("Alice"));
        map.insert("null".to_string(), Value::Null);
        map.insert("empty".to_string(), Value::from(""));

        let value = Value::Map(map);
        assert_eq!(value.field("name"), Some(Value::from("Alice")));
        assert_eq!(value.field("null"), Some(Value::Missing));
        assert_eq!(value.field("empty"), Some(Value::Missing));
    }

    #[test]
    fn reference_forwarder() {
        let mut map = HashMap::new();
        map.insert("name".to_string(), "Alice".to_string());

        let data: &dyn ValidationData = &map;
        assert_eq!(data.field("name"), Some(Value::from("Alice")));
    }

    #[test]
    fn boxed_forwarder() {
        let mut map = HashMap::new();
        map.insert("name".to_string(), "Alice".to_string());

        let data: Box<dyn ValidationData> = Box::new(map);
        assert_eq!(data.field("name"), Some(Value::from("Alice")));
    }
}
