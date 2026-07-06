use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{
    Attribute, Data, DeriveInput, Expr, Fields, Lit, LitStr, Meta, Pat, Type, parse::Parser,
    parse_macro_input,
};

#[proc_macro_derive(ToolDesc)]
pub fn derive_tool_desc(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    expand_tool_desc(input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

#[proc_macro]
pub fn build_prompt(input: TokenStream) -> TokenStream {
    let template = parse_macro_input!(input as LitStr);
    expand_build_prompt(template)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

#[derive(Debug)]
enum TemplateNode {
    Text(String),
    Expr(String),
    If {
        condition: String,
        then_nodes: Vec<TemplateNode>,
        else_nodes: Vec<TemplateNode>,
    },
    For {
        pattern: String,
        iterable: String,
        nodes: Vec<TemplateNode>,
    },
}

#[derive(Debug)]
enum TemplatePart {
    Text(String),
    Tag(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StopTag {
    Else,
    EndIf,
    EndFor,
}

fn expand_build_prompt(template: LitStr) -> syn::Result<TokenStream2> {
    let parts = tokenize_template(&template.value(), template.span())?;
    let mut parser = TemplateParser {
        parts: &parts,
        index: 0,
        span: template.span(),
    };
    let nodes = parser.parse_nodes(&[])?;
    if parser.index != parts.len() {
        return Err(syn::Error::new(
            template.span(),
            "unexpected trailing prompt template tag",
        ));
    }

    let writes = render_nodes(&nodes)?;
    Ok(quote! {{
        let mut __prompt = String::new();
        {
            use std::fmt::Write as _;
            #(#writes)*
        }
        __prompt
    }})
}

fn tokenize_template(src: &str, span: proc_macro2::Span) -> syn::Result<Vec<TemplatePart>> {
    let mut parts = Vec::new();
    let mut text = String::new();
    let mut chars = src.char_indices().peekable();

    while let Some((_, ch)) = chars.next() {
        match ch {
            '{' => {
                if matches!(chars.peek(), Some((_, '{'))) {
                    chars.next();
                    text.push('{');
                    continue;
                }

                if !text.is_empty() {
                    parts.push(TemplatePart::Text(std::mem::take(&mut text)));
                }

                let mut tag = String::new();
                let mut closed = false;
                for (_, tag_ch) in chars.by_ref() {
                    if tag_ch == '}' {
                        closed = true;
                        break;
                    }
                    tag.push(tag_ch);
                }

                if !closed {
                    return Err(syn::Error::new(span, "unclosed prompt template tag"));
                }
                parts.push(TemplatePart::Tag(tag.trim().to_string()));
            }
            '}' => {
                if matches!(chars.peek(), Some((_, '}'))) {
                    chars.next();
                    text.push('}');
                } else {
                    return Err(syn::Error::new(
                        span,
                        "unmatched `}` in prompt template; use `}}` for a literal brace",
                    ));
                }
            }
            _ => text.push(ch),
        }
    }

    if !text.is_empty() {
        parts.push(TemplatePart::Text(text));
    }
    Ok(parts)
}

struct TemplateParser<'a> {
    parts: &'a [TemplatePart],
    index: usize,
    span: proc_macro2::Span,
}

impl TemplateParser<'_> {
    fn parse_nodes(&mut self, stop_tags: &[StopTag]) -> syn::Result<Vec<TemplateNode>> {
        let mut nodes = Vec::new();

        while self.index < self.parts.len() {
            match &self.parts[self.index] {
                TemplatePart::Text(text) => {
                    nodes.push(TemplateNode::Text(text.clone()));
                    self.index += 1;
                }
                TemplatePart::Tag(tag) => {
                    if let Some(stop_tag) = parse_stop_tag(tag) {
                        if stop_tags.contains(&stop_tag) {
                            return Ok(nodes);
                        }
                        return Err(syn::Error::new(
                            self.span,
                            format!("unexpected prompt template tag `{{{tag}}}`"),
                        ));
                    }

                    if let Some(condition) = tag.strip_prefix("if ") {
                        nodes.push(self.parse_if(condition.trim().to_string())?);
                    } else if let Some(loop_src) = tag.strip_prefix("for ") {
                        nodes.push(self.parse_for(loop_src.trim())?);
                    } else if tag.is_empty() {
                        return Err(syn::Error::new(
                            self.span,
                            "empty prompt template tag is not allowed",
                        ));
                    } else {
                        nodes.push(TemplateNode::Expr(tag.clone()));
                        self.index += 1;
                    }
                }
            }
        }

        Ok(nodes)
    }

    fn parse_if(&mut self, condition: String) -> syn::Result<TemplateNode> {
        self.index += 1;
        let then_nodes = self.parse_nodes(&[StopTag::Else, StopTag::EndIf])?;
        let stop = self.current_stop_tag()?;
        let else_nodes = if stop == StopTag::Else {
            self.index += 1;
            let nodes = self.parse_nodes(&[StopTag::EndIf])?;
            let stop = self.current_stop_tag()?;
            if stop != StopTag::EndIf {
                return Err(syn::Error::new(self.span, "expected `{/if}`"));
            }
            nodes
        } else {
            Vec::new()
        };
        self.index += 1;

        Ok(TemplateNode::If {
            condition,
            then_nodes,
            else_nodes,
        })
    }

    fn parse_for(&mut self, loop_src: &str) -> syn::Result<TemplateNode> {
        self.index += 1;
        let Some((pattern, iterable)) = loop_src.split_once(" in ") else {
            return Err(syn::Error::new(
                self.span,
                "for prompt template tag must use `{for pattern in iterable}`",
            ));
        };

        let nodes = self.parse_nodes(&[StopTag::EndFor])?;
        let stop = self.current_stop_tag()?;
        if stop != StopTag::EndFor {
            return Err(syn::Error::new(self.span, "expected `{/for}`"));
        }
        self.index += 1;

        Ok(TemplateNode::For {
            pattern: pattern.trim().to_string(),
            iterable: iterable.trim().to_string(),
            nodes,
        })
    }

    fn current_stop_tag(&self) -> syn::Result<StopTag> {
        let Some(TemplatePart::Tag(tag)) = self.parts.get(self.index) else {
            return Err(syn::Error::new(self.span, "unclosed prompt template block"));
        };
        parse_stop_tag(tag).ok_or_else(|| {
            syn::Error::new(
                self.span,
                format!("expected prompt template block end, found `{{{tag}}}`"),
            )
        })
    }
}

fn parse_stop_tag(tag: &str) -> Option<StopTag> {
    match tag {
        "else" => Some(StopTag::Else),
        "/if" => Some(StopTag::EndIf),
        "/for" => Some(StopTag::EndFor),
        _ => None,
    }
}

fn render_nodes(nodes: &[TemplateNode]) -> syn::Result<Vec<TokenStream2>> {
    let mut writes = Vec::new();
    for node in nodes {
        writes.push(render_node(node)?);
    }
    Ok(writes)
}

fn render_node(node: &TemplateNode) -> syn::Result<TokenStream2> {
    match node {
        TemplateNode::Text(text) => Ok(quote! {
            __prompt.push_str(#text);
        }),
        TemplateNode::Expr(expr) => {
            let expr: Expr = syn::parse_str(expr)?;
            Ok(quote! {
                write!(&mut __prompt, "{}", #expr).expect("writing to prompt string cannot fail");
            })
        }
        TemplateNode::If {
            condition,
            then_nodes,
            else_nodes,
        } => {
            let condition: Expr = syn::parse_str(condition)?;
            let then_writes = render_nodes(then_nodes)?;
            let else_writes = render_nodes(else_nodes)?;
            Ok(quote! {
                if #condition {
                    #(#then_writes)*
                } else {
                    #(#else_writes)*
                }
            })
        }
        TemplateNode::For {
            pattern,
            iterable,
            nodes,
        } => {
            let pattern = Pat::parse_single.parse_str(pattern)?;
            let iterable: Expr = syn::parse_str(iterable)?;
            let writes = render_nodes(nodes)?;
            Ok(quote! {
                for #pattern in #iterable {
                    #(#writes)*
                }
            })
        }
    }
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
        let extra = array_items_extra(&field.ty);

        property_inserts.push(quote! {
            properties.insert(
                #name.to_string(),
                llm::FunctionProperty {
                    r#type: #ty.to_string(),
                    description: #description.to_string(),
                    extra: #extra,
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
        impl #impl_generics stride_agent::ToolDesc for #struct_name #ty_generics #where_clause {
            fn function_parameters() -> llm::FunctionParameters {
                let mut properties = std::collections::HashMap::new();
                let mut required: Vec<String> = Vec::new();

                #(#property_inserts)*
                #(#required_pushes)*

                llm::FunctionParameters {
                    param_type: "object".to_string(),
                    properties,
                    required: #required_expr,
                    ..Default::default()
                }
            }

            fn decode(value: serde_json::Value) -> Result<Self, String> {
                Self::try_from(value)
            }
        }

        impl #impl_generics From<#struct_name #ty_generics> for llm::FunctionParameters #where_clause {
            fn from(_: #struct_name #ty_generics) -> Self {
                <#struct_name #ty_generics as stride_agent::ToolDesc>::function_parameters()
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

/// JSON Schema requires array types to declare an `items` schema; strict
/// providers (Google/Gemini) reject a bare `{"type":"array"}`. For `Vec<T>`
/// fields this emits the `items` entry into the property's `extra` map, using
/// the element type's schema type.
fn array_items_extra(ty: &Type) -> TokenStream2 {
    let unwrapped = single_generic_arg(ty, "Option").unwrap_or(ty);
    if let Some(inner) = single_generic_arg(unwrapped, "Vec") {
        let inner_ty = schema_type(inner).0;
        quote! {{
            let mut extra = serde_json::Map::new();
            extra.insert(
                "items".to_string(),
                serde_json::json!({ "type": #inner_ty }),
            );
            extra
        }}
    } else {
        quote! { Default::default() }
    }
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
