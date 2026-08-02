use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{DeriveInput, Expr, ExprLit, Fields, Ident, ItemStruct, Lit, Token, parenthesized};

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

#[proc_macro_attribute]
pub fn matrix_settings(_attr: TokenStream, item: TokenStream) -> TokenStream {
    assert!(_attr.is_empty());
    let item_ast = syn::parse(item).unwrap();

    convert_settings(item_ast)
}

fn convert_settings(mut item: ItemStruct) -> TokenStream {
    let struct_name = item.ident.clone();
    let mut default_field_initializers = vec![];
    let mut type_name_string_collector = vec![];

    if let Fields::Named(ref mut fields) = item.fields {
        for field in &mut fields.named {
            let original_type = field.ty.clone();
            let field_name = field.ident.as_ref().unwrap().clone();

            let mut setting_meta = None;

            field.attrs.retain(|attr| {
                if attr.path().is_ident("setting") {
                    if let Ok(exprs) =
                        attr.parse_args_with(Punctuated::<Expr, Token![,]>::parse_terminated)
                    {
                        let mut human_readable: Option<String> = None;
                        let mut description: Option<String> = None;
                        let mut uses_cloud = false;
                        let mut section_expr: Option<Expr> = None;
                        let mut default_expr = quote! { Default::default() };

                        for expr in exprs.iter() {
                            let Expr::Assign(assign) = expr else {
                                panic!(
                                    "Each `setting` argument must be `key = value`, found `{}`",
                                    quote!(#expr)
                                );
                            };
                            let Expr::Path(key_path) = assign.left.as_ref() else {
                                panic!("`setting` argument keys must be plain identifiers");
                            };
                            let key = key_path
                                .path
                                .get_ident()
                                .expect("`setting` argument keys must be plain identifiers")
                                .to_string();
                            let value = assign.right.as_ref();

                            match key.as_str() {
                                "name" => {
                                    let Expr::Lit(ExprLit {
                                        lit: Lit::Str(lit_str),
                                        ..
                                    }) = value
                                    else {
                                        panic!("`name` must be a string literal");
                                    };
                                    human_readable = Some(lit_str.value());
                                }
                                "description" => {
                                    let Expr::Lit(ExprLit {
                                        lit: Lit::Str(lit_str),
                                        ..
                                    }) = value
                                    else {
                                        panic!("`description` must be a string literal");
                                    };
                                    description = Some(lit_str.value());
                                }
                                "uses_cloud" => {
                                    let Expr::Lit(ExprLit {
                                        lit: Lit::Bool(lit_bool),
                                        ..
                                    }) = value
                                    else {
                                        panic!("`uses_cloud` must be a bool literal");
                                    };
                                    uses_cloud = lit_bool.value;
                                }
                                "section" => {
                                    section_expr = Some(value.clone());
                                }
                                "default" => {
                                    default_expr = quote! { #value };
                                }
                                other => panic!("Unknown `setting` argument `{other}`"),
                            }
                        }

                        let human_readable = human_readable
                            .unwrap_or_else(|| panic!("`setting` requires `name = \"...\"`"));
                        let description = description.unwrap_or_else(|| {
                            panic!("`setting` requires `description = \"...\"`")
                        });
                        let section_expr = section_expr
                            .unwrap_or_else(|| panic!("`setting` requires `section = ...`"));

                        setting_meta = Some((
                            human_readable,
                            description,
                            uses_cloud,
                            section_expr,
                            default_expr,
                        ));
                    }
                    false
                } else {
                    true
                }
            });

            if let Some((human_readable, description, uses_cloud, section_expr, default_expr)) =
                setting_meta
            {
                let type_name_str = format!("{}.{}", deplace_core::APP_MATRIX_NAME, field_name);

                field.ty = syn::parse2(quote! { MatrixSettingField<#original_type> }).unwrap();

                default_field_initializers.push(quote! {
                    #field_name : MatrixSettingField {
                        val: #default_expr,
                        type_name: #type_name_str,
                        human_readable: #human_readable,
                        uses_cloud: #uses_cloud,
                        description: #description,
                        section: #section_expr,
                    }
                });
                type_name_string_collector.push((
                    type_name_str.clone(),
                    field_name,
                    uses_cloud,
                    human_readable,
                    description,
                ));
            } else {
                default_field_initializers.push(quote! {
                    #field_name: Default::default()
                });
            }
        }
    } else {
        panic!("Only applicable to structs with named fields")
    }

    let signal_bindings = type_name_string_collector
        .iter()
        .map(|(_, field_name, _, _, _)| {
            quote! {
                let #field_name = self.#field_name.val;
            }
        });

    // File event: update signal; for cloud fields also push the new value up.
    let match_arms_file: Vec<_> = type_name_string_collector
        .iter()
        .map(|(type_name, field_name, uses_cloud, _, _)| {
            let upload = if *uses_cloud {
                quote! {
                    // Don't re-upload to cloud when the backend already handled it
                    // (skip_cloud_upload=true) or when the event itself came from the cloud.
                    // Only upload for genuine local-file changes detected by the watcher.
                    if new.value != "null" && !new.skip_cloud_upload && !new.cloud {
                        let key_c = new.key.clone();
                        let val_c = new.value.clone();
                        ::leptos::task::spawn_local(async move {
                            let args = ::serde_wasm_bindgen::to_value(
                                &::serde_json::json!({ "key": key_c, "value": val_c })
                            ).expect("Failed to serialize cloud-upload args");
                            if let Err(e) = call_tauri("set_setting_cloud", args).await {
                                ::log::error!("Failed to upload '{}' to cloud: {:?}", key_c, e);
                            }
                        });
                    }
                }
            } else {
                quote! {}
            };
            quote! {
                #type_name => {
                    match ::serde_json::from_str(&new.value) {
                        Ok(parsed) => {
                            if #field_name.get_untracked() != parsed {
                                #field_name.set(parsed);
                            }
                        }
                        Err(e) => ::log::warn!("Failed to deserialize field '{}' (value: {:?}): {:?}", stringify!(#field_name), new.value, e)
                    }
                    #upload
                }
            }
        })
        .collect();

    // Cloud event: just update the signal (value already came from the cloud).
    let match_arms_cloud: Vec<_> = type_name_string_collector
        .iter()
        .map(|(type_name, field_name, _, _, _)| {
            quote! {
                #type_name => match ::serde_json::from_str(&new.value) {
                    Ok(parsed) => {
                        if #field_name.get_untracked() != parsed {
                            #field_name.set(parsed);
                        }
                    }
                    Err(e) => ::log::warn!("Failed to deserialize field '{}' (value: {:?}): {:?}", stringify!(#field_name), new.value, e)
                }
            }
        })
        .collect();

    let get_all_calls = type_name_string_collector
        .iter()
        .map(|(_, field_name, _, _, _)| {
            quote! { self.#field_name.fetch().await?; }
        });

    let search_pushes = type_name_string_collector.iter().map(
        |(type_name, field_name, _, human_readable, description)| {
            quote! {
                if #human_readable.to_lowercase().contains(&query)
                    || #description.to_lowercase().contains(&query)
                    || self.#field_name.section.id().contains(&query)
                {
                    results.push((
                        self.#field_name.section,
                        Setting {
                            type_name: #type_name,
                            human_readable: #human_readable,
                            description: #description,
                        },
                    ));
                }
            }
        },
    );

    let expanded = quote! {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum SettingsSection {
            Profile,
            General,
            Appearance,
            Audio,
            Chats,
            Updates,
            Divider,
        }

        impl SettingsSection {
            pub fn id(&self) -> &'static str {
                match self {
                    SettingsSection::Profile => "profile",
                    SettingsSection::General => "general",
                    SettingsSection::Appearance => "appearance",
                    SettingsSection::Audio => "audio",
                    SettingsSection::Chats => "chats",
                    SettingsSection::Updates => "updates",
                    SettingsSection::Divider => "divider",
                }
            }
        }

        #[derive(Debug)]
        pub struct MatrixSettingField<T: 'static> {
            pub val: T,
            pub type_name: &'static str,
            pub human_readable: &'static str,
            pub uses_cloud: Option<bool>,
            pub last_changed: i64,
            pub description: &'static str,
            pub section: SettingsSection,
        }

        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub struct Setting {
            pub type_name: &'static str,
            pub human_readable: &'static str,
            pub description: &'static str,
        }

        pub struct CloudSetting<T: 'static> {
            value: T,
            last_changed: i64,
        }

        impl<T: 'static> Copy for MatrixSettingField<T> {}
        impl<T: 'static> Clone for MatrixSettingField<T> {
            fn clone(&self) -> Self { *self }
        }

        impl<T> MatrixSettingField<T>
        where
            T: 'static + Send + Sync + Clone + ::serde::Serialize + ::serde::de::DeserializeOwned,
        {
            /// Updates the signal and persists the new value to the backend
            /// (and cloud, if `uses_cloud`).
            pub fn set(&self, val: T) {
                let serialized = match ::serde_json::to_string(&val) {
                    Ok(s) => s,
                    Err(e) => {
                        ::log::error!("Failed to serialize {}: {:?}", self.type_name, e);
                        return;
                    }
                };
                self.val.set(val);
                let type_name = self.type_name;
                let uses_cloud = self.uses_cloud;
                ::leptos::task::spawn_local(async move {
                    let args = ::serde_wasm_bindgen::to_value(
                        &::serde_json::json!({ "key": type_name, "value": serialized, "to_cloud": uses_cloud })
                    ).expect("Failed to serialize args");
                    if let Err(e) = call_tauri("set_setting", args).await {
                        ::log::error!("Failed to save setting {}: {:?}", type_name, e);
                    }
                });
            }

            /// Fetches the value from the backend (or cloud, if `uses_cloud`)
            /// and updates the signal, persisting the current default if unset.
            pub async fn fetch(&self) -> Result<(), String> {
                let signal = self.val;
                let type_name = self.type_name;
                let uses_cloud = self.uses_cloud;
                let args = ::serde_wasm_bindgen::to_value(
                    &::serde_json::json!({ "key": type_name, "from_cloud": uses_cloud })
                ).map_err(|e| format!("Failed to serialize args: {:?}", e))?;
                let res = call_tauri("get_setting", args)
                    .await
                    .map_err(|e| format!("Tauri call failed: {:?}", e))?;
                let json_str: Option<String> = ::serde_wasm_bindgen::from_value(res)
                    .map_err(|e| format!("Failed to deserialize response: {:?}", e))?;
                if let Some(s) = json_str {
                    let val: T = ::serde_json::from_str(&s)
                        .map_err(|e| format!("Failed to parse value: {:?}", e))?;
                    signal.set(val);
                } else {
                    let serialized = ::serde_json::to_string(&signal.get_untracked())
                        .map_err(|e| format!("Failed to serialize default: {:?}", e))?;
                    let set_args = ::serde_wasm_bindgen::to_value(
                        &::serde_json::json!({ "key": type_name, "value": serialized, "to_cloud": uses_cloud })
                    ).map_err(|e| format!("Failed to serialize set args: {:?}", e))?;
                    if let Err(e) = call_tauri("set_setting", set_args).await {
                        ::log::warn!("Failed to persist default for {}: {:?}", type_name, e);
                    }
                }
                Ok(())
            }
        }

        #[derive(Clone, Copy)]
        #item

        impl Default for #struct_name {
            fn default() -> Self {
                Self {
                    #(#default_field_initializers),*
                }
            }
        }

        impl #struct_name {
            pub async fn get_all(&self) -> Result<(), String> {
                ::tracing::debug!("Getting all settings");
                #(#get_all_calls)*
                Ok(())
            }

            pub fn search(&self, query: &str) -> Vec<(SettingsSection, Setting)> {
                let query = query.to_lowercase();
                let mut results = Vec::new();
                #(#search_pushes)*
                results
            }
        }
    };

    expanded.into()
}
