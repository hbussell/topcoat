//! Runtime descriptions of schemas, fields, and validators.

use std::borrow::Cow;

use crate::value::Number;

/// A runtime description of a schema.
#[derive(Debug, Clone, PartialEq)]
pub struct SchemaDescriptor {
    /// The fields defined by the schema.
    pub fields: Vec<FieldDescriptor>,
}

/// A runtime description of a single schema field.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldDescriptor {
    /// The field name as it appears in validation data.
    pub name: Cow<'static, str>,
    /// The field's value type.
    pub ty: FieldType,
    /// The validators applied to the field, in declaration order.
    pub validators: Vec<ValidatorDescriptor>,
    /// Whether the field is required.
    pub required: bool,
}

/// The type of value a field accepts.
#[derive(Debug, Clone, PartialEq)]
pub enum FieldType {
    /// A string.
    String,
    /// A signed integer.
    Integer,
    /// A floating-point number.
    Float,
    /// A boolean.
    Bool,
    /// A list of values of an inner type.
    List(Box<FieldType>),
    /// A nested schema.
    Nested(SchemaDescriptor),
}

/// A runtime description of a single validator.
#[derive(Debug, Clone, PartialEq)]
pub enum ValidatorDescriptor {
    /// Validate the value as an email address.
    Email,
    /// Minimum string length in characters.
    MinLength(usize),
    /// Maximum string length in characters.
    MaxLength(usize),
    /// Minimum numeric value.
    Min(Number),
    /// Maximum numeric value.
    Max(Number),
    /// Numeric range, inclusive.
    Range { min: Number, max: Number },
    /// One of a fixed set of string values.
    OneOf(&'static [&'static str]),
    /// A regular expression pattern.
    Regex(&'static str),
    /// Trim whitespace before further checks.
    Trim,
    /// A custom validator.
    Custom(CustomDescriptor),
    /// A default value is used when the field is missing.
    Default,
}

/// A runtime description of a custom validator.
#[derive(Debug, Clone, PartialEq)]
pub struct CustomDescriptor {
    /// Stable machine-readable name.
    pub name: &'static str,
    /// Default human-readable message.
    pub message: Cow<'static, str>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptor_construction() {
        let descriptor = SchemaDescriptor {
            fields: vec![FieldDescriptor {
                name: Cow::Borrowed("email"),
                ty: FieldType::String,
                validators: vec![
                    ValidatorDescriptor::Email,
                    ValidatorDescriptor::MaxLength(254),
                ],
                required: true,
            }],
        };

        assert_eq!(descriptor.fields.len(), 1);
        assert_eq!(descriptor.fields[0].name, "email");
        assert!(descriptor.fields[0].required);
    }

    #[test]
    fn nested_descriptor_round_trip() {
        let inner = SchemaDescriptor {
            fields: vec![FieldDescriptor {
                name: Cow::Borrowed("city"),
                ty: FieldType::String,
                validators: vec![ValidatorDescriptor::MinLength(1)],
                required: true,
            }],
        };

        let outer = SchemaDescriptor {
            fields: vec![FieldDescriptor {
                name: Cow::Borrowed("address"),
                ty: FieldType::Nested(inner.clone()),
                validators: vec![],
                required: true,
            }],
        };

        if let FieldType::Nested(nested) = &outer.fields[0].ty {
            assert_eq!(nested, &inner);
        } else {
            panic!("expected nested field type");
        }
    }
}
