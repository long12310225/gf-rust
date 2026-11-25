//! The `#[derive(FromRecord)]` macro for `database/gdb` struct scanning
//! (ADR-100).
//!
//! Fields map to columns by name; `#[gdb(column = "user_name")]` renames the
//! column and `#[gdb(skip)]` fills the field with `Default::default()`.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{Data, DeriveInput, Fields, LitStr, parse_macro_input};

/// Derives `FromRecord`, mapping record columns to struct fields.
#[proc_macro_derive(FromRecord, attributes(gdb))]
pub fn derive_from_record(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match expand(input) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

/// Derives `ToRecord`, mapping struct fields to INSERT/UPDATE columns
/// (ADR-122).
#[proc_macro_derive(ToRecord, attributes(gdb))]
pub fn derive_to_record(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match expand_to_record(input) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

/// Builds the `ToRecord` implementation.
fn expand_to_record(input: DeriveInput) -> syn::Result<TokenStream2> {
    let name = &input.ident;
    let Data::Struct(data) = &input.data else {
        return Err(syn::Error::new_spanned(
            name,
            "ToRecord can only be derived for structs",
        ));
    };
    let Fields::Named(fields) = &data.fields else {
        return Err(syn::Error::new_spanned(
            name,
            "ToRecord requires named fields",
        ));
    };
    let mut pushes = Vec::new();
    for field in &fields.named {
        let Some(ident) = field.ident.as_ref() else {
            continue;
        };
        let mut column = ident.to_string();
        let mut skip = false;
        for attribute in &field.attrs {
            if !attribute.path().is_ident("gdb") {
                continue;
            }
            attribute.parse_nested_meta(|meta| {
                if meta.path.is_ident("column") {
                    let value: LitStr = meta.value()?.parse()?;
                    column = value.value();
                    return Ok(());
                }
                if meta.path.is_ident("skip") {
                    skip = true;
                    return Ok(());
                }
                if meta.input.peek(syn::Token![=]) {
                    let _: LitStr = meta.value()?.parse()?;
                }
                Ok(())
            })?;
        }
        if skip {
            continue;
        }
        pushes.push(quote! {
            data.push((
                ::std::string::String::from(#column),
                ::gf_database::gdb::ToColumnValue::to_column_value(&self.#ident),
            ));
        });
    }
    Ok(quote! {
        impl ::gf_database::gdb::ToRecord for #name {
            fn to_record(&self) -> ::std::vec::Vec<(
                ::std::string::String,
                ::gf_database::gdb::Value,
            )> {
                let mut data = ::std::vec::Vec::new();
                #(#pushes)*
                data
            }
        }
    })
}

/// Builds the FromRecord implementation.
fn expand(input: DeriveInput) -> syn::Result<TokenStream2> {
    let name = &input.ident;
    let Data::Struct(data) = &input.data else {
        return Err(syn::Error::new_spanned(
            name,
            "FromRecord can only be derived for structs",
        ));
    };
    let Fields::Named(fields) = &data.fields else {
        return Err(syn::Error::new_spanned(
            name,
            "FromRecord requires named fields",
        ));
    };
    let mut initializers = Vec::new();
    for field in &fields.named {
        let Some(ident) = field.ident.as_ref() else {
            continue;
        };
        let mut column = ident.to_string();
        let mut skip = false;
        for attribute in &field.attrs {
            if !attribute.path().is_ident("gdb") {
                continue;
            }
            attribute.parse_nested_meta(|meta| {
                if meta.path.is_ident("column") {
                    let value: LitStr = meta.value()?.parse()?;
                    column = value.value();
                    return Ok(());
                }
                if meta.path.is_ident("skip") {
                    skip = true;
                    return Ok(());
                }
                Ok(())
            })?;
        }
        if skip {
            initializers.push(quote! {
                #ident: ::std::default::Default::default(),
            });
        } else {
            initializers.push(quote! {
                #ident: ::gf_database::gdb::FieldFromValue::field_from_value(
                    record.get(#column),
                )?,
            });
        }
    }
    Ok(quote! {
        impl ::gf_database::gdb::FromRecord for #name {
            fn from_record(
                record: &::gf_database::gdb::Record,
            ) -> ::std::result::Result<Self, ::gf_database::gdb::Error> {
                Ok(#name {
                    #(#initializers)*
                })
            }
        }
    })
}
