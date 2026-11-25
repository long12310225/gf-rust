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

/// Derives `CommandInput`, the input struct of an object subcommand
/// (ADR-186). Fields use the same `#[cmd(...)]` attributes as
/// `CommandObject`.
#[proc_macro_derive(CommandInput, attributes(cmd))]
pub fn derive_command_input(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match expand_input(input) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

/// Marks `impl Root { ... }` as a command object whose methods become
/// subcommands (ADR-186).
///
/// `#[command_object(name = "root", usage = "...")]`; each method takes
/// `&self` plus one `CommandInput` type and returns `Result<(), CommandError>`.
#[proc_macro_attribute]
pub fn command_object(attribute: TokenStream, item: TokenStream) -> TokenStream {
    match expand_command_object(attribute.into(), item.into()) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

fn expand_input(input: DeriveInput) -> syn::Result<TokenStream2> {
    let name = &input.ident;
    let Data::Struct(data) = &input.data else {
        return Err(syn::Error::new_spanned(
            name,
            "CommandInput can only be derived for structs",
        ));
    };
    let Fields::Named(fields) = &data.fields else {
        return Err(syn::Error::new_spanned(
            name,
            "CommandInput requires named fields",
        ));
    };
    let (argument_inits, field_inits) = field_parsers(fields)?;
    Ok(quote! {
        impl ::gf_os::gcmd::CommandInput for #name {
            fn command_meta() -> ::gf_os::gcmd::CommandMeta {
                let mut arguments = ::std::vec::Vec::new();
                #(#argument_inits)*
                ::gf_os::gcmd::CommandMeta {
                    arguments,
                    ..::std::default::Default::default()
                }
            }

            fn from_parser(
                parser: &::gf_os::gcmd::Parser,
            ) -> ::std::result::Result<Self, ::gf_os::gcmd::CommandError> {
                Ok(#name {
                    #(#field_inits)*
                })
            }
        }
    })
}

fn field_parsers(fields: &syn::FieldsNamed) -> syn::Result<(Vec<TokenStream2>, Vec<TokenStream2>)> {
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
    Ok((argument_inits, field_inits))
}

fn expand_command_object(attribute: TokenStream2, item: TokenStream2) -> syn::Result<TokenStream2> {
    let mut root_name = String::new();
    let mut usage = String::new();
    let mut brief = String::new();
    let mut description = String::new();
    let mut examples = String::new();
    if !attribute.is_empty() {
        let parser = syn::meta::parser(|meta| {
            if meta.path.is_ident("name") {
                root_name = meta.value()?.parse::<LitStr>()?.value();
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
            if meta.input.peek(syn::Token![=]) {
                let _: LitStr = meta.value()?.parse()?;
            }
            Ok(())
        });
        syn::parse::Parser::parse2(parser, attribute)?;
    }
    let mut item_impl: syn::ItemImpl = syn::parse2(item)?;
    let self_type = &item_impl.self_ty;
    let mut sub_commands = Vec::new();
    for method in &item_impl.items {
        let syn::ImplItem::Fn(method) = method else {
            continue;
        };
        let method_ident = &method.sig.ident;
        let method_name = method_ident.to_string();
        let method_attrs = method.attrs.clone();
        let mut sub_name = method_name.clone();
        let mut sub_usage = String::new();
        let mut sub_brief = String::new();
        let mut sub_description = String::new();
        for attribute in &method_attrs {
            if !attribute.path().is_ident("cmd") {
                continue;
            }
            attribute.parse_nested_meta(|meta| {
                if meta.path.is_ident("name") {
                    sub_name = meta.value()?.parse::<LitStr>()?.value();
                    return Ok(());
                }
                if meta.path.is_ident("usage") {
                    sub_usage = meta.value()?.parse::<LitStr>()?.value();
                    return Ok(());
                }
                if meta.path.is_ident("brief") {
                    sub_brief = meta.value()?.parse::<LitStr>()?.value();
                    return Ok(());
                }
                if meta.path.is_ident("description") {
                    sub_description = meta.value()?.parse::<LitStr>()?.value();
                    return Ok(());
                }
                if meta.input.peek(syn::Token![=]) {
                    let _: LitStr = meta.value()?.parse()?;
                }
                Ok(())
            })?;
        }
        let typed: Vec<&syn::PatType> = method
            .sig
            .inputs
            .iter()
            .filter_map(|input| match input {
                syn::FnArg::Typed(typed) => Some(typed),
                syn::FnArg::Receiver(_) => None,
            })
            .collect();
        if typed.len() != 1 {
            return Err(syn::Error::new_spanned(
                &method.sig,
                "command object methods need &self plus exactly one CommandInput parameter",
            ));
        }
        let input_type = &typed[0].ty;
        let default_usage = if sub_usage.is_empty() {
            quote! { ::std::format!("{} {}", #root_name, #sub_name) }
        } else {
            quote! { ::std::string::String::from(#sub_usage) }
        };
        sub_commands.push(quote! {
            {
                let mut command = ::gf_os::gcmd::Command::new(#sub_name);
                command.usage = #default_usage;
                command.brief = ::std::string::String::from(#sub_brief);
                command.description = ::std::string::String::from(#sub_description);
                command.arguments =
                    <#input_type as ::gf_os::gcmd::CommandInput>::command_meta().arguments;
                command.run = ::std::option::Option::Some(::std::sync::Arc::new(
                    |parser: &::gf_os::gcmd::Parser| {
                        let root = <#self_type as ::gf_os::gcmd::CommandObject>::from_parser(parser)?;
                        let input =
                            <#input_type as ::gf_os::gcmd::CommandInput>::from_parser(parser)?;
                        <#self_type>::#method_ident(&root, input)
                    },
                ));
                commands.push(::std::sync::Arc::new(command));
            }
        });
    }
    for item in &mut item_impl.items {
        if let syn::ImplItem::Fn(method) = item {
            method
                .attrs
                .retain(|attribute| !attribute.path().is_ident("cmd"));
        }
    }
    Ok(quote! {
        #item_impl

        impl ::gf_os::gcmd::CommandObject for #self_type {
            fn command_name() -> &'static str {
                #root_name
            }

            fn command_meta() -> ::gf_os::gcmd::CommandMeta {
                ::gf_os::gcmd::CommandMeta {
                    usage: ::std::string::String::from(#usage),
                    brief: ::std::string::String::from(#brief),
                    description: ::std::string::String::from(#description),
                    examples: ::std::string::String::from(#examples),
                    arguments: ::std::vec::Vec::new(),
                }
            }

            fn from_parser(
                _parser: &::gf_os::gcmd::Parser,
            ) -> ::std::result::Result<Self, ::gf_os::gcmd::CommandError> {
                Ok(<Self as ::std::default::Default>::default())
            }

            fn execute(&self) -> ::std::result::Result<(), ::gf_os::gcmd::CommandError> {
                Ok(())
            }

            fn sub_commands(&self) -> ::std::vec::Vec<::std::sync::Arc<::gf_os::gcmd::Command>> {
                let mut commands = ::std::vec::Vec::new();
                #(#sub_commands)*
                commands
            }
        }
    })
}
