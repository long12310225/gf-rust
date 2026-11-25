//! `#[derive(StructMeta)]` for `gf-structs` (ADR-183).
//!
//! Reads each field and its optional `#[gf(tag = "...")]` Go-style tag text,
//! then emits a `StructMeta` implementation with compile-time metadata.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{Data, DeriveInput, Fields, LitStr, parse_macro_input};

/// Derives `gf_structs::StructMeta`.
#[proc_macro_derive(StructMeta, attributes(gf))]
pub fn derive_struct_meta(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match expand(&input) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

fn expand(input: &DeriveInput) -> syn::Result<TokenStream2> {
    let name = &input.ident;
    let data = match &input.data {
        Data::Struct(data) => data,
        _ => {
            return Err(syn::Error::new_spanned(
                name,
                "StructMeta can only be derived for structs",
            ));
        }
    };
    let Fields::Named(fields) = &data.fields else {
        return Err(syn::Error::new_spanned(
            name,
            "StructMeta requires named fields",
        ));
    };
    let mut entries = Vec::new();
    for field in &fields.named {
        let ident = field.ident.as_ref().expect("named field");
        let field_name = ident.to_string();
        let ty = &field.ty;
        let type_name = quote!(#ty).to_string();
        let optional = type_name.starts_with("Option<") || type_name.starts_with("Option <");
        let tags = parse_gf_tags(field)?;
        let tag_entries = tags.iter().map(|(key, value)| quote!((#key, #value)));
        entries.push(quote! {
            ::gf_structs::FieldMeta {
                name: #field_name,
                type_name: #type_name,
                optional: #optional,
                tags: &[#(#tag_entries),*],
            }
        });
    }
    let struct_name = name.to_string();
    Ok(quote! {
        impl ::gf_structs::StructMeta for #name {
            const NAME: &'static str = #struct_name;
            const FIELDS: &'static [::gf_structs::FieldMeta] = &[#(#entries),*];
        }
    })
}

fn parse_gf_tags(field: &syn::Field) -> syn::Result<Vec<(String, String)>> {
    for attribute in &field.attrs {
        if !attribute.path().is_ident("gf") {
            continue;
        }
        let mut text = None;
        attribute.parse_nested_meta(|meta| {
            if meta.path.is_ident("tag") {
                let value: LitStr = meta.value()?.parse()?;
                text = Some(value.value());
                Ok(())
            } else {
                Err(meta.error("unsupported gf attribute (expected `tag`)"))
            }
        })?;
        if let Some(text) = text {
            return Ok(parse_tag_text(&text));
        }
    }
    Ok(Vec::new())
}

/// Parses Go-style tag text (`json:"name,omitempty" gconv:"nick"`).
fn parse_tag_text(text: &str) -> Vec<(String, String)> {
    let mut tags = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut index = 0;
    while index < chars.len() {
        while index < chars.len() && chars[index].is_whitespace() {
            index += 1;
        }
        let key_start = index;
        while index < chars.len() && chars[index] != ':' && !chars[index].is_whitespace() {
            index += 1;
        }
        if index == key_start {
            break;
        }
        let key: String = chars[key_start..index].iter().collect();
        while index < chars.len() && chars[index].is_whitespace() {
            index += 1;
        }
        if index >= chars.len() || chars[index] != ':' {
            break;
        }
        index += 1;
        while index < chars.len() && chars[index].is_whitespace() {
            index += 1;
        }
        if index >= chars.len() || chars[index] != '"' {
            break;
        }
        index += 1;
        let mut value = String::new();
        while index < chars.len() {
            let character = chars[index];
            if character == '\\' && index + 1 < chars.len() {
                value.push(chars[index + 1]);
                index += 2;
                continue;
            }
            if character == '"' {
                index += 1;
                break;
            }
            value.push(character);
            index += 1;
        }
        tags.push((key, value));
    }
    tags
}
