//! The `#[derive(CommandObject)]` macro for `os/gcmd` (ADR-108).
//!
//! Struct attributes: `#[cmd(name = "serve", brief = "...", usage = "...",
//! description = "...", examples = "...")]`. Field attributes:
//! `#[cmd(short = "p", brief = "port", default = "8080", arg, orphan,
//! rename = "target", skip)]`.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{Data, DeriveInput, Fields, LitStr, parse_macro_input};

/// Derives `CommandObject`, the Rust counterpart of GoFrame's object lane.
#[proc_macro_derive(CommandObject, attributes(cmd))]
pub fn derive_command_object(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match expand(input) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

/// Builds the `CommandObject` implementation.
fn expand(input: DeriveInput) -> syn::Result<TokenStream2> {
    let name = &input.ident;
    let mut command_name = name.to_string();
    let mut usage = String::new();
    let mut brief = String::new();
    let mut description = String::new();
    let mut examples = String::new();
    for attribute in &input.attrs {
        if !attribute.path().is_ident("cmd") {
            continue;
        }
        attribute.parse_nested_meta(|meta| {
            if meta.path.is_ident("name") {
                command_name = meta.value()?.parse::<LitStr>()?.value();
                return Ok(());
            }
            if meta.path.is_ident("usage") {
                usage = meta.value()?.parse::<LitStr>()?.value();
                return Ok(());
            }
            if meta.path.is_ident("brief") {
                brief = meta.value()?.parse::<LitStr>()?.value();
                return Ok(());
            }
            if meta.path.is_ident("description") {
                description = meta.value()?.parse::<LitStr>()?.value();
                return Ok(());
            }
            if meta.path.is_ident("examples") {
                examples = meta.value()?.parse::<LitStr>()?.value();
                return Ok(());
            }
            // Unknown keys are ignored for forward compatibility.
            if meta.input.peek(syn::Token![=]) {
                let _: LitStr = meta.value()?.parse()?;
            }
            Ok(())
        })?;
    }
    let Data::Struct(data) = &input.data else {
        return Err(syn::Error::new_spanned(
            name,
            "CommandObject can only be derived for structs",
        ));
    };
    let Fields::Named(fields) = &data.fields else {
        return Err(syn::Error::new_spanned(
            name,
            "CommandObject requires named fields",
        ));
    };
    let mut argument_inits = Vec::new();
    let mut field_inits = Vec::new();
    for field in &fields.named {
        let Some(ident) = field.ident.as_ref() else {
            continue;
        };
        let mut field_name = ident.to_string();
        let mut short = String::new();
        let mut field_brief = String::new();
        let mut default = String::new();
        let mut is_arg = false;
        let mut orphan = false;
        let mut skip = false;
        for attribute in &field.attrs {
            if !attribute.path().is_ident("cmd") {
                continue;
            }
            attribute.parse_nested_meta(|meta| {
                if meta.path.is_ident("rename") {
                    field_name = meta.value()?.parse::<LitStr>()?.value();
                    return Ok(());
                }
                if meta.path.is_ident("short") {
                    short = meta.value()?.parse::<LitStr>()?.value();
                    return Ok(());
                }
                if meta.path.is_ident("brief") {
                    field_brief = meta.value()?.parse::<LitStr>()?.value();
                    return Ok(());
                }
                if meta.path.is_ident("default") {
                    default = meta.value()?.parse::<LitStr>()?.value();
                    return Ok(());
                }
                if meta.path.is_ident("arg") {
                    is_arg = true;
                    return Ok(());
                }
                if meta.path.is_ident("orphan") {
                    orphan = true;
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
            field_inits.push(quote! {
                #ident: ::std::default::Default::default(),
            });
            continue;
        }
        argument_inits.push(quote! {
            arguments.push(::gf_os::gcmd::Argument {
                name: ::std::string::String::from(#field_name),
                short: ::std::string::String::from(#short),
                default: ::std::string::String::from(#default),
                brief: ::std::string::String::from(#field_brief),
                is_arg: #is_arg,
                orphan: #orphan,
            });
        });
        let default_value = if default.is_empty() {
            quote! { ::std::option::Option::None }
        } else {
            quote! {
                ::std::option::Option::Some(::gf_os::gcmd::CommandValue::String(
                    ::std::string::String::from(#default),
                ))
            }
        };
        let field_type = &field.ty;
        field_inits.push(quote! {
            #ident: {
                let value = parser.get_opt(#field_name, #default_value);
                <#field_type as ::gf_os::gcmd::FieldFromParser>::from_parser_value(value.as_ref())?
            },
        });
    }
    Ok(quote! {
        impl ::gf_os::gcmd::CommandObject for #name {
            fn command_name() -> &'static str {
                #command_name
            }

            fn command_meta() -> ::gf_os::gcmd::CommandMeta {
                let mut arguments = ::std::vec::Vec::new();
                #(#argument_inits)*
                ::gf_os::gcmd::CommandMeta {
                    usage: ::std::string::String::from(#usage),
                    brief: ::std::string::String::from(#brief),
                    description: ::std::string::String::from(#description),
                    examples: ::std::string::String::from(#examples),
                    arguments,
                }
            }

            fn from_parser(
                parser: &::gf_os::gcmd::Parser,
            ) -> ::std::result::Result<Self, ::gf_os::gcmd::CommandError> {
                Ok(#name {
                    #(#field_inits)*
                })
            }

            fn execute(&self) -> ::std::result::Result<(), ::gf_os::gcmd::CommandError> {
                Self::run(self)
            }
        }
    })
}
