use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::parse::{Parse, ParseStream, Parser};
use syn::punctuated::Punctuated;
use syn::{Expr, ExprLit, Fields, Ident, ItemStruct, Lit, Token, parenthesized};

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
    let mut toml_inserts = vec![];
    let mut toml_cloud_inserts = vec![];
    let mut field_updaters = vec![];
    let mut field_refreshes = vec![];

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
                        let mut uses_cloud: Option<Option<bool>> = None;
                        let mut section_expr: Option<Expr> = None;
                        let mut default_expr = None;

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
                                    uses_cloud = match value {
                                        Expr::Path(path) if path.path.is_ident("None") => {
                                            Some(None)
                                        }
                                        Expr::Call(call)
                                            if matches!(
                                                call.func.as_ref(),
                                                Expr::Path(path) if path.path.is_ident("Some")
                                            ) =>
                                        {
                                            let Some(Expr::Lit(ExprLit {
                                                lit: Lit::Bool(lit_bool),
                                                ..
                                            })) = call.args.first()
                                            else {
                                                panic!(
                                                    "`uses_cloud` must be `None` or `Some(bool)`"
                                                );
                                            };
                                            Some(Some(lit_bool.value))
                                        }
                                        _ => panic!("`uses_cloud` must be `None` or `Some(bool)`"),
                                    };
                                }
                                "section" => {
                                    section_expr = Some(value.clone());
                                }
                                "default" => {
                                    default_expr = Some(quote! { #value });
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
                        let default_expr = default_expr
                            .unwrap_or_else(|| panic!("`setting` requires `default = ...`"));
                        let uses_cloud = uses_cloud
                            .unwrap_or_else(|| panic!("`setting` requires `uses_cloud = ...`"));

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

            let type_name = format!("{}", field_name);

            if let Some((human_readable, description, uses_cloud, section_expr, default_expr)) =
                setting_meta
            {
                field.ty = syn::parse2(quote! { MatrixSettingField<#original_type> }).unwrap();

                toml_inserts.push(quote! {
                    if let Err(e) = self.#field_name.insert_into_toml_with_description(&mut table) {
                        ::tracing::error!("Failed to insert field {} into TOML: {}", #type_name, e);
                    }
                });
                toml_cloud_inserts.push(quote! {
                    self.#field_name.insert_into_toml_cloud(&mut table);
                });

                let set_cloud_name = format_ident!("set_cloud_{}", field_name);
                let set_name = format_ident!("set_{}", field_name);
                let get_name = format_ident!("get_{}", field_name);
                let watch_name = format_ident!("watch_{}", field_name);
                field_updaters.push(quote! {
                    pub async fn #set_name(&self, val: #original_type) {
                        self.#field_name.set(val, &self.document, &self.client).await;
                    }

                    pub fn #set_cloud_name(&self, uses_cloud: bool) {
                        self.#field_name.set_uses_cloud(uses_cloud, &self.document);
                    }

                    pub fn #get_name(&self) -> #original_type {
                        self.#field_name.value()
                    }

                    pub fn #watch_name(&self) -> ::tokio::sync::watch::Receiver<#original_type> {
                        self.#field_name.watch()
                    }
                });

                field_refreshes.push(quote! {
                    settings.#field_name.refresh(&settings.document, &settings.client, settings.file_last_chaned.load(::std::sync::atomic::Ordering::Relaxed)).await;
                });

                let cloud_name =
                    quote! { ::const_format::formatcp!("{APP_MATRIX_NAME}.{}", #type_name) };

                let uses_cloud_expr = match uses_cloud {
                    Some(b) => quote! { Some(::std::sync::atomic::AtomicBool::new(#b)) },
                    None => quote! { None },
                };

                default_field_initializers.push(quote! {
                    #field_name: MatrixSettingField {
                        val: ::tokio::sync::watch::Sender::new(#default_expr),
                        type_name: #type_name,
                        human_readable: #human_readable,
                        local_name: #type_name,
                        cloud_name: #cloud_name,
                        uses_cloud: #uses_cloud_expr,
                        description: #description,
                        section: #section_expr,
                    }
                });
                type_name_string_collector.push((
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

        let extra_fields = (|input: ParseStream| {
            Punctuated::<syn::Field, Token![,]>::parse_terminated_with(
                input,
                syn::Field::parse_named,
            )
        })
        .parse2(quote! {
            file_last_chaned: ::std::sync::Arc<::std::sync::atomic::AtomicI64>,
            document: ::std::sync::Arc<::std::sync::Mutex<toml_edit::DocumentMut>>,
            file_path: std::path::PathBuf,
            client: matrix_sdk::Client,
        })
        .unwrap();
        fields.named.extend(extra_fields);
    } else {
        panic!("Only applicable to structs with named fields")
    }

    let search_pushes =
        type_name_string_collector
            .iter()
            .map(|(field_name, _, human_readable, description)| {
                let type_name = format!("{}", field_name);
                quote! {
                    if #human_readable.to_lowercase().contains(&query)
                        || #description.to_lowercase().contains(&query)
                        || self.#field_name.section.id().contains(&query)
                    {
                        results.push((
                            self.#field_name.section,
                            SettingSearchResult {
                                type_name: #type_name,
                                human_readable: #human_readable,
                                description: #description,
                            },
                        ));
                    }
                }
            });

    let expanded = quote! {
        use crate::APP_MATRIX_NAME;
        use std::str::FromStr;

        #[derive(Debug)]
        pub struct MatrixSettingField<T: 'static> {
            val: ::tokio::sync::watch::Sender<T>,
            pub type_name: &'static str,
            pub human_readable: &'static str,
            pub local_name: &'static str,
            pub cloud_name: &'static str,
            uses_cloud: Option<::std::sync::atomic::AtomicBool>,
            pub description: &'static str,
            pub section: SettingsSection,
        }

        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub struct SettingSearchResult {
            pub type_name: &'static str,
            pub human_readable: &'static str,
            pub description: &'static str,
        }

        #[derive(Debug, Clone, Serialize, Deserialize)]
        pub struct CloudSetting<T: 'static> {
            value: T,
            last_changed: i64,
        }

        impl<T: Clone + 'static> Clone for MatrixSettingField<T> {
            fn clone(&self) -> Self {
                MatrixSettingField {
                    val: self.val.clone(),
                    type_name: self.type_name,
                    human_readable: self.human_readable,
                    local_name: self.local_name,
                    cloud_name: self.cloud_name.clone(),
                    uses_cloud: self.uses_cloud.as_ref().map(|c| ::std::sync::atomic::AtomicBool::new(c.load(::std::sync::atomic::Ordering::Relaxed))),
                    description: self.description,
                    section: self.section,
                }
            }
        }

        impl<T> MatrixSettingField<T>
        where
            T: Clone + Default + ::serde::Serialize + ::serde::de::DeserializeOwned,
        {
            /// Cheap snapshot of the current value.
            pub fn value(&self) -> T {
                self.val.borrow().clone()
            }

            /// Subscribe to reactive updates of the current value.
            pub fn watch(&self) -> ::tokio::sync::watch::Receiver<T> {
                self.val.subscribe()
            }

            fn uses_cloud(&self) -> bool {
                self.uses_cloud
                    .as_ref()
                    .map(|c| c.load(::std::sync::atomic::Ordering::Relaxed))
                    .unwrap_or(false)
            }

            fn to_raw(&self) -> ::anyhow::Result<::ruma::serde::Raw<::ruma::events::AnyGlobalAccountDataEventContent>> {
                let now = chrono::Utc::now();

                let setting = CloudSetting { value: self.value(), last_changed: now.timestamp() };
                ::serde_json::value::to_raw_value(&setting).map(|v| ::ruma::serde::Raw::from_json(v)).map_err(|e| ::anyhow::anyhow!(e))
            }

            fn insert_into_toml(&self, table: &mut toml_edit::Table) -> ::anyhow::Result<()> {
                let toml_value = self.value()
                    .serialize(toml_edit::ser::ValueSerializer::new())?;

                let existing = table.get(self.local_name).and_then(|item| item.as_value());
                if existing.map(|v| v.to_string()) == Some(toml_value.to_string()) {
                    return Ok(());
                }

                if let Some(item) = table.get_mut(self.local_name) {
                    *item = toml_edit::Item::Value(toml_value);
                } else {
                    table.insert(self.local_name, toml_edit::Item::Value(toml_value));
                }


                Ok(())
            }

            fn insert_into_toml_with_description(&self, table: &mut toml_edit::Table) -> ::anyhow::Result<()> {
                let toml_value = self.value()
                    .serialize(toml_edit::ser::ValueSerializer::new())?;
                table.insert(self.local_name, toml_edit::Item::Value(toml_value));

                if let Some(mut key) = table.key_mut(self.local_name) {
                    key.leaf_decor_mut()
                        .set_prefix(format!("# {}\n", self.description));
                }

                Ok(())
            }

            fn insert_into_toml_cloud(&self, table: &mut toml_edit::Table) {
                if self.uses_cloud.is_none() {
                    return;
                }

                table.insert(self.local_name, toml_edit::Item::Value(toml_edit::Value::Boolean(toml_edit::Formatted::new(self.uses_cloud()))));
            }

            fn set_uses_cloud(&self, uses_cloud: bool, document: &::std::sync::Mutex<toml_edit::DocumentMut>) {
                let Some(cell) = &self.uses_cloud else {
                    tracing::warn!("Cannot set uses_cloud to true when uses_cloud is not set");
                    return;
                };
                cell.store(uses_cloud, ::std::sync::atomic::Ordering::Relaxed);

                let mut document = document.lock().unwrap();
                let Some(table) = document.get_mut(CLOUD_TABLE).and_then(|item| item.as_table_mut()) else {
                    tracing::warn!("Cloud table not found, not updating table");
                    return;
                };

                self.insert_into_toml_cloud(table);
            }

            async fn set(&self, val: T, document: &::std::sync::Mutex<toml_edit::DocumentMut>, client: &matrix_sdk::Client) {
                self.val.send_replace(val);

                {
                    let mut document = document.lock().unwrap();
                    let Some(table) = document.get_mut(SETTINGS_TABLE).and_then(|item| item.as_table_mut()) else {
                        tracing::warn!("Settings table not found, not updating table");
                        return;
                    };

                    if let Err(e) = self.insert_into_toml(table) {
                        tracing::error!("Failed to insert into TOML: {:?}", e);
                    }
                }

                if self.uses_cloud() {
                    if let Err(e) = set_field_cloud(client, self).await {
                        tracing::error!("Failed to save setting {}: {:?}", self.type_name, e);
                    };
                }
            }

            async fn refresh(&self, document: &::std::sync::Mutex<toml_edit::DocumentMut>, client: &matrix_sdk::Client, file_last_changed: i64) {
                let mut missing_from_cloud = false;
                let cloud_setting = if self.uses_cloud() {
                    match get_field_cloud::<T>(client.clone(), &self.cloud_name).await {
                        Ok(Some(setting)) => Some(setting),
                        Ok(None) => {
                            missing_from_cloud = true;
                            None
                        }
                        Err(e) => {
                            tracing::error!("Failed to get field {} from cloud: {:?}", self.type_name, e);
                            None
                        }
                    }
                } else {
                    None
                };

                {
                    let mut document = document.lock().unwrap();
                    let Some(table) = document.get_mut(SETTINGS_TABLE).and_then(|item| item.as_table_mut()) else {
                        tracing::warn!("Settings table not found, not updating table");
                        return;
                    };

                    let local_setting = get_field_local(table, &self.local_name).unwrap_or_else(|e| {
                        tracing::error!("Failed to get local setting {}: {:?}", self.type_name, e);
                        T::default()
                    });

                    let newer = match &cloud_setting {
                        Some(cs) if cs.last_changed > file_last_changed => cs.value.clone(),
                        _ => local_setting,
                    };

                    self.val.send_replace(newer);
                    if let Err(e) = self.insert_into_toml(table) {
                        tracing::error!("Failed to insert into TOML: {:?}", e);
                    }
                }

                if missing_from_cloud {
                    if let Err(e) = set_field_cloud(client, self).await {
                        tracing::error!("Failed to seed cloud value for setting {}: {:?}", self.type_name, e);
                    }
                }
            }
        }

        #[derive(Clone)]
        #item

        impl #struct_name {
            pub fn new(file_path: std::path::PathBuf, client: matrix_sdk::Client) -> Self {
                let existing = if file_path.exists() {
                    let content = std::fs::read_to_string(&file_path).map_err(|e| ::tracing::error!("Failed to read settings file: {:?}", e)).ok();

                    content.map(|c| toml_edit::DocumentMut::from_str(&c).map_err(|e| ::tracing::error!("Failed to parse settings file: {:?}", e)).ok()).flatten()
                } else {
                    None
                };

                let existing_is_none = existing.is_none();

                let settings = Self {
                    #(#default_field_initializers,)*
                    document: ::std::sync::Arc::new(::std::sync::Mutex::new(existing.unwrap_or_default())),
                    file_last_chaned: ::std::sync::Arc::new(::std::sync::atomic::AtomicI64::new(chrono::Utc::now().timestamp())),
                    file_path,
                    client,
                };

                if existing_is_none {
                    settings.create_settings_table();
                    settings.create_cloud_table();
                    settings.save();
                }

                settings
            }

            /// Pulls the latest value for every cloud-backed setting and merges it with the
            /// local copy. Call this once a client is authenticated; calling it before then
            /// just fails the cloud fetch and falls back to the local value.
            pub async fn refresh(&self) {
                let settings = self;
                #(#field_refreshes)*
                settings.save();
                settings.update_file_last_changed();
            }
        }

        impl #struct_name {
            pub fn search(&self, query: &str) -> Vec<(SettingsSection, SettingSearchResult)> {
                let query = query.to_lowercase();
                let mut results = Vec::new();
                #(#search_pushes)*
                results
            }

             fn create_settings_table(&self) {
                let mut table = toml_edit::Table::new();
                #(#toml_inserts)*

                self.document.lock().unwrap().insert(SETTINGS_TABLE, toml_edit::Item::Table(table));
            }

            fn create_cloud_table(&self) {
                let mut table = toml_edit::Table::new();
                #(#toml_cloud_inserts)*

                self.document.lock().unwrap().insert(CLOUD_TABLE, toml_edit::Item::Table(table));
            }

            fn save(&self) {
                let content = self.document.lock().unwrap().to_string();
                if let Err(e) = std::fs::write(&self.file_path, content) {
                    tracing::error!("Failed to save settings: {}", e);
                }
                self.file_last_chaned.store(chrono::Utc::now().timestamp(), ::std::sync::atomic::Ordering::Relaxed);
            }

            fn update_file_last_changed(&self) {
                match std::fs::metadata(&self.file_path).and_then(|metadata| metadata.modified()) {
                    Ok(modified) => {
                        let secs = modified
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_secs() as i64)
                            .unwrap_or_else(|_| chrono::Utc::now().timestamp());
                        self.file_last_chaned.store(secs, ::std::sync::atomic::Ordering::Relaxed);
                    }
                    Err(e) => {
                        tracing::warn!("Failed to get file metadata: {}", e);
                        self.file_last_chaned.store(chrono::Utc::now().timestamp(), ::std::sync::atomic::Ordering::Relaxed);
                    }
                }
            }

            #(#field_updaters)*
        }
    };

    expanded.into()
}
