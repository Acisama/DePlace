use proc_macro::TokenStream;
use proc_macro2::TokenTree;
use quote::{format_ident, quote};
use syn::parse::discouraged::Speculative;
use syn::parse::{Parse, ParseStream};
use syn::{Expr, Ident, Token};

/// Either a bare icon/weight name, or `<bool expr> ? <name> : <name>`.
enum IconPart {
    Ident(Ident),
    Ternary {
        cond: Box<Expr>,
        if_true: Ident,
        if_false: Ident,
    },
}

impl Parse for IconPart {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        // Fast path: a bare ident with nothing else before the next `,`.
        let fork = input.fork();
        if let Ok(ident) = fork.parse::<Ident>()
            && (fork.peek(Token![,]) || fork.is_empty())
        {
            input.advance_to(&fork);
            return Ok(IconPart::Ident(ident));
        }

        // Otherwise, scan raw tokens up to the top-level `?` and parse them as the condition.
        let mut cond_tokens = proc_macro2::TokenStream::new();
        loop {
            if input.peek(Token![?]) {
                break;
            }
            if input.is_empty() {
                return Err(input.error("expected `?` in `<cond> ? <name> : <name>`"));
            }
            let tt: TokenTree = input.parse()?;
            cond_tokens.extend(std::iter::once(tt));
        }
        let cond: Expr = syn::parse2(cond_tokens)?;

        input.parse::<Token![?]>()?;
        let if_true: Ident = input.parse()?;
        input.parse::<Token![:]>()?;
        let if_false: Ident = input.parse()?;

        Ok(IconPart::Ternary {
            cond: Box::new(cond),
            if_true,
            if_false,
        })
    }
}

struct PhosphorIconInput {
    name: IconPart,
    weight: IconPart,
    size: Expr,
    color: Option<Expr>,
}

impl Parse for PhosphorIconInput {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let name: IconPart = input.parse()?;
        input.parse::<Token![,]>()?;
        let weight: IconPart = input.parse()?;
        input.parse::<Token![,]>()?;
        let size: Expr = input.parse()?;

        let mut color = None;
        if input.parse::<Option<Token![,]>>()?.is_some() && !input.is_empty() {
            color = Some(input.parse()?);
            input.parse::<Option<Token![,]>>()?;
        }

        Ok(Self {
            name,
            weight,
            size,
            color,
        })
    }
}

fn icon_path(name: &Ident, weight: &Ident) -> proc_macro2::TokenStream {
    let weight = format_ident!("{}", weight.to_string().to_uppercase());
    quote! { ::phosphor_svgs::icon::#name::#weight }
}

/// Builds the `&'static str` path expression for a fixed name and a (possibly conditional) weight.
fn build_path_for_name(name: &Ident, weight: &IconPart) -> proc_macro2::TokenStream {
    match weight {
        IconPart::Ident(weight) => icon_path(name, weight),
        IconPart::Ternary {
            cond,
            if_true,
            if_false,
        } => {
            let true_path = icon_path(name, if_true);
            let false_path = icon_path(name, if_false);
            quote! { if #cond { #true_path } else { #false_path } }
        }
    }
}

/// Builds the full `&'static str` path expression, handling either or both of
/// `name`/`weight` being conditional.
fn build_path(name: &IconPart, weight: &IconPart) -> proc_macro2::TokenStream {
    match name {
        IconPart::Ident(name) => build_path_for_name(name, weight),
        IconPart::Ternary {
            cond,
            if_true,
            if_false,
        } => {
            let true_branch = build_path_for_name(if_true, weight);
            let false_branch = build_path_for_name(if_false, weight);
            quote! { if #cond { #true_branch } else { #false_branch } }
        }
    }
}

pub fn iced_icon(input: TokenStream) -> TokenStream {
    let PhosphorIconInput {
        name,
        weight,
        size,
        color,
    } = syn::parse_macro_input!(input as PhosphorIconInput);

    let path = build_path(&name, &weight);
    let color_call = color.map(|color| quote! { .color(#color) });

    let expanded = quote! {
        crate::components::phosphor_icon(#path, #size)
            #color_call
    };

    expanded.into()
}
