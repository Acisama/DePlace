use proc_macro::TokenStream;
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{Expr, Ident, Token, parenthesized};

struct StyleCall {
    name: Ident,
    args: Option<Punctuated<Expr, Token![,]>>,
}

impl Parse for StyleCall {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let name: Ident = input.parse()?;

        let args = if input.peek(syn::token::Paren) {
            let content;
            parenthesized!(content in input);
            Some(content.parse_terminated(Expr::parse, Token![,])?)
        } else {
            None
        };

        Ok(StyleCall { name, args })
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Modifier {
    None,
    Hover,
}

struct StyleList {
    base_calls: Vec<StyleCall>,
    hover_calls: Vec<StyleCall>,
}

impl Parse for StyleList {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut base_calls = Vec::new();
        let mut hover_calls = Vec::new();

        while !input.is_empty() {
            if input.peek(Token![,]) {
                input.parse::<Token![,]>()?;
                continue;
            }

            if input.peek(Ident) {
                let forked = input.fork();
                let ident: Ident = forked.parse()?;

                if ident == "hover" && forked.peek(syn::token::Paren) {
                    input.parse::<Ident>()?;
                    let content;
                    parenthesized!(content in input);

                    while !content.is_empty() {
                        if content.peek(Token![,]) {
                            content.parse::<Token![,]>()?;
                            continue;
                        }
                        hover_calls.push(content.parse()?);
                    }
                    continue;
                }
            }

            base_calls.push(input.parse()?);
        }

        Ok(StyleList {
            base_calls,
            hover_calls,
        })
    }
}

#[proc_macro]
pub fn tailwind_div(input: TokenStream) -> TokenStream {
    let StyleList {
        base_calls,
        hover_calls,
    } = syn::parse_macro_input!(input as StyleList);

    let base_methods = base_calls.into_iter().map(|call| {
        let name = &call.name;
        match &call.args {
            Some(args) => quote! { .#name(#args) },
            None => quote! { .#name() },
        }
    });

    let hover_methods: Vec<_> = hover_calls
        .into_iter()
        .map(|call| {
            let name = &call.name;
            match &call.args {
                Some(args) => quote! { .#name(#args) },
                None => quote! { .#name() },
            }
        })
        .collect();

    let hover_block = if !hover_methods.is_empty() {
        quote! {
            .hover(|style| style #( #hover_methods )* )
        }
    } else {
        quote! {}
    };

    let expanded = quote! {
        gpui::div()
            #( #base_methods )*
            #hover_block
    };

    TokenStream::from(expanded)
}
