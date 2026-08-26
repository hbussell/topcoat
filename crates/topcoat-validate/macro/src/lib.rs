#![cfg_attr(docsrs, feature(doc_cfg))]

use proc_macro::TokenStream;

mod schema;

#[doc = include_str!("../docs/schema.md")]
#[proc_macro_derive(Schema, attributes(schema))]
pub fn derive_schema(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as syn::DeriveInput);
    match schema::expand(input) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}
