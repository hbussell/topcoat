//! The `Schema` trait.

use crate::{SchemaDescriptor, ValidationData, ValidationErrors};

/// A type that can be validated from untrusted data.
pub trait Schema: Sized {
    /// Validate a value from the supplied data.
    ///
    /// # Errors
    ///
    /// Returns a `ValidationErrors` value when the data does not satisfy the
    /// schema.
    fn validate<D: ValidationData>(data: &D) -> Result<Self, ValidationErrors>;

    /// Return a runtime description of the schema.
    fn descriptor() -> SchemaDescriptor;
}
