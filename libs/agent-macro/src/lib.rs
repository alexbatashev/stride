use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{Attribute, Data, DeriveInput, Expr, Fields, Lit, Meta, Type, parse_macro_input};

#[proc_macro_derive(ToolDesc)]
pub fn derive_tool_desc(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    expand_tool_desc(input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

fn expand_tool_desc(input: DeriveInput) -> syn::Result<TokenStream2> {
    let struct_name = input.ident;
    let generics = input.generics;
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
    let fields = match input.data {
        Data::Struct(data) => match data.fields {
            Fields::Named(fields) => fields.named,
            _ => {
                return Err(syn::Error::new(
                    struct_name.span(),
                    "ToolDesc can only be derived for structs with named fields",
                ));
            }
        },
        _ => {
            return Err(syn::Error::new(
                struct_name.span(),
                "ToolDesc can only be derived for structs",
            ));
        }
    };

    let mut property_inserts = Vec::new();
    let mut required_pushes = Vec::new();
    let mut field_decoders = Vec::new();

    for field in fields {
        let ident = field.ident.expect("named field");
        let name = ident.to_string();
        let description = doc_description(&field.attrs);
        let (ty, required) = schema_type(&field.ty);

        property_inserts.push(quote! {
            properties.insert(
                #name.to_string(),
                llm::FunctionProperty {
                    r#type: #ty.to_string(),
                    description: #description.to_string(),
                },
            );
        });

        if required {
            required_pushes.push(quote! {
                required.push(#name.to_string());
            });
        }

        let missing_message = format!("missing required parameter `{name}`");
        let decode_error_prefix = format!("invalid parameter `{name}`: ");
        if required {
            field_decoders.push(quote! {
                #ident: {
                    let value = object
                        .remove(#name)
                        .ok_or_else(|| #missing_message.to_string())?;
                    serde_json::from_value(value)
                        .map_err(|err| format!("{}{}", #decode_error_prefix, err))?
                }
            });
        } else {
            field_decoders.push(quote! {
                #ident: match object.remove(#name) {
                    Some(value) if !value.is_null() => serde_json::from_value(value)
                        .map_err(|err| format!("{}{}", #decode_error_prefix, err))?,
                    _ => None,
                }
            });
        }
    }

    let required_expr = if required_pushes.is_empty() {
        quote!(None)
    } else {
        quote!(Some(required))
    };

    Ok(quote! {
        impl #impl_generics friday_agent::ToolDesc for #struct_name #ty_generics #where_clause {
            fn function_parameters() -> llm::FunctionParameters {
                let mut properties = std::collections::HashMap::new();
                let mut required = Vec::new();

                #(#property_inserts)*
                #(#required_pushes)*

                llm::FunctionParameters {
                    param_type: "object".to_string(),
                    properties,
                    required: #required_expr,
                }
            }

            fn decode(value: serde_json::Value) -> Result<Self, String> {
                Self::try_from(value)
            }
        }

        impl #impl_generics From<#struct_name #ty_generics> for llm::FunctionParameters #where_clause {
            fn from(_: #struct_name #ty_generics) -> Self {
                <#struct_name #ty_generics as friday_agent::ToolDesc>::function_parameters()
            }
        }

        impl #impl_generics TryFrom<serde_json::Value> for #struct_name #ty_generics #where_clause {
            type Error = String;

            fn try_from(value: serde_json::Value) -> Result<Self, Self::Error> {
                let mut object = match value {
                    serde_json::Value::Object(object) => object,
                    _ => return Err("tool arguments must be a JSON object".to_string()),
                };

                Ok(Self {
                    #(#field_decoders,)*
                })
            }
        }
    })
}

fn doc_description(attrs: &[Attribute]) -> String {
    attrs
        .iter()
        .filter_map(|attr| {
            if !attr.path().is_ident("doc") {
                return None;
            }

            match &attr.meta {
                Meta::NameValue(meta) => match &meta.value {
                    Expr::Lit(expr) => match &expr.lit {
                        Lit::Str(lit) => Some(lit.value().trim().to_string()),
                        _ => None,
                    },
                    _ => None,
                },
                _ => None,
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn schema_type(ty: &Type) -> (&'static str, bool) {
    if let Some(inner) = single_generic_arg(ty, "Option") {
        return (schema_type(inner).0, false);
    }

    if single_generic_arg(ty, "Vec").is_some() {
        return ("array", true);
    }

    match type_name(ty).as_deref() {
        Some("String") | Some("str") => ("string", true),
        Some("bool") => ("boolean", true),
        Some("f32") | Some("f64") => ("number", true),
        Some("i8") | Some("i16") | Some("i32") | Some("i64") | Some("i128") | Some("isize")
        | Some("u8") | Some("u16") | Some("u32") | Some("u64") | Some("u128") | Some("usize") => {
            ("integer", true)
        }
        Some("Value") | Some("HashMap") | Some("BTreeMap") => ("object", true),
        _ => ("object", true),
    }
}

fn single_generic_arg<'a>(ty: &'a Type, expected: &str) -> Option<&'a Type> {
    let Type::Path(path) = ty else {
        return None;
    };
    let segment = path.path.segments.last()?;
    if segment.ident != expected {
        return None;
    }
    let syn::PathArguments::AngleBracketed(args) = &segment.arguments else {
        return None;
    };
    match args.args.first()? {
        syn::GenericArgument::Type(inner) => Some(inner),
        _ => None,
    }
}

fn type_name(ty: &Type) -> Option<String> {
    match ty {
        Type::Path(path) => path
            .path
            .segments
            .last()
            .map(|segment| segment.ident.to_string()),
        Type::Reference(reference) => type_name(&reference.elem),
        _ => None,
    }
}
