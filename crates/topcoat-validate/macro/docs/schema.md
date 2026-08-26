# `#[derive(Schema)]`

Derive `Schema` on a named struct to generate validation logic and a runtime descriptor.

# Field attributes

List attributes inside `#[schema(...)]` on a struct field. They are evaluated in declaration order; the first validator failure for a field wins.

- `email`: value must look like an email address.
- `min_length = N`: string must have at least `N` characters.
- `max_length = N`: string must have at most `N` characters.
- `min = N`: number must be at least `N`.
- `max = N`: number must be at most `N`.
- `range(min = N, max = M)`: number must be inside the inclusive range.
- `one_of = "a, b, c"`: string must be one of the comma-separated choices.
- `regex = "..."`: string must match the pattern. Requires the `regex` feature.
- `trim`: trim whitespace before other checks.
- `string`, `number`, `bool`: explicit coercion hints.
- `custom = path::ToType`: run the given `CustomValidator` implementation.
- `rename = "field_name"`: read the field under a different key.
- `default = expr`: use the expression when the field is missing.

# Type inference

The macro infers the expected shape from the Rust type. Unsupported types fail at compile time with a message naming the field.

| Rust type | Inferred shape |
|-----------|----------------|
| `String` | string |
| `u8`..`u64`, `i8`..`i64` | integer |
| `f32`, `f64` | float |
| `bool` | bool |
| `Option<T>` | optional `T` |
| `Vec<T>` | list of `T` |
| `T: Schema` | nested schema |

See the [`topcoat-validate` schema guide](https://github.com/tokio-rs/topcoat/blob/main/crates/topcoat-validate/docs/schema.md) for details on validation behavior, custom validators, nested schemas, and descriptors.
