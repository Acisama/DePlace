use proc_macro::TokenStream;
use quote::quote;
use syn::{
    DeriveInput, Expr, ExprLit, Fields, Lit, Path, Token, parse_macro_input, punctuated::Punctuated,
};

mod iced_cache;
mod phosphor_icon;
mod section;
mod settings;

/// Generates a settings-section widget struct from a declarative field tree.
///
/// ```rust
/// section!(GeneralSection, [
///     section("Language/Region", [ hour_format, date_format ]),
///     minimize_to_tray,
/// ]);
/// ```
///
/// Each bare identifier must name a field of `deplace_core::settings::Settings`;
/// its rendering (toggle/dropdown/etc) is picked by the `SettingWidget` impl for
/// that field's type, so the macro itself never needs to know field types.
#[proc_macro]
pub fn section(input: TokenStream) -> TokenStream {
    section::section(input)
}

/// `#[matrix_settings(namespace = "...")]` -- the namespace is inserted only
/// into each field's cloud (matrix account-data) event type, as
/// `{APP_MATRIX_NAME}.{namespace}.{field}`. It keeps multiple
/// `#[matrix_settings]` structs (e.g. one for general settings, one for
/// theme) from colliding in the cloud namespace; local TOML storage is
/// unaffected since each struct is already backed by its own file.
#[proc_macro_attribute]
pub fn matrix_settings(attr: TokenStream, item: TokenStream) -> TokenStream {
    let namespace = syn::parse_macro_input!(attr as settings::MatrixSettingsArgs).namespace;
    let item_ast = syn::parse(item).unwrap();

    settings::convert_settings(item_ast, &namespace.value())
}

/// Macro to derive hash on a state struct
///
/// Within a struct decorated with this macro, you can add the `#[hash]` attribute to a field, making it
/// so that these fields are taken into consideration for the hash computed over the struct.
///
/// Hash is used extensivel in this crate with the lazy widget, which is why this this macro was created.
///
/// This will treat several fields in a special way, depending on the field name, for instance adding
/// functions.
/// Since the original author didn't bother documenting anything when creating this macro, you will have
/// to read through the definition to find out what exactly is happening, and you will even have to read
/// through the rest of the code to find out how the functions created by this macro are to be tied in.
#[proc_macro_attribute]
pub fn iced_cache(attr: TokenStream, item: TokenStream) -> TokenStream {
    let derives = parse_macro_input!(attr with Punctuated::<Path, Token![,]>::parse_terminated);
    let item_ast = syn::parse(item).unwrap();

    iced_cache::convert_iced(item_ast, derives)
}

/// Builds a `PhosphorIcon` from a bare icon name, e.g. `phosphor_icon!(hash, bold, 16.0)`.
/// An optional trailing expression sets the color: `phosphor_icon!(hash, bold, 16.0, theme.text.dim)`.
#[proc_macro]
pub fn iced_icon(input: TokenStream) -> TokenStream {
    phosphor_icon::iced_icon(input)
}

/// Safely creates a `NonZeroUsize` at compile time.
#[proc_macro]
pub fn nonzero_usize(input: TokenStream) -> TokenStream {
    let expr = parse_macro_input!(input as Expr);

    if let Expr::Lit(ExprLit {
        lit: Lit::Int(lit_int),
        ..
    }) = &expr
        && lit_int.base10_digits() == "0"
    {
        let error = quote! {
            compile_error!("Compilation failed: Value cannot be zero!");
        };
        return error.into();
    }

    let expanded = quote! {
        const {
            match ::core::num::NonZeroUsize::new(#expr) {
                Some(nz) => nz,
                None => panic!("Compilation failed: Value evaluated to zero!"),
            }
        }
    };

    expanded.into()
}

#[proc_macro_derive(EnumConstVec)]
pub fn derive_enum_const_vec(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as DeriveInput);
    let ident = &input.ident;

    let syn::Data::Enum(data) = &input.data else {
        panic!("EnumConstVec can only be derived for enums");
    };

    let entries = data.variants.iter().map(|variant| {
        assert!(
            matches!(variant.fields, Fields::Unit),
            "EnumVariants only supports fieldless variants, but `{}` has fields",
            variant.ident
        );

        let variant_ident = &variant.ident;

        quote! { #ident::#variant_ident }
    });

    quote! {
        impl #ident {
            const fn const_vec() -> &'static [Self] {
                &[#(#entries),*]
            }
        }
    }
    .into()
}

#[proc_macro_derive(EnumVariants)]
pub fn derive_enum_variants(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as DeriveInput);
    let ident = &input.ident;

    let syn::Data::Enum(data) = &input.data else {
        panic!("EnumVariants can only be derived for enums");
    };

    let entries = data.variants.iter().map(|variant| {
        assert!(
            matches!(variant.fields, Fields::Unit),
            "EnumVariants only supports fieldless variants, but `{}` has fields",
            variant.ident
        );

        let variant_ident = &variant.ident;
        let name = serde_variant_name(variant);

        quote! { (#ident::#variant_ident, #name) }
    });

    quote! {
        impl EnumVariants for #ident {
            fn all_variants() -> impl Iterator<Item = (#ident, &'static str)> {
                [#(#entries),*].into_iter()
            }
        }
    }
    .into()
}

/// Mirrors serde's `#[serde(rename = "...")]`, falling back to the
/// variant's own name, so the string matches what serde_json produces.
fn serde_variant_name(variant: &syn::Variant) -> String {
    let mut name = variant.ident.to_string();
    for attr in &variant.attrs {
        if !attr.path().is_ident("serde") {
            continue;
        }
        let _ = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("rename") {
                name = meta.value()?.parse::<syn::LitStr>()?.value();
            }
            Ok(())
        });
    }
    name
}
