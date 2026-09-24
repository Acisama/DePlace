use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{Fields, ItemStruct, Path, Token, punctuated::Punctuated};

fn type_string(ty: &syn::Type) -> String {
    quote!(#ty).to_string().replace(' ', "")
}

fn assert_role(
    struct_name: &syn::Ident,
    field_name: &syn::Ident,
    field_ty: &syn::Type,
    trait_path: proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    let assert_fn = format_ident!("__assert_{}_{}_role", struct_name, field_name);
    quote! {
        #[allow(non_snake_case)]
        fn #assert_fn() {
            fn assert_impl<T: #trait_path>() {}
            assert_impl::<#field_ty>();
        }
    }
}

pub fn convert_iced(item: ItemStruct, derives: Punctuated<Path, Token![,]>) -> TokenStream {
    let struct_name = item.ident.clone();

    let mut assertions = Vec::new();

    let Fields::Named(fields) = &item.fields else {
        panic!("Only applicable to structs with named fields");
    };

    let mut item_fields = Vec::new();
    let mut hashings = Vec::new();

    let mut hashed_presence_version = false;
    let mut hashed_room_watchers = false;

    for field in &fields.named {
        let field_name = field.ident.as_ref().unwrap().clone();
        let field_ty = field.ty.clone();
        let field_vis = field.vis.clone();

        item_fields.push(quote! {
            #field_vis #field_name: #field_ty,
        });

        let mut ignore = false;
        for attr in &field.attrs {
            if !attr.path().is_ident("hash") && !attr.path().is_ident("ignore") {
                continue;
            }

            if attr.path().is_ident("ignore") {
                ignore = true;
                continue;
            }

            match &attr.meta {
                // bare `#[hash]`
                syn::Meta::Path(_) => hashings.push(quote! { self.#field_name.hash(state); }),
                _ => panic!("`#[hash]` doesn't take arguments"),
            };
        }

        if ignore {
            continue;
        }

        if field_name.to_string().as_str() == "active_room"
            || type_string(&field.ty).as_str() == "Receiver<Option<DeplaceRoom>>"
        {
            assertions.push(assert_role(
                &struct_name,
                &field_name,
                &field_ty,
                quote! { ::deplace_core::state::roles::IsActiveRoom },
            ));
            hashings.push(quote! {
                self.active_room.borrow().as_ref().map(|r| r.room_id()).hash(state);
            });
        }

        if field_name.to_string().as_str() == "active_server"
            || type_string(&field.ty).as_str() == "Receiver<ActiveServer>"
        {
            assertions.push(assert_role(
                &struct_name,
                &field_name,
                &field_ty,
                quote! { ::deplace_core::state::roles::IsActiveServer },
            ));
            hashings.push(quote! {
                self.active_server.borrow().hash(state);
            });
        }

        if field_name.to_string().as_str() == "avatar_cache"
            || type_string(&field.ty).as_str() == "AvatarCache"
        {
            assertions.push(assert_role(
                &struct_name,
                &field_name,
                &field_ty,
                quote! { ::deplace_core::state::roles::IsAvatarCache },
            ));
            item_fields.push(quote! {
                avatar_states_for_hash: std::collections::BTreeSet<OwnedMxcUri>,
            });
            hashings.push(quote! {
                for uri in &self.avatar_states_for_hash {
                    if let Some(res) = self.avatar_cache.get(uri) {
                        res.hash(state)
                    }
                }
            });
        }

        if field_name.to_string().as_str() == "thumbnail_cache"
            || type_string(&field.ty).as_str() == "ThumbnailCache"
        {
            assertions.push(assert_role(
                &struct_name,
                &field_name,
                &field_ty,
                quote! { ::deplace_core::state::roles::IsThumbnailCache },
            ));
            item_fields.push(quote! {
                thumbnail_states_for_hash: std::collections::BTreeSet<(String, u64, u64)>,
            });
            hashings.push(quote! {
                for key in &self.thumbnail_states_for_hash {
                    self.thumbnail_cache
                        .get(key)
                        .unwrap_or_default()
                        .hash(state);
                }
            });
        }

        if field_name.to_string().as_str() == "video_cache"
            || type_string(&field.ty).as_str() == "VideoCache"
        {
            assertions.push(assert_role(
                &struct_name,
                &field_name,
                &field_ty,
                quote! { ::deplace_core::state::roles::IsVideoCache },
            ));
            item_fields.push(quote! {
                video_states_for_hash: std::collections::BTreeSet<String>,
            });
            hashings.push(quote! {
                for key in &self.video_states_for_hash {
                    self.video_cache
                        .get(key)
                        .unwrap_or_default()
                        .hash(state);
                }
            });
        }

        if field_name.to_string().as_str() == "membership_map"
            || type_string(&field.ty).as_str() == "Receiver<MembershipMap>"
        {
            assertions.push(assert_role(
                &struct_name,
                &field_name,
                &field_ty,
                quote! { ::deplace_core::state::roles::IsMembershipMap },
            ));
            hashings.push(quote! {
                self.state.membership_version().hash(state);
            });
        }

        if !hashed_presence_version
            && (field_name.to_string().as_str() == "presence_map"
                || type_string(&field.ty).as_str() == "Receiver<PresenceMap>")
        {
            hashed_presence_version = true;
            assertions.push(assert_role(
                &struct_name,
                &field_name,
                &field_ty,
                quote! { ::deplace_core::state::roles::IsPresenceMap },
            ));
            hashings.push(quote! {
                self.state.presence_version().hash(state);
            });
        }

        if !hashed_room_watchers
            && (field_name.to_string().as_str() == "room_watchers"
                || type_string(&field.ty).as_str() == "RoomWatchers")
        {
            hashed_room_watchers = true;
            assertions.push(assert_role(
                &struct_name,
                &field_name,
                &field_ty,
                quote! { ::deplace_core::state::roles::IsRoomWatchers },
            ));
            hashings.push(quote! {
                self.room_watchers.hash(state);
            });
        }
    }

    // Autoref specialization: fold in `self.extra_hash(state)` if `#struct_name`
    // implements `ExtraHash`, and no-op otherwise. The struct/traits are scoped
    // to this fn body so multiple `#[iced_cache]` structs in one file don't clash.
    hashings.push(quote! {
        {
            struct ExtraHashWrap<'a, T>(&'a T);

            trait ViaNoExtraHash {
                fn maybe_extra_hash<H: std::hash::Hasher>(&self, _state: &mut H) {}
            }
            impl<'a, T> ViaNoExtraHash for ExtraHashWrap<'a, T> {}

            trait ViaExtraHash {
                fn maybe_extra_hash<H: std::hash::Hasher>(&self, state: &mut H);
            }
            impl<'a, T: ::deplace_core::state::roles::ExtraHash> ViaExtraHash for &ExtraHashWrap<'a, T> {
                fn maybe_extra_hash<H: std::hash::Hasher>(&self, state: &mut H) {
                    ::deplace_core::state::roles::ExtraHash::extra_hash(self.0, state);
                }
            }

            (&&ExtraHashWrap(self)).maybe_extra_hash(state);
        }
    });

    let derive_attr = if derives.is_empty() {
        quote! {}
    } else {
        quote! { #[derive(#derives)] }
    };

    let expanded = quote! {
        #derive_attr
        pub struct #struct_name {
            #(#item_fields)*
        }

        #(#assertions)*

        impl std::hash::Hash for #struct_name {
            fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
                #(#hashings)*
            }
        }
    };

    expanded.into()
}
