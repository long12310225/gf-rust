//! The `#[derive(Validate)]` macro for `util/gvalid` (ADR-092).
//!
//! Annotate fields with `#[gf(rule = "required|length:1,10")]`; the macro
//! implements [`gf_valid::gvalid::Validatable`] for the struct, emitting the
//! rules in declaration order and a data map built through
//! `gf_valid::gvalid::FieldValue`.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{Data, DeriveInput, Fields, LitStr, parse_macro_input};

/// Derives `Validatable` from `#[gf(rule = "...")]` field attributes.
#[proc_macro_derive(Validate, attributes(gf))]
pub fn derive_validate(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match expand(input) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

/// Builds the `Validatable` implementation.
fn expand(input: DeriveInput) -> syn::Result<TokenStream2> {
    let name = &input.ident;
    let Data::Struct(data) = &input.data else {
        return Err(syn::Error::new_spanned(
            name,
            "Validate can only be derived for structs",
        ));
    };
    let Fields::Named(fields) = &data.fields else {
        return Err(syn::Error::new_spanned(
            name,
            "Validate requires named fields",
        ));
    };
    let mut rule_entries = Vec::new();
    let mut data_entries = Vec::new();
    for field in &fields.named {
        let Some(ident) = field.ident.as_ref() else {
            continue;
        };
        let mut rule: Option<String> = None;
        let mut name_override: Option<String> = None;
        for attribute in &field.attrs {
            if !attribute.path().is_ident("gf") {
                continue;
            }
            attribute.parse_nested_meta(|meta| {
                if meta.path.is_ident("rule") {
                    let value: LitStr = meta.value()?.parse()?;
                    rule = Some(value.value());
                    return Ok(());
                }
                if meta.path.is_ident("name") {
                    let value: LitStr = meta.value()?.parse()?;
                    name_override = Some(value.value());
                    return Ok(());
                }
                // Unknown `gf` keys are ignored for forward compatibility.
                if meta.input.peek(syn::Token![=]) {
                    let _: LitStr = meta.value()?.parse()?;
                }
                Ok(())
            })?;
        }
        let field_name = name_override.unwrap_or_else(|| ident.to_string());
        if let Some(rule) = rule {
            rule_entries.push(quote! {
                rules.insert(::std::string::String::from(#field_name), ::std::string::String::from(#rule));
            });
        }
        data_entries.push(quote! {
            data.insert(
                ::std::string::String::from(#field_name),
                ::gf_valid::gvalid::FieldValue::field_value(&self.#ident),
            );
        });
    }
    Ok(quote! {
        impl ::gf_valid::gvalid::Validatable for #name {
            fn validation_rules() -> ::gf_valid::gvalid::Rules {
                let mut rules = ::gf_valid::gvalid::IndexMap::new();
                #(#rule_entries)*
                ::gf_valid::gvalid::Rules::Map(rules)
            }

            fn validation_data(&self) -> ::gf_valid::gvalid::Value {
                let mut data = ::gf_valid::gvalid::IndexMap::new();
                #(#data_entries)*
                ::gf_valid::gvalid::Value::Map(data)
            }
        }
    })
}
