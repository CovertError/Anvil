//! Proc macros for Spark.
//!
//! - `#[spark_component(template = "spark/counter")]` — on a struct: emits
//!   Serialize/Deserialize/Default derives, the `Component` impl, the
//!   `DynComponent` impl, and the `inventory::submit!` for `ComponentEntry`.
//! - `#[spark_actions]` — on `impl Counter { … }`: scans the impl block for
//!   `pub async fn`s (treated as actions), `#[spark_mount]`, `#[spark_on(…)]`,
//!   `#[spark_updated(…)]`, and emits `DispatchEntry`, `ListenerEntry`,
//!   `MountEntry` submissions plus a generated dispatcher function. The original
//!   impl block is preserved (minus the marker attributes).
//! - `#[spark_mount]`, `#[spark_on("event")]`, `#[spark_updated("field")]` —
//!   marker attributes consumed by `#[spark_actions]`. On their own they
//!   compile to a no-op (so `cargo check` works even if `#[spark_actions]` is
//!   missing — the actions simply won't be registered).

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{
    parse_macro_input, parse_quote, Attribute, FnArg, ImplItem, ImplItemFn, ItemImpl, ItemStruct,
    Lit, LitStr, Meta, MetaNameValue, Pat, Token, Type,
};

// ─── #[spark_component(template = "spark/foo")] ────────────────────────────

struct ComponentArgs {
    template: String,
}

impl Parse for ComponentArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let pairs: Punctuated<MetaNameValue, Token![,]> = Punctuated::parse_terminated(input)?;
        let mut template = None;
        for nv in pairs {
            if nv.path.is_ident("template") {
                if let syn::Expr::Lit(expr_lit) = &nv.value {
                    if let Lit::Str(s) = &expr_lit.lit {
                        template = Some(s.value());
                    }
                }
            }
        }
        Ok(ComponentArgs {
            template: template
                .ok_or_else(|| syn::Error::new(input.span(), "expected `template = \"...\"`"))?,
        })
    }
}

#[proc_macro_attribute]
pub fn component(args: TokenStream, input: TokenStream) -> TokenStream {
    let args = parse_macro_input!(args as ComponentArgs);
    let mut input = parse_macro_input!(input as ItemStruct);

    // Collect field metadata before we strip Spark attributes.
    let mut model_fields: Vec<(syn::Ident, Type)> = Vec::new();
    let mut data_fields: Vec<syn::Ident> = Vec::new();
    let mut skip_fields: Vec<syn::Ident> = Vec::new();

    if let syn::Fields::Named(fields_named) = &mut input.fields {
        for field in &mut fields_named.named {
            let ident = field.ident.clone().expect("named field");
            let ty = field.ty.clone();
            let mut is_skip = false;
            let mut is_model = false;
            // Walk + strip #[spark(...)] attributes.
            field.attrs.retain(|attr| {
                if !attr.path().is_ident("spark") {
                    return true;
                }
                if let Meta::List(list) = &attr.meta {
                    let tokens: TokenStream2 = list.tokens.clone();
                    let str = tokens.to_string();
                    if str.contains("model") {
                        is_model = true;
                    }
                    if str.contains("skip") {
                        is_skip = true;
                    }
                }
                false
            });
            if is_skip {
                skip_fields.push(ident.clone());
                // Ensure serde skips it too.
                field.attrs.push(parse_quote!(#[serde(skip)]));
            } else {
                data_fields.push(ident.clone());
            }
            if is_model {
                model_fields.push((ident.clone(), ty.clone()));
            }
        }
    }

    let struct_ident = input.ident.clone();
    let template_path = args.template.clone();

    // Inject `derive(Serialize, Deserialize, Default, Clone)` if not present.
    let needed_derives = needed_derives(&input.attrs);
    if !needed_derives.is_empty() {
        let combined: TokenStream2 = needed_derives.iter().map(|p| quote!(#p,)).collect();
        input.attrs.push(parse_quote!(#[derive(#combined)]));
    }

    let _ = data_fields;
    let _ = skip_fields;

    let model_arms: Vec<TokenStream2> = model_fields
        .iter()
        .map(|(name, ty)| {
            let name_s = name.to_string();
            quote! {
                #name_s => {
                    self.#name = ::spark::serde_json::from_value::<#ty>(write.value.clone())
                        .map_err(|e| ::spark::Error::InvalidArguments {
                            method: "apply_writes".into(),
                            message: format!("field {}: {}", #name_s, e),
                        })?;
                }
            }
        })
        .collect();

    let class_name_literal = quote! {
        ::std::concat!(::std::module_path!(), "::", ::std::stringify!(#struct_ident))
    };

    let dyn_impl = quote! {
        impl ::spark::registry::DynComponent for #struct_ident {
            fn as_any_mut(&mut self) -> &mut (dyn ::std::any::Any + ::std::marker::Send + ::std::marker::Sync) {
                self
            }

            fn snapshot_data(&self) -> ::spark::serde_json::Value {
                ::spark::serde_json::to_value(self).unwrap_or(::spark::serde_json::Value::Null)
            }

            fn render(&self) -> ::spark::Result<String> {
                <Self as ::spark::Component>::render(self)
            }

            fn apply_writes<'a>(
                &'a mut self,
                writes: &'a [::spark::PropertyWrite],
                ctx: &'a mut ::spark::Ctx,
            ) -> ::std::pin::Pin<Box<dyn ::spark::futures::Future<Output = ::spark::Result<()>> + ::std::marker::Send + 'a>> {
                Box::pin(<Self as ::spark::Component>::apply_writes(self, writes, ctx))
            }

            fn dispatch_call<'a>(
                &'a mut self,
                method: &'a str,
                args: ::std::vec::Vec<::spark::serde_json::Value>,
                ctx: &'a mut ::spark::Ctx,
            ) -> ::std::pin::Pin<Box<dyn ::spark::futures::Future<Output = ::spark::Result<()>> + ::std::marker::Send + 'a>> {
                Box::pin(<Self as ::spark::Component>::dispatch_call(self, method, args, ctx))
            }
        }
    };

    let component_impl = quote! {
        #[::spark::async_trait::async_trait]
        impl ::spark::Component for #struct_ident {
            fn class_name() -> &'static str { #class_name_literal }
            fn view_path() -> &'static str { #template_path }

            fn listeners() -> ::std::vec::Vec<String> {
                ::spark::registry::listeners_for(<Self as ::spark::Component>::class_name())
            }

            fn snapshot_data(&self) -> ::spark::serde_json::Value {
                ::spark::serde_json::to_value(self).unwrap_or(::spark::serde_json::Value::Null)
            }

            fn load_snapshot(data: &::spark::serde_json::Value) -> ::spark::Result<Self> {
                ::spark::serde_json::from_value::<Self>(data.clone())
                    .map_err(|e| ::spark::Error::SnapshotDecode(format!("load_snapshot: {e}")))
            }

            fn mount(props: ::spark::MountProps) -> Self {
                Self::default_then_props(props)
            }

            async fn apply_writes(
                &mut self,
                writes: &[::spark::PropertyWrite],
                _ctx: &mut ::spark::Ctx,
            ) -> ::spark::Result<()> {
                for write in writes {
                    match write.name.as_str() {
                        #(#model_arms)*
                        other => {
                            ::tracing::debug!(
                                field = other,
                                class = <Self as ::spark::Component>::class_name(),
                                "ignored property write for non-#[spark(model)] field"
                            );
                        }
                    }
                }
                Ok(())
            }

            async fn dispatch_call(
                &mut self,
                method: &str,
                args: ::std::vec::Vec<::spark::serde_json::Value>,
                ctx: &mut ::spark::Ctx,
            ) -> ::spark::Result<()> {
                let class = <Self as ::spark::Component>::class_name();
                if let Some(entry) = ::spark::registry::dispatcher_for(class) {
                    let any: &mut (dyn ::std::any::Any + ::std::marker::Send + ::std::marker::Sync) = self;
                    return (entry.dispatch)(any, method, args, ctx).await;
                }
                Err(::spark::Error::UnknownMethod {
                    class: class.into(),
                    method: method.into(),
                })
            }
        }

        impl #struct_ident {
            #[doc(hidden)]
            fn default_then_props(props: ::spark::MountProps) -> Self {
                let class = <Self as ::spark::Component>::class_name();
                if let Some(factory) = ::spark::registry::mount_factory_for(class) {
                    let any_boxed: Box<dyn ::std::any::Any + ::std::marker::Send> = (factory)(props);
                    match any_boxed.downcast::<Self>() {
                        Ok(this) => return *this,
                        Err(_) => return Self::default(),
                    }
                }
                let _ = props;
                Self::default()
            }
        }
    };

    let const_class = quote! {
        ::std::concat!(::std::module_path!(), "::", ::std::stringify!(#struct_ident))
    };
    let const_view = LitStr::new(&template_path, proc_macro2::Span::call_site());

    let component_entry = quote! {
        fn __spark_mount_thunk(props: ::spark::MountProps) -> ::spark::BoxedComponent {
            let inst: #struct_ident = <#struct_ident as ::spark::Component>::mount(props);
            ::spark::BoxedComponent {
                class: #const_class,
                view: #const_view,
                state: Box::new(inst),
            }
        }
        fn __spark_load_thunk(data: &::spark::serde_json::Value) -> ::spark::Result<::spark::BoxedComponent> {
            let inst = <#struct_ident as ::spark::Component>::load_snapshot(data)?;
            Ok(::spark::BoxedComponent {
                class: #const_class,
                view: #const_view,
                state: Box::new(inst),
            })
        }
        fn __spark_listeners_thunk() -> ::std::vec::Vec<String> {
            ::spark::registry::listeners_for(#const_class)
        }
        ::spark::inventory::submit! {
            ::spark::registry::ComponentEntry {
                class: #const_class,
                view: #const_view,
                listeners: __spark_listeners_thunk,
                mount: __spark_mount_thunk,
                load: __spark_load_thunk,
            }
        }
    };

    let expanded = quote! {
        #input

        #dyn_impl

        #component_impl

        const _: () = {
            #component_entry
        };
    };

    expanded.into()
}

fn needed_derives(attrs: &[Attribute]) -> Vec<syn::Path> {
    let mut has_serialize = false;
    let mut has_deserialize = false;
    let mut has_default = false;
    let mut has_clone = false;

    for attr in attrs {
        if attr.path().is_ident("derive") {
            if let Meta::List(list) = &attr.meta {
                let s = list.tokens.to_string();
                if s.contains("Serialize") {
                    has_serialize = true;
                }
                if s.contains("Deserialize") {
                    has_deserialize = true;
                }
                if s.contains("Default") {
                    has_default = true;
                }
                if s.contains("Clone") {
                    has_clone = true;
                }
            }
        }
    }

    let mut needed: Vec<syn::Path> = Vec::new();
    if !has_serialize {
        needed.push(parse_quote!(::spark::serde::Serialize));
    }
    if !has_deserialize {
        needed.push(parse_quote!(::spark::serde::Deserialize));
    }
    if !has_default {
        needed.push(parse_quote!(::std::default::Default));
    }
    if !has_clone {
        needed.push(parse_quote!(::std::clone::Clone));
    }
    needed
}

// ─── #[spark_actions] on impl block ────────────────────────────────────────

#[proc_macro_attribute]
pub fn actions(_args: TokenStream, input: TokenStream) -> TokenStream {
    let mut input = parse_macro_input!(input as ItemImpl);

    let self_ty = match &*input.self_ty {
        Type::Path(p) => p.path.clone(),
        _ => {
            return syn::Error::new_spanned(
                &input.self_ty,
                "#[spark_actions] requires a path self type, e.g. `impl Counter { ... }`",
            )
            .to_compile_error()
            .into();
        }
    };
    let struct_ident = match self_ty.segments.last() {
        Some(seg) => seg.ident.clone(),
        None => {
            return syn::Error::new_spanned(
                &input.self_ty,
                "#[spark_actions] could not extract struct ident",
            )
            .to_compile_error()
            .into();
        }
    };

    // Walk the methods, classifying them.
    let mut action_methods: Vec<ImplItemFn> = Vec::new();
    let mut mount_method: Option<ImplItemFn> = None;
    let mut on_methods: Vec<(String, ImplItemFn)> = Vec::new();
    let mut updated_methods: Vec<(String, ImplItemFn)> = Vec::new();

    for item in input.items.iter_mut() {
        let ImplItem::Fn(method) = item else { continue };

        let mut is_mount = false;
        let mut on_event: Option<String> = None;
        let mut updated_field: Option<String> = None;

        // Strip marker attributes, recording what we saw. We accept either bare
        // attribute names (`#[spark_mount]`) or qualified paths
        // (`#[spark_derive::mount]`, `#[crate::spark_mount]`), keying on the
        // *last* path segment.
        method.attrs.retain(|attr| {
            let last_seg = attr
                .path()
                .segments
                .last()
                .map(|s| s.ident.to_string())
                .unwrap_or_default();
            match last_seg.as_str() {
                "spark_mount" | "mount" => {
                    is_mount = true;
                    false
                }
                "spark_on" | "on" => {
                    if let Ok(s) = attr.parse_args::<LitStr>() {
                        on_event = Some(s.value());
                    }
                    false
                }
                "spark_updated" | "updated" => {
                    if let Ok(s) = attr.parse_args::<LitStr>() {
                        updated_field = Some(s.value());
                    }
                    false
                }
                _ => true,
            }
        });

        if is_mount {
            mount_method = Some(method.clone());
            continue;
        }
        if let Some(event) = on_event.clone() {
            on_methods.push((event, method.clone()));
            // Also expose as a regular action so the on_broadcast handler can route to it.
            action_methods.push(method.clone());
            continue;
        }
        if let Some(field) = updated_field.clone() {
            updated_methods.push((field, method.clone()));
            continue;
        }

        // Anything else that's `async fn name(&mut self, ...)` is an action.
        if method.sig.asyncness.is_some() {
            if let Some(FnArg::Receiver(rec)) = method.sig.inputs.first() {
                if rec.mutability.is_some() {
                    action_methods.push(method.clone());
                }
            }
        }
    }

    // Build the dispatch function body.
    let dispatch_arms: Vec<TokenStream2> = action_methods.iter().map(dispatch_arm_for).collect();

    let on_arms: Vec<TokenStream2> = on_methods
        .iter()
        .map(|(event, m)| {
            let method_ident = &m.sig.ident;
            let event_lit = LitStr::new(event, proc_macro2::Span::call_site());
            quote! {
                #event_lit => {
                    let _ = this.#method_ident().await;
                }
            }
        })
        .collect();

    let on_events: Vec<String> = on_methods.iter().map(|(e, _)| e.clone()).collect();
    let on_events_count = on_events.len();
    let on_event_literals: Vec<LitStr> = on_events
        .iter()
        .map(|e| LitStr::new(e, proc_macro2::Span::call_site()))
        .collect();

    let updated_arms: Vec<TokenStream2> = updated_methods
        .iter()
        .map(|(field, m)| {
            let method_ident = &m.sig.ident;
            let field_lit = LitStr::new(field, proc_macro2::Span::call_site());
            quote! {
                if write_name == #field_lit {
                    let this = any.downcast_mut::<#struct_ident>().expect("downcast");
                    this.#method_ident();
                }
            }
        })
        .collect();

    let _ = updated_arms; // currently unused; planned for v1.1.

    let dispatch_fn_ident = format_ident!("__spark_dispatch_{}", struct_ident);
    let listeners_fn_ident = format_ident!("__spark_listeners_{}", struct_ident);
    let mount_fn_ident = format_ident!("__spark_mount_{}", struct_ident);

    // Class name as a literal — must be evaluable in a `static` initializer.
    let class_path = quote! {
        ::std::concat!(::std::module_path!(), "::", ::std::stringify!(#struct_ident))
    };

    let dispatcher_block = quote! {
        #[allow(non_camel_case_types)]
        type __SparkSelfTy = #self_ty;

        #[allow(non_snake_case)]
        fn #dispatch_fn_ident<'a>(
            any: &'a mut (dyn ::std::any::Any + ::std::marker::Send + ::std::marker::Sync),
            method: &'a str,
            args: ::std::vec::Vec<::spark::serde_json::Value>,
            ctx: &'a mut ::spark::Ctx,
        ) -> ::std::pin::Pin<Box<dyn ::spark::futures::Future<Output = ::spark::Result<()>> + ::std::marker::Send + 'a>>
        {
            Box::pin(async move {
                let _ = ctx;
                let _ = &args;
                if method == "__on_broadcast" {
                    let event_name: String = args.get(0)
                        .and_then(|v| v.as_str())
                        .map(String::from)
                        .unwrap_or_default();
                    let this = any.downcast_mut::<__SparkSelfTy>().expect("downcast");
                    let _ = this;
                    match event_name.as_str() {
                        #(#on_arms)*
                        _ => {}
                    }
                    return Ok(());
                }
                match method {
                    #(#dispatch_arms)*
                    other => {
                        return Err(::spark::Error::UnknownMethod {
                            class: #class_path.into(),
                            method: other.into(),
                        });
                    }
                }
            })
        }

        #[inline]
        fn __spark_into_err(e: ::spark::Error) -> ::spark::Error { e }

        ::spark::inventory::submit! {
            ::spark::registry::DispatchEntry {
                class: #class_path,
                dispatch: #dispatch_fn_ident,
            }
        }
    };

    let listeners_block = if on_events_count > 0 {
        quote! {
            #[allow(non_snake_case)]
            fn #listeners_fn_ident() -> ::std::vec::Vec<String> {
                vec![#(String::from(#on_event_literals)),*]
            }
            ::spark::inventory::submit! {
                ::spark::registry::ListenerEntry {
                    class: #class_path,
                    events: #listeners_fn_ident,
                }
            }
        }
    } else {
        quote! {}
    };

    let mount_block = if let Some(mount_method) = mount_method {
        let method_ident = &mount_method.sig.ident;
        quote! {
            #[allow(non_snake_case)]
            fn #mount_fn_ident(props: ::spark::MountProps) -> Box<dyn ::std::any::Any + ::std::marker::Send> {
                let inst: #self_ty = #self_ty::#method_ident(props);
                Box::new(inst)
            }
            ::spark::inventory::submit! {
                ::spark::registry::MountEntry {
                    class: #class_path,
                    mount: #mount_fn_ident,
                }
            }
        }
    } else {
        quote! {}
    };

    let expanded = quote! {
        #input

        const _: () = {
            #dispatcher_block
            #listeners_block
            #mount_block
        };
    };

    expanded.into()
}

fn dispatch_arm_for(method: &ImplItemFn) -> TokenStream2 {
    let method_ident = &method.sig.ident;
    let method_name = method_ident.to_string();
    let method_name_lit = LitStr::new(&method_name, proc_macro2::Span::call_site());

    // Extract non-receiver, non-Ctx params.
    let mut typed_args: Vec<(syn::Ident, Type)> = Vec::new();
    let mut takes_ctx = false;
    for arg in method.sig.inputs.iter().skip(1) {
        let FnArg::Typed(typed) = arg else { continue };
        let ty = (*typed.ty).clone();
        let Pat::Ident(ident_pat) = &*typed.pat else {
            continue;
        };
        // Detect `ctx: &mut Ctx`.
        if is_ctx_type(&ty) {
            takes_ctx = true;
            continue;
        }
        typed_args.push((ident_pat.ident.clone(), ty));
    }

    let arg_extractions: Vec<TokenStream2> = typed_args
        .iter()
        .enumerate()
        .map(|(i, (name, ty))| {
            let idx = i;
            let name_str = name.to_string();
            quote! {
                let #name: #ty = match args.get(#idx) {
                    Some(v) => ::spark::serde_json::from_value::<#ty>(v.clone())
                        .map_err(|e| ::spark::Error::InvalidArguments {
                            method: #method_name_lit.into(),
                            message: format!("arg {}: {}", #name_str, e),
                        })?,
                    None => return Err(::spark::Error::InvalidArguments {
                        method: #method_name_lit.into(),
                        message: format!("missing arg {}", #name_str),
                    }),
                };
            }
        })
        .collect();

    let arg_names: Vec<TokenStream2> = typed_args.iter().map(|(n, _)| quote!(#n)).collect();

    let call = if takes_ctx {
        quote! { this.#method_ident(#(#arg_names,)* ctx).await }
    } else {
        quote! { this.#method_ident(#(#arg_names),*).await }
    };

    quote! {
        #method_name_lit => {
            let this = any.downcast_mut::<__SparkSelfTy>().expect("downcast");
            #(#arg_extractions)*
            let _ = ctx; // suppress unused
            #call.map_err(__spark_into_err)?;
            return Ok(());
        }
    }
}

fn is_ctx_type(ty: &Type) -> bool {
    let s = quote!(#ty).to_string();
    s.contains("Ctx")
}

// ─── Marker attributes (no-ops on their own) ───────────────────────────────

#[proc_macro_attribute]
pub fn mount(_args: TokenStream, input: TokenStream) -> TokenStream {
    input
}

#[proc_macro_attribute]
pub fn on(_args: TokenStream, input: TokenStream) -> TokenStream {
    input
}

#[proc_macro_attribute]
pub fn updated(_args: TokenStream, input: TokenStream) -> TokenStream {
    input
}
