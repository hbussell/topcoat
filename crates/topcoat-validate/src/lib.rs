#![cfg_attr(docsrs, feature(doc_cfg))]
#![doc = include_str!("../docs/schema.md")]

extern crate self as topcoat_validate;

pub mod data;
pub mod descriptor;
pub mod error;
#[cfg(feature = "router")]
pub mod extract;
pub mod schema;
pub mod validator;
pub mod value;

pub use data::*;
pub use descriptor::*;
pub use error::*;
#[cfg(feature = "router")]
pub use extract::*;
pub use schema::*;
pub use validator::*;
pub use value::*;
