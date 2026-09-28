use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{Ident, LitStr, Token, Type, bracketed, parenthesized};

/// One entry inside a `section!` field list: either `field_name: FieldType`
/// for a `Settings` field, or a nested `section("Title", [ ...items ])` group.
enum Item {
    Spacer,
    Field(Ident, Box<Type>),
    Group(LitStr, Vec<Item>),
}

impl Parse for Item {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let fork = input.fork();
        if let Ok(ident) = fork.parse::<Ident>()
            && fork.peek(syn::token::Paren)
        {
            if ident == "section" {
                input.parse::<Ident>()?;

                let paren_content;
                parenthesized!(paren_content in input);

                let title: LitStr = paren_content.parse()?;
                paren_content.parse::<Token![,]>()?;

                let bracket_content;
                bracketed!(bracket_content in paren_content);

                let items = Punctuated::<Item, Token![,]>::parse_terminated(&bracket_content)?;

                return Ok(Item::Group(title, items.into_iter().collect()));
            } else if ident == "spacer" {
                input.parse::<Ident>()?;

                let paren_content;
                parenthesized!(paren_content in input);
                return Ok(Item::Spacer);
            }
        }

        let field: Ident = input.parse()?;
        input.parse::<Token![:]>()?;
        let ty: Type = input.parse()?;
        Ok(Item::Field(field, Box::new(ty)))
    }
}

struct SectionInput {
    struct_name: Ident,
    items: Vec<Item>,
}

impl Parse for SectionInput {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let struct_name: Ident = input.parse()?;
        input.parse::<Token![,]>()?;
        let bracket_content;
        bracketed!(bracket_content in input);
        let items = Punctuated::<Item, Token![,]>::parse_terminated(&bracket_content)?;
        let _ = input.parse::<Token![,]>();
        Ok(Self {
            struct_name,
            items: items.into_iter().collect(),
        })
    }
}

fn pascal_case(name: &str) -> String {
    name.split('_')
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect()
}

/// Walks the item tree, collecting `Message` variants / `update` match arms
/// as a side effect, and returning the `view` token stream for this level.
fn walk(
    items: &[Item],
    struct_name: &Ident,
    message_name: &Ident,
    action_name: &Ident,
    message_variants: &mut Vec<proc_macro2::TokenStream>,
    update_arms: &mut Vec<proc_macro2::TokenStream>,
    hash_fields: &mut Vec<Ident>,
) -> Vec<proc_macro2::TokenStream> {
    items
        .iter()
        .map(|item| match item {
            Item::Field(field, ty) => {
                let variant = format_ident!("{}", pascal_case(&field.to_string()));
                let set_name = format_ident!("set_{}", field);

                hash_fields.push(field.clone());
                message_variants.push(quote! { #variant(#ty) });

                update_arms.push(quote! {
                    #message_name::#variant(value) => {
                        let settings = self.settings.clone();
                        Some(#action_name::Run(::iced::Task::future(async move {
                            settings.#set_name(value).await;
                        })))
                    }
                });

                quote! {
                    crate::components::home::overlay::settings::widgets::SettingWidget::render(
                        &self.settings.#field,
                        theme,
                        structure,
                        #message_name::#variant,
                    )
                }
            }
            Item::Spacer => quote! {
                crate::components::home::overlay::settings::widgets::render_spacer(structure)
            },
            Item::Group(title, sub_items) => {
                let id = format!("{}::{}", struct_name, title.value());
                let sub_views = walk(
                    sub_items,
                    struct_name,
                    message_name,
                    action_name,
                    message_variants,
                    update_arms,
                    hash_fields,
                );

                quote! {
                    crate::components::home::overlay::settings::widgets::render_subsection(
                        #title,
                        !self.closed_subsections.contains(#id),
                        #message_name::ToggleSubsection(#id),
                        theme,
                        structure,
                        w::column![ #(#sub_views),* ].spacing(structure.small_gap).into(),
                    )
                }
            }
        })
        .collect()
}

pub fn section(input: TokenStream) -> TokenStream {
    let SectionInput { struct_name, items } = syn::parse_macro_input!(input as SectionInput);

    let message_name = format_ident!("{}Message", struct_name);
    let action_name = format_ident!("{}Action", struct_name);

    let mut message_variants = vec![
        quote! { ToggleSubsection(&'static str) },
        quote! { ToggleCloud(&'static str, bool) },
    ];
    let mut update_arms = vec![quote! {
        #message_name::None => None,
        #message_name::ToggleSubsection(id) => {
            if !self.closed_subsections.remove(id) {
                self.closed_subsections.insert(id);
            }
            None
        }
    }];

    let mut hash_fields = Vec::new();

    let views = walk(
        &items,
        &struct_name,
        &message_name,
        &action_name,
        &mut message_variants,
        &mut update_arms,
        &mut hash_fields,
    );

    let hashings = hash_fields.iter().map(|field| {
        quote! {
            self.settings.#field.value().hash(state);
            self.settings.#field.uses_cloud.as_ref().map(|c| c.borrow().hash(state));
        }
    });

    let cloud_toggle_arms = hash_fields.iter().map(|field| {
        let name = field.to_string();
        quote! {
            #name => self.settings.#field.set_uses_cloud(uses_cloud, &self.settings),
        }
    });

    update_arms.push(quote! {
        #message_name::ToggleCloud(field_name, uses_cloud) => {
            match field_name {
                #(#cloud_toggle_arms)*
                _ => {}
            }
            None
        }
    });

    let expanded = quote! {
        #[derive(Clone, Debug)]
        pub struct #struct_name {
            settings: ::deplace_core::settings::Settings,
            closed_subsections: ::std::collections::BTreeSet<&'static str>,
        }

        impl #struct_name {
            pub fn new(settings: ::deplace_core::settings::Settings) -> Self {
                Self {
                    settings,
                    closed_subsections: ::std::collections::BTreeSet::new(),
                }
            }
        }

        impl ::std::hash::Hash for #struct_name {
            fn hash<H: ::std::hash::Hasher>(&self, state: &mut H) {
                #(#hashings)*
                self.closed_subsections.hash(state);
            }
        }

        #[derive(Clone, Debug, Default)]
        pub enum #message_name {
            #[default]
            None,
            #(#message_variants),*
        }

        impl crate::components::home::overlay::settings::widgets::ToggleCloudExt for #message_name {
            fn toggle_cloud(field_name: &'static str, uses_cloud: bool) -> Self {
                Self::ToggleCloud(field_name, uses_cloud)
            }
        }

        pub enum #action_name {
            Run(::iced::Task<()>),
        }

        impl crate::components::IcedWidget<#message_name, #action_name> for #struct_name {
            fn update(&mut self, message: #message_name) -> Option<#action_name> {
                match message {
                    #(#update_arms),*
                }
            }

            fn view(&self, theme: crate::common::Theme, structure: crate::common::Structure) -> ::iced::Element<'static, #message_name> {
                use ::iced::widget as w;

                w::column![ #(#views),* ]
                    .spacing(structure.gap)
                    .width(::iced::Length::Fill)
                    .into()
            }
        }
    };

    expanded.into()
}
