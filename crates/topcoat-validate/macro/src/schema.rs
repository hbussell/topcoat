//! Attribute parsing and code generation for `#[derive(Schema)]`.

use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::{
    Data, DeriveInput, Expr, Ident, Lit, LitInt, LitStr, Path, Token, Type,
    parse::{Parse, ParseStream},
    parse_quote,
    punctuated::Punctuated,
};

pub fn expand(input: DeriveInput) -> syn::Result<TokenStream> {
    let schema = SchemaInput::parse(input)?;
    schema.expand()
}

struct SchemaInput {
    ident: Ident,
    generics: syn::Generics,
    fields: Vec<Field>,
}

struct Field {
    ident: Ident,
    ty: Type,
    attrs: Vec<FieldAttr>,
    default_expr: Option<Expr>,
}

enum FieldAttr {
    Email,
    MinLength { value: LitInt },
    MaxLength { value: LitInt },
    Min { value: LitNumber },
    Max { value: LitNumber },
    Range { min: LitNumber, max: LitNumber },
    OneOf { value: LitStr },
    Regex { value: LitStr },
    Trim,
    Custom { path: Path },
    String,
    Number,
    Bool,
    Rename { value: LitStr },
    Default { expr: Expr },
}

struct LitNumber {
    negative: bool,
    lit: Lit,
}

#[derive(Clone, Copy)]
enum ScalarType {
    String,
    Integer,
    F32,
    F64,
    Bool,
}

enum FieldShape {
    Scalar(ScalarType),
    Option(Box<FieldShape>),
    Vec(Box<FieldShape>),
    Nested(Box<Type>),
}

impl FieldShape {
    fn is_optional(&self) -> bool {
        matches!(self, FieldShape::Option(_))
    }

    fn is_nested(&self) -> bool {
        matches!(self, FieldShape::Nested(..))
    }

    /// True when the data source must provide a nested object for this
    /// shape: a nested schema, or an optional one. Such fields are read with
    /// `nested` so flat sources collect their dotted keys.
    fn is_nested_object(&self) -> bool {
        match self {
            FieldShape::Nested(..) => true,
            FieldShape::Option(inner) => inner.is_nested(),
            _ => false,
        }
    }
}

impl SchemaInput {
    fn parse(input: DeriveInput) -> syn::Result<Self> {
        let ident = input.ident;
        let generics = input.generics;
        let Data::Struct(data) = input.data else {
            return Err(syn::Error::new_spanned(
                &ident,
                "Schema can only be derived for structs",
            ));
        };

        let mut fields = Vec::new();
        for field in data.fields {
            let ident = field.ident.clone().ok_or_else(|| {
                syn::Error::new_spanned(&field, "Schema does not support tuple structs")
            })?;
            let mut attrs = Vec::new();
            let mut default_expr = None;
            for attr in &field.attrs {
                if attr.path().is_ident("schema") {
                    let parsed: FieldAttrs = attr.parse_args()?;
                    for item in parsed.items {
                        if let FieldAttr::Default { expr } = item {
                            default_expr = Some(expr);
                        } else {
                            attrs.push(item);
                        }
                    }
                }
            }
            fields.push(Field {
                ident,
                ty: field.ty,
                attrs,
                default_expr,
            });
        }

        Ok(Self {
            ident,
            generics,
            fields,
        })
    }

    fn expand(&self) -> syn::Result<TokenStream> {
        let tc = crate_path();
        let ident = &self.ident;
        let (impl_generics, ty_generics, where_clause) = self.generics.split_for_impl();

        let validate_fields: Vec<TokenStream> = self
            .fields
            .iter()
            .map(|field| field.generate_validate(&tc))
            .collect::<Result<_, _>>()?;
        let descriptor_fields: Vec<TokenStream> = self
            .fields
            .iter()
            .map(|field| field.generate_descriptor(&tc))
            .collect::<Result<_, _>>()?;
        let field_idents: Vec<&Ident> = self.fields.iter().map(|field| &field.ident).collect();

        Ok(quote! {
            #[automatically_derived]
            impl #impl_generics #tc::Schema for #ident #ty_generics #where_clause {
                fn validate<D: #tc::ValidationData>(__data: &D) -> ::std::result::Result<Self, #tc::ValidationErrors> {
                    let mut __errors = #tc::ValidationErrors::new();
                    #(#validate_fields)*

                    if __errors.is_empty() {
                        Ok(Self {
                            #(#field_idents: #field_idents.unwrap()),*
                        })
                    } else {
                        Err(__errors)
                    }
                }

                fn descriptor() -> #tc::SchemaDescriptor {
                    #tc::SchemaDescriptor {
                        fields: ::std::vec![#(#descriptor_fields),*],
                    }
                }
            }

            #[automatically_derived]
            impl #impl_generics #ident #ty_generics #where_clause {
                /// Validate a value from the supplied data.
                pub fn validate<D: #tc::ValidationData>(__data: &D) -> ::std::result::Result<Self, #tc::ValidationErrors> {
                    <Self as #tc::Schema>::validate(__data)
                }

                /// Return a runtime description of the schema.
                pub fn descriptor() -> #tc::SchemaDescriptor {
                    <Self as #tc::Schema>::descriptor()
                }
            }
        })
    }
}

impl Field {
    fn field_name(&self) -> String {
        self.attrs
            .iter()
            .find_map(|attr| match attr {
                FieldAttr::Rename { value } => Some(value.value()),
                _ => None,
            })
            .unwrap_or_else(|| self.ident.to_string())
    }

    fn has_trim(&self) -> bool {
        self.attrs
            .iter()
            .any(|attr| matches!(attr, FieldAttr::Trim))
    }

    fn has_default(&self) -> bool {
        self.default_expr.is_some()
    }

    fn coerce_attr(&self) -> Option<ScalarType> {
        self.attrs.iter().find_map(|attr| match attr {
            FieldAttr::String => Some(ScalarType::String),
            FieldAttr::Number => Some(ScalarType::Integer),
            FieldAttr::Bool => Some(ScalarType::Bool),
            _ => None,
        })
    }

    fn shape(&self) -> syn::Result<FieldShape> {
        infer_shape(&self.ty, self.coerce_attr().as_ref())
    }

    fn generate_validate(&self, tc: &syn::Path) -> syn::Result<TokenStream> {
        let field_ident = &self.ident;
        let field_name = self.field_name();
        let field_name_str = LitStr::new(&field_name, self.ident.span());
        let ty = &self.ty;
        let shape = self.shape()?;
        let is_optional = shape.is_optional();
        let default_expr = &self.default_expr;
        let trim = self.has_trim();

        let accessor = if shape.is_nested_object() {
            quote! { __data.nested(#field_name_str) }
        } else {
            quote! { __data.field(#field_name_str) }
        };

        let missing_handler = if is_optional {
            quote! { #field_ident = ::std::option::Option::Some(::std::option::Option::None); }
        } else if let Some(default) = default_expr {
            quote! { #field_ident = ::std::option::Option::Some(#default); }
        } else {
            quote! {
                __errors.push(
                    #field_name_str,
                    #tc::ValidationError::with_code("required", "This field is required"),
                );
            }
        };

        let present_validation =
            self.generate_present_validation(tc, &shape, field_ident, &field_name_str, trim)?;

        // A pair list wraps every field in a single-element list, so non-list
        // shapes check for a missing value through that wrapper. For `Vec`
        // fields the wrapper is the value itself.
        let is_missing = if matches!(shape, FieldShape::Vec(_)) {
            quote! { __raw.is_missing() }
        } else {
            quote! { #tc::validator::is_missing(&__raw) }
        };

        Ok(quote! {
            let mut #field_ident: ::std::option::Option<#ty> = ::std::option::Option::None;
            {
                let mut __raw = #accessor.unwrap_or_else(|| #tc::value::Value::Missing);
                if #is_missing {
                    #missing_handler
                } else {
                    #present_validation
                }
            }
        })
    }

    fn generate_present_validation(
        &self,
        tc: &syn::Path,
        shape: &FieldShape,
        field_ident: &Ident,
        field_name_str: &LitStr,
        trim: bool,
    ) -> syn::Result<TokenStream> {
        match shape {
            FieldShape::Scalar(scalar) => {
                self.generate_scalar_validation(tc, *scalar, field_ident, field_name_str, trim)
            }
            FieldShape::Option(inner) => {
                let inner_value = Ident::new("__option_value", Span::call_site());
                let field_name_path = quote! { #field_name_str };
                let inner_validation = self.generate_shape_block(
                    tc,
                    inner,
                    &inner_value,
                    &field_name_path,
                    &quote! { #field_ident = ::std::option::Option::Some(::std::option::Option::Some(__value)); },
                    None,
                    trim,
                )?;

                Ok(quote! {
                    let #inner_value = __raw;
                    #inner_validation
                })
            }
            FieldShape::Vec(inner) => {
                let item_var = Ident::new("__item", Span::call_site());
                let index_var = Ident::new("__index", Span::call_site());
                let result_var = Ident::new("__result", Span::call_site());
                let item_path = quote! {
                    ::std::format!("{}.{}", #field_name_str, #index_var)
                };
                let item_validation = self.generate_shape_block(
                    tc,
                    inner,
                    &item_var,
                    &item_path,
                    &quote! { #result_var.push(__value); },
                    None,
                    trim,
                )?;

                Ok(quote! {
                    match __raw {
                        #tc::value::Value::List(__list) => {
                            let mut #result_var = ::std::vec::Vec::with_capacity(__list.len());
                            for (#index_var, #item_var) in __list.into_iter().enumerate() {
                                #item_validation
                            }
                            #field_ident = ::std::option::Option::Some(#result_var);
                        }
                        _ => {
                            __errors.push(
                                #field_name_str,
                                #tc::ValidationError::with_code("list", "Value must be a list"),
                            );
                        }
                    }
                })
            }
            FieldShape::Nested(nested_ty) => Ok(quote! {
                match <#nested_ty as #tc::Schema>::validate(&__raw) {
                    ::std::result::Result::Ok(__value) => {
                        #field_ident = ::std::option::Option::Some(__value);
                    }
                    ::std::result::Result::Err(__nested_errors) => {
                        for (__path, __err) in __nested_errors.into_iter() {
                            __errors.push(
                                ::std::format!("{}.{}", #field_name_str, __path),
                                __err,
                            );
                        }
                    }
                }
            }),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn generate_shape_block(
        &self,
        tc: &syn::Path,
        shape: &FieldShape,
        value_var: &Ident,
        path_expr: &TokenStream,
        setter: &TokenStream,
        none_setter: Option<&TokenStream>,
        trim: bool,
    ) -> syn::Result<TokenStream> {
        match shape {
            FieldShape::Scalar(scalar) => {
                self.generate_scalar_block(tc, *scalar, value_var, path_expr, setter, trim)
            }
            FieldShape::Option(inner) => {
                let none_setter = none_setter.ok_or_else(|| {
                    syn::Error::new_spanned(
                        &self.ty,
                        "optional values are not supported inside lists or other optional values",
                    )
                })?;
                let inner_value = Ident::new("__option_inner", Span::call_site());
                let inner_block = self.generate_shape_block(
                    tc,
                    inner,
                    &inner_value,
                    path_expr,
                    setter,
                    None,
                    trim,
                )?;
                Ok(quote! {
                    if #value_var.is_missing() {
                        #none_setter
                    } else {
                        let #inner_value = #value_var;
                        #inner_block
                    }
                })
            }
            FieldShape::Vec(inner) => {
                let item_var = Ident::new("__item", Span::call_site());
                let index_var = Ident::new("__index", Span::call_site());
                let result_var = Ident::new("__result", Span::call_site());
                let item_path = quote! {
                    ::std::format!("{}.{}", #path_expr, #index_var)
                };
                let item_validation = self.generate_shape_block(
                    tc,
                    inner,
                    &item_var,
                    &item_path,
                    &quote! { #result_var.push(__value); },
                    None,
                    trim,
                )?;

                Ok(quote! {
                    match #value_var {
                        #tc::value::Value::List(__list) => {
                            let mut #result_var = ::std::vec::Vec::with_capacity(__list.len());
                            for (#index_var, #item_var) in __list.into_iter().enumerate() {
                                #item_validation
                            }
                            let __value = #result_var;
                            #setter
                        }
                        _ => {
                            __errors.push(
                                #path_expr.clone(),
                                #tc::ValidationError::with_code("list", "Value must be a list"),
                            );
                        }
                    }
                })
            }
            FieldShape::Nested(nested_ty) => Ok(quote! {
                match <#nested_ty as #tc::Schema>::validate(&#value_var) {
                    ::std::result::Result::Ok(__value) => {
                        #setter
                    }
                    ::std::result::Result::Err(__nested_errors) => {
                        for (__path, __err) in __nested_errors.into_iter() {
                            __errors.push(
                                ::std::format!("{}.{}", #path_expr, __path),
                                __err,
                            );
                        }
                    }
                }
            }),
        }
    }

    #[allow(clippy::unnecessary_wraps)]
    fn generate_scalar_validation(
        &self,
        tc: &syn::Path,
        scalar: ScalarType,
        field_ident: &Ident,
        field_name_str: &LitStr,
        trim: bool,
    ) -> syn::Result<TokenStream> {
        let coerce_fn = coerce_fn_for(scalar, tc);
        let extract = extract_for(scalar, tc);
        let field_name_path = quote! { #field_name_str };
        let checks = self.generate_checks(tc, &field_name_path);
        let has_validators = !checks.is_empty();
        let trim_code = if trim {
            quote! { __coerced = #tc::validator::trim(__coerced); }
        } else {
            TokenStream::new()
        };

        let validation_body = if has_validators {
            quote! {
                '__scalar: {
                    #(#checks)*
                    match #extract {
                        ::std::result::Result::Ok(__value) => {
                            #field_ident = ::std::option::Option::Some(__value);
                            break '__scalar;
                        }
                        ::std::result::Result::Err(__err) => {
                            __errors.push(#field_name_str, __err);
                            break '__scalar;
                        }
                    }
                }
            }
        } else {
            quote! {
                match #extract {
                    ::std::result::Result::Ok(__value) => {
                        #field_ident = ::std::option::Option::Some(__value);
                    }
                    ::std::result::Result::Err(__err) => {
                        __errors.push(#field_name_str, __err);
                    }
                }
            }
        };

        Ok(quote! {
            match #coerce_fn(&__raw) {
                ::std::result::Result::Ok(mut __coerced) => {
                    #trim_code
                    #validation_body
                }
                ::std::result::Result::Err(__err) => {
                    __errors.push(#field_name_str, __err);
                }
            }
        })
    }

    #[allow(clippy::unnecessary_wraps)]
    fn generate_scalar_block(
        &self,
        tc: &syn::Path,
        scalar: ScalarType,
        value_var: &Ident,
        path_expr: &TokenStream,
        setter: &TokenStream,
        trim: bool,
    ) -> syn::Result<TokenStream> {
        let coerce_fn = coerce_fn_for(scalar, tc);
        let extract = extract_for(scalar, tc);
        let checks = self.generate_checks(tc, path_expr);
        let has_validators = !checks.is_empty();
        let trim_code = if trim {
            quote! { __coerced = #tc::validator::trim(__coerced); }
        } else {
            TokenStream::new()
        };

        let validation_body = if has_validators {
            quote! {
                '__scalar: {
                    #(#checks)*
                    match #extract {
                        ::std::result::Result::Ok(__value) => {
                            #setter
                            break '__scalar;
                        }
                        ::std::result::Result::Err(__err) => {
                            __errors.push(#path_expr.clone(), __err);
                            break '__scalar;
                        }
                    }
                }
            }
        } else {
            quote! {
                match #extract {
                    ::std::result::Result::Ok(__value) => {
                        #setter
                    }
                    ::std::result::Result::Err(__err) => {
                        __errors.push(#path_expr.clone(), __err);
                    }
                }
            }
        };

        Ok(quote! {
            match #coerce_fn(&#value_var) {
                ::std::result::Result::Ok(mut __coerced) => {
                    #trim_code
                    #validation_body
                }
                ::std::result::Result::Err(__err) => {
                    __errors.push(#path_expr.clone(), __err);
                }
            }
        })
    }

    fn generate_checks(&self, tc: &syn::Path, path_expr: &TokenStream) -> Vec<TokenStream> {
        let mut checks = Vec::new();

        for attr in &self.attrs {
            match attr {
                FieldAttr::Email => {
                    checks.push(quote! {
                        if let ::std::result::Result::Err(__err) = #tc::validator::email(&__coerced) {
                            __errors.push(#path_expr.clone(), __err);
                            break '__scalar;
                        }
                    });
                }
                FieldAttr::MinLength { value } => {
                    checks.push(quote! {
                        if let ::std::result::Result::Err(__err) = #tc::validator::min_length(&__coerced, #value as usize) {
                            __errors.push(#path_expr.clone(), __err);
                            break '__scalar;
                        }
                    });
                }
                FieldAttr::MaxLength { value } => {
                    checks.push(quote! {
                        if let ::std::result::Result::Err(__err) = #tc::validator::max_length(&__coerced, #value as usize) {
                            __errors.push(#path_expr.clone(), __err);
                            break '__scalar;
                        }
                    });
                }
                FieldAttr::Min { value } => {
                    let number = number_token(value, tc);
                    checks.push(quote! {
                        if let ::std::result::Result::Err(__err) = #tc::validator::min(&__coerced, #number) {
                            __errors.push(#path_expr.clone(), __err);
                            break '__scalar;
                        }
                    });
                }
                FieldAttr::Max { value } => {
                    let number = number_token(value, tc);
                    checks.push(quote! {
                        if let ::std::result::Result::Err(__err) = #tc::validator::max(&__coerced, #number) {
                            __errors.push(#path_expr.clone(), __err);
                            break '__scalar;
                        }
                    });
                }
                FieldAttr::Range { min, max } => {
                    let min_number = number_token(min, tc);
                    let max_number = number_token(max, tc);
                    checks.push(quote! {
                        if let ::std::result::Result::Err(__err) = #tc::validator::range(&__coerced, #min_number, #max_number) {
                            __errors.push(#path_expr.clone(), __err);
                            break '__scalar;
                        }
                    });
                }
                FieldAttr::OneOf { value } => {
                    let choices = one_of_choices(value);
                    checks.push(quote! {
                        if let ::std::result::Result::Err(__err) = #tc::validator::one_of(&__coerced, #choices) {
                            __errors.push(#path_expr.clone(), __err);
                            break '__scalar;
                        }
                    });
                }
                FieldAttr::Regex { value } => {
                    checks.push(quote! {
                        if let ::std::result::Result::Err(__err) = #tc::validator::regex(&__coerced, #value) {
                            __errors.push(#path_expr.clone(), __err);
                            break '__scalar;
                        }
                    });
                }
                FieldAttr::Custom { path } => {
                    checks.push(quote! {
                        __coerced = match <#path as #tc::validator::CustomValidator>::validate(&__coerced) {
                            ::std::result::Result::Ok(__value) => __value,
                            ::std::result::Result::Err(__err) => {
                                __errors.push(#path_expr.clone(), __err);
                                break '__scalar;
                            }
                        };
                    });
                }
                _ => {}
            }
        }

        checks
    }

    fn generate_descriptor(&self, tc: &syn::Path) -> syn::Result<TokenStream> {
        let field_name = self.field_name();
        let field_name_str = LitStr::new(&field_name, self.ident.span());
        let shape = self.shape()?;
        let field_type = field_type_for_shape(&shape, tc);
        let required = !shape.is_optional() && !self.has_default();
        let mut validator_descriptors: Vec<TokenStream> = self
            .attrs
            .iter()
            .filter_map(|attr| validator_descriptor_token(attr, tc))
            .collect();
        if self.has_default() {
            validator_descriptors.push(quote! { #tc::descriptor::ValidatorDescriptor::Default });
        }

        Ok(quote! {
            #tc::descriptor::FieldDescriptor {
                name: ::std::borrow::Cow::Borrowed(#field_name_str),
                ty: #field_type,
                validators: ::std::vec![#(#validator_descriptors),*],
                required: #required,
            }
        })
    }
}

struct FieldAttrs {
    items: Vec<FieldAttr>,
}

impl Parse for FieldAttrs {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let punctuated: Punctuated<FieldAttr, Token![,]> = Punctuated::parse_terminated(input)?;
        Ok(Self {
            items: punctuated.into_iter().collect(),
        })
    }
}

impl Parse for FieldAttr {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let ident: Ident = input.parse()?;
        let name = ident.to_string();
        let span = ident.span();

        match name.as_str() {
            "email" => Ok(FieldAttr::Email),
            "trim" => Ok(FieldAttr::Trim),
            "string" => Ok(FieldAttr::String),
            "number" => Ok(FieldAttr::Number),
            "bool" => Ok(FieldAttr::Bool),
            "min_length" | "max_length" => {
                input.parse::<Token![=]>()?;
                let value: LitInt = input.parse()?;
                if name == "min_length" {
                    Ok(FieldAttr::MinLength { value })
                } else {
                    Ok(FieldAttr::MaxLength { value })
                }
            }
            "min" | "max" => {
                input.parse::<Token![=]>()?;
                let value: LitNumber = input.parse()?;
                if name == "min" {
                    Ok(FieldAttr::Min { value })
                } else {
                    Ok(FieldAttr::Max { value })
                }
            }
            "range" => {
                let content;
                syn::parenthesized!(content in input);
                let min_ident: Ident = content.parse()?;
                if min_ident != "min" {
                    return Err(syn::Error::new(min_ident.span(), "expected `min`"));
                }
                content.parse::<Token![=]>()?;
                let min: LitNumber = content.parse()?;
                content.parse::<Token![,]>()?;
                let max_ident: Ident = content.parse()?;
                if max_ident != "max" {
                    return Err(syn::Error::new(max_ident.span(), "expected `max`"));
                }
                content.parse::<Token![=]>()?;
                let max: LitNumber = content.parse()?;
                Ok(FieldAttr::Range { min, max })
            }
            "one_of" | "regex" | "rename" => {
                input.parse::<Token![=]>()?;
                let value: LitStr = input.parse()?;
                match name.as_str() {
                    "one_of" => Ok(FieldAttr::OneOf { value }),
                    "regex" => Ok(FieldAttr::Regex { value }),
                    "rename" => Ok(FieldAttr::Rename { value }),
                    _ => unreachable!(),
                }
            }
            "custom" => {
                input.parse::<Token![=]>()?;
                let path: Path = input.parse()?;
                Ok(FieldAttr::Custom { path })
            }
            "default" => {
                input.parse::<Token![=]>()?;
                let expr: Expr = input.parse()?;
                Ok(FieldAttr::Default { expr })
            }
            _ => Err(syn::Error::new(
                span,
                format!("unknown schema attribute `{name}`"),
            )),
        }
    }
}

impl Parse for LitNumber {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let negative = if input.peek(Token![-]) {
            input.parse::<Token![-]>()?;
            true
        } else {
            false
        };
        let lit: Lit = input.parse()?;
        match lit {
            Lit::Int(ref int) => {
                let magnitude: i128 = int.base10_digits().parse().map_err(|_| {
                    syn::Error::new(
                        int.span(),
                        "integer literal is out of range for a schema bound",
                    )
                })?;
                // Reject anything past i64::MAX on either side: larger
                // magnitudes could not round-trip through the generated code.
                if magnitude > i128::from(i64::MAX) {
                    return Err(syn::Error::new(
                        int.span(),
                        "integer literal is out of range for a schema bound",
                    ));
                }
                Ok(Self { negative, lit })
            }
            Lit::Float(_) => Ok(Self { negative, lit }),
            _ => Err(syn::Error::new(
                lit.span(),
                "expected an integer or float literal",
            )),
        }
    }
}

fn infer_shape(ty: &Type, coerce_attr: Option<&ScalarType>) -> syn::Result<FieldShape> {
    if let Some(scalar) = coerce_attr {
        return Ok(FieldShape::Scalar(*scalar));
    }

    match ty {
        Type::Path(type_path) if type_path.qself.is_none() => {
            let segment = type_path.path.segments.last().ok_or_else(|| {
                syn::Error::new_spanned(
                    ty,
                    "unsupported field type for schema: try String, i64, or a custom validator",
                )
            })?;
            let name = segment.ident.to_string();
            match name.as_str() {
                "String" => Ok(FieldShape::Scalar(ScalarType::String)),
                "bool" => Ok(FieldShape::Scalar(ScalarType::Bool)),
                "i8" | "i16" | "i32" | "i64" | "u8" | "u16" | "u32" | "u64" => {
                    Ok(FieldShape::Scalar(ScalarType::Integer))
                }
                "f32" => Ok(FieldShape::Scalar(ScalarType::F32)),
                "f64" => Ok(FieldShape::Scalar(ScalarType::F64)),
                "Option" => {
                    let inner = extract_single_generic(segment)?;
                    Ok(FieldShape::Option(Box::new(infer_shape(inner, None)?)))
                }
                "Vec" => {
                    let inner = extract_single_generic(segment)?;
                    Ok(FieldShape::Vec(Box::new(infer_shape(inner, None)?)))
                }
                "i128" | "u128" => Err(syn::Error::new_spanned(
                    ty,
                    format!(
                        "unsupported field type `{name}` for schema: 128-bit integers are not supported, try i64 or a custom validator"
                    ),
                )),
                _ => Ok(FieldShape::Nested(Box::new(ty.clone()))),
            }
        }
        Type::Reference(reference) if reference.mutability.is_none() => {
            if let Type::Path(type_path) = &*reference.elem
                && let Some(segment) = type_path.path.segments.last()
                && segment.ident == "str"
            {
                return Err(syn::Error::new_spanned(
                    ty,
                    "unsupported field type `&str` for schema: validation values are owned, try String",
                ));
            }
            Err(syn::Error::new_spanned(
                ty,
                "unsupported field type for schema: try String, i64, or a custom validator",
            ))
        }
        _ => Err(syn::Error::new_spanned(
            ty,
            "unsupported field type for schema: try String, i64, or a custom validator",
        )),
    }
}

fn extract_single_generic(segment: &syn::PathSegment) -> syn::Result<&Type> {
    let syn::PathArguments::AngleBracketed(args) = &segment.arguments else {
        return Err(syn::Error::new_spanned(
            segment,
            "expected a generic argument",
        ));
    };
    let mut types = args.args.iter().filter_map(|arg| match arg {
        syn::GenericArgument::Type(ty) => Some(ty),
        _ => None,
    });
    let first = types
        .next()
        .ok_or_else(|| syn::Error::new_spanned(segment, "expected a type argument"))?;
    if types.next().is_some() {
        return Err(syn::Error::new_spanned(
            segment,
            "expected exactly one type argument",
        ));
    }
    Ok(first)
}

fn coerce_fn_for(scalar: ScalarType, tc: &syn::Path) -> TokenStream {
    match scalar {
        ScalarType::String => quote! { #tc::validator::string },
        ScalarType::Integer => quote! { #tc::validator::integer },
        ScalarType::F32 | ScalarType::F64 => quote! { #tc::validator::float },
        ScalarType::Bool => quote! { #tc::validator::bool },
    }
}

fn extract_for(scalar: ScalarType, tc: &syn::Path) -> TokenStream {
    match scalar {
        ScalarType::String => quote! {
            match __coerced {
                #tc::value::Value::String(value) => ::std::result::Result::Ok(value),
                _ => ::std::result::Result::Err(#tc::ValidationError::with_code("string", "Value must be a string")),
            }
        },
        ScalarType::Integer => quote! {
            match __coerced {
                #tc::value::Value::Number(#tc::value::Number::Integer(value)) => {
                    match ::std::convert::TryFrom::try_from(value) {
                        ::std::result::Result::Ok(v) => ::std::result::Result::Ok(v),
                        ::std::result::Result::Err(_) => ::std::result::Result::Err(
                            #tc::ValidationError::with_code("integer", "Value is out of range")
                        ),
                    }
                }
                _ => ::std::result::Result::Err(#tc::ValidationError::with_code("integer", "Value must be an integer")),
            }
        },
        ScalarType::F32 => quote! {
            match __coerced {
                #tc::value::Value::Number(#tc::value::Number::Float(value)) => {
                    #[allow(clippy::cast_possible_truncation)] // narrow to the declared f32 field
                    let narrowed = value as f32;
                    ::std::result::Result::Ok(narrowed)
                }
                _ => ::std::result::Result::Err(#tc::ValidationError::with_code("float", "Value must be a number")),
            }
        },
        ScalarType::F64 => quote! {
            match __coerced {
                #tc::value::Value::Number(#tc::value::Number::Float(value)) => ::std::result::Result::Ok(value),
                _ => ::std::result::Result::Err(#tc::ValidationError::with_code("float", "Value must be a number")),
            }
        },
        ScalarType::Bool => quote! {
            match __coerced {
                #tc::value::Value::Bool(value) => ::std::result::Result::Ok(value),
                _ => ::std::result::Result::Err(#tc::ValidationError::with_code("bool", "Value must be a boolean")),
            }
        },
    }
}

fn field_type_for_shape(shape: &FieldShape, tc: &syn::Path) -> TokenStream {
    match shape {
        FieldShape::Scalar(ScalarType::String) => quote! { #tc::descriptor::FieldType::String },
        FieldShape::Scalar(ScalarType::Integer) => quote! { #tc::descriptor::FieldType::Integer },
        FieldShape::Scalar(ScalarType::F32 | ScalarType::F64) => {
            quote! { #tc::descriptor::FieldType::Float }
        }
        FieldShape::Scalar(ScalarType::Bool) => quote! { #tc::descriptor::FieldType::Bool },
        FieldShape::Option(inner) => field_type_for_shape(inner, tc),
        FieldShape::Vec(inner) => {
            let inner_type = field_type_for_shape(inner, tc);
            quote! { #tc::descriptor::FieldType::List(::std::boxed::Box::new(#inner_type)) }
        }
        FieldShape::Nested(nested_ty) => {
            quote! { #tc::descriptor::FieldType::Nested(<#nested_ty as #tc::Schema>::descriptor()) }
        }
    }
}

fn number_token(number: &LitNumber, tc: &syn::Path) -> TokenStream {
    let LitNumber { negative, lit } = number;
    match lit {
        Lit::Int(int) => {
            // Parse checked the range, so the magnitude always fits and
            // negating it cannot overflow.
            let magnitude = int
                .base10_digits()
                .parse::<i64>()
                .expect("parse rejected out-of-range integer bounds");
            let value = if *negative { -magnitude } else { magnitude };
            quote! { #tc::value::Number::Integer(#value) }
        }
        Lit::Float(float) => {
            let magnitude: f64 = float
                .base10_digits()
                .parse()
                .expect("a float literal parses");
            let value = if *negative { -magnitude } else { magnitude };
            quote! { #tc::value::Number::Float(#value) }
        }
        _ => unreachable!(),
    }
}

fn one_of_choices(lit: &LitStr) -> TokenStream {
    let choices: Vec<LitStr> = lit
        .value()
        .split(',')
        .map(|s| LitStr::new(s.trim(), lit.span()))
        .collect();
    quote! { &[#(#choices),*] }
}

fn validator_descriptor_token(attr: &FieldAttr, tc: &syn::Path) -> Option<TokenStream> {
    match attr {
        FieldAttr::Email => Some(quote! { #tc::descriptor::ValidatorDescriptor::Email }),
        FieldAttr::MinLength { value } => {
            Some(quote! { #tc::descriptor::ValidatorDescriptor::MinLength(#value as usize) })
        }
        FieldAttr::MaxLength { value } => {
            Some(quote! { #tc::descriptor::ValidatorDescriptor::MaxLength(#value as usize) })
        }
        FieldAttr::Min { value } => {
            let number = number_token(value, tc);
            Some(quote! { #tc::descriptor::ValidatorDescriptor::Min(#number) })
        }
        FieldAttr::Max { value } => {
            let number = number_token(value, tc);
            Some(quote! { #tc::descriptor::ValidatorDescriptor::Max(#number) })
        }
        FieldAttr::Range { min, max } => {
            let min_number = number_token(min, tc);
            let max_number = number_token(max, tc);
            Some(
                quote! { #tc::descriptor::ValidatorDescriptor::Range { min: #min_number, max: #max_number } },
            )
        }
        FieldAttr::OneOf { value } => {
            let choices = one_of_choices(value);
            Some(quote! { #tc::descriptor::ValidatorDescriptor::OneOf(#choices) })
        }
        FieldAttr::Regex { value } => {
            Some(quote! { #tc::descriptor::ValidatorDescriptor::Regex(#value) })
        }
        FieldAttr::Trim => Some(quote! { #tc::descriptor::ValidatorDescriptor::Trim }),
        FieldAttr::Custom { path } => Some(quote! {
            #tc::descriptor::ValidatorDescriptor::Custom(#tc::descriptor::CustomDescriptor {
                name: <#path as #tc::validator::CustomValidator>::name(),
                message: <#path as #tc::validator::CustomValidator>::message(),
            })
        }),
        _ => None,
    }
}

/// The path generated code uses to name `topcoat-validate` items.
///
/// The standalone crate is preferred whenever it is a dependency of the crate
/// being compiled, under whatever name the caller gives it; a caller that
/// depends on the facade alone resolves through `::topcoat::validate`. Mirrors
/// the resolution logic in `topcoat-core-grammar`'s `paths` module.
fn crate_path() -> syn::Path {
    if let Ok(found) = proc_macro_crate::crate_name("topcoat-validate") {
        let name = match found {
            proc_macro_crate::FoundCrate::Itself => "topcoat_validate".to_string(),
            proc_macro_crate::FoundCrate::Name(name) => name,
        };
        let ident = Ident::new(&name, Span::call_site());
        return parse_quote!(::#ident);
    }

    if let Ok(found) = proc_macro_crate::crate_name("topcoat") {
        let name = match found {
            proc_macro_crate::FoundCrate::Itself => "topcoat".to_string(),
            proc_macro_crate::FoundCrate::Name(name) => name,
        };
        let ident = Ident::new(&name, Span::call_site());
        return parse_quote!(::#ident::validate);
    }

    // Neither the crate nor the facade is a declared dependency.
    parse_quote!(::topcoat_validate)
}
