//! Expansion of `#[derive(Model)]`.

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{Data, DeriveInput, Fields};

use crate::relation::{collect_relations, RelationDecl};

pub fn expand(input: &DeriveInput) -> syn::Result<TokenStream> {
    let struct_name = &input.ident;
    let vis = &input.vis;

    let table_name = extract_table_name(input)?;
    let pk_column = extract_pk_column(input).unwrap_or_else(|| "id".to_string());
    let has_soft_deletes = input
        .attrs
        .iter()
        .any(|a| a.path().is_ident("soft_deletes"));

    let fields = match &input.data {
        Data::Struct(s) => match &s.fields {
            Fields::Named(named) => named.named.iter().collect::<Vec<_>>(),
            _ => {
                return Err(syn::Error::new_spanned(
                    input,
                    "Model derive only supports structs with named fields",
                ));
            }
        },
        _ => {
            return Err(syn::Error::new_spanned(
                input,
                "Model derive only supports structs",
            ));
        }
    };

    let column_names: Vec<String> = fields
        .iter()
        .map(|f| f.ident.as_ref().unwrap().to_string())
        .collect();

    let columns_struct_name = format_ident!("{}Columns", struct_name);

    let column_accessors = fields.iter().map(|f| {
        let ident = f.ident.as_ref().unwrap();
        let col_name = ident.to_string();
        let ty = &f.ty;
        quote! {
            pub fn #ident(&self) -> ::anvilforge::cast::Column<#struct_name, #ty> {
                ::anvilforge::cast::Column::new(#col_name)
            }
        }
    });

    let pk_field_ident = fields
        .iter()
        .find(|f| f.ident.as_ref().unwrap() == &pk_column)
        .map(|f| f.ident.clone().unwrap())
        .ok_or_else(|| {
            syn::Error::new_spanned(
                input,
                format!("primary key field '{pk_column}' not found in struct"),
            )
        })?;

    let pk_field_type = fields
        .iter()
        .find(|f| f.ident.as_ref().unwrap() == &pk_column)
        .map(|f| f.ty.clone())
        .unwrap();

    let columns_array = column_names.iter().map(|n| quote!(#n)).collect::<Vec<_>>();

    let relations = collect_relations(input)?;
    let relation_methods = relations
        .iter()
        .map(|r| expand_relation(struct_name, &pk_field_ident, r));
    let relation_types = relations.iter().map(|r| relation_type_decl(struct_name, r));

    let table_lit = syn::LitStr::new(&table_name, struct_name.span());
    let pk_lit = syn::LitStr::new(&pk_column, struct_name.span());

    let from_row_fields = fields.iter().map(|f| {
        let ident = f.ident.as_ref().unwrap();
        let col_name = ident.to_string();
        quote! {
            #ident: row.try_get(#col_name)?,
        }
    });

    // ── Write API: emit Eloquent-style INSERT/UPDATE/DELETE methods ──────────
    //
    // Fields automatically excluded from INSERT/UPDATE:
    //   - the primary key (filled in by RETURNING / not present on insert)
    //   - `created_at` / `updated_at` / `deleted_at` (DB defaults handle them)
    //
    // The result: `user.save(&pool).await?` Just Works for the canonical model
    // shape: id + scalar fields + timestamps.
    let writable_field_idents: Vec<&syn::Ident> = fields
        .iter()
        .filter_map(|f| {
            let ident = f.ident.as_ref().unwrap();
            let name = ident.to_string();
            if name == pk_column
                || name == "created_at"
                || name == "updated_at"
                || name == "deleted_at"
            {
                None
            } else {
                Some(ident)
            }
        })
        .collect();
    let writable_field_names: Vec<String> = writable_field_idents
        .iter()
        .map(|i| i.to_string())
        .collect();

    let insert_columns_csv = writable_field_names.join(", ");
    let insert_placeholders_csv = (1..=writable_field_names.len())
        .map(|i| format!("${i}"))
        .collect::<Vec<_>>()
        .join(", ");
    let update_set_csv = writable_field_names
        .iter()
        .enumerate()
        .map(|(i, name)| format!("{name} = ${}", i + 1))
        .collect::<Vec<_>>()
        .join(", ");
    let update_pk_placeholder = writable_field_names.len() + 1;

    let insert_sql = format!(
        "INSERT INTO {table_name} ({insert_columns_csv}) VALUES ({insert_placeholders_csv}) RETURNING {pk_column}"
    );

    // upsert_set_excluded_csv: `col1 = EXCLUDED.col1, col2 = EXCLUDED.col2, ...`
    // for every writable column. Used in `ON CONFLICT ... DO UPDATE SET ...`.
    let upsert_set_excluded_csv = writable_field_names
        .iter()
        .map(|n| format!("{n} = EXCLUDED.{n}"))
        .collect::<Vec<_>>()
        .join(", ");
    let upsert_set_excluded_csv_lit =
        syn::LitStr::new(&upsert_set_excluded_csv, struct_name.span());
    let insert_columns_csv_lit = syn::LitStr::new(&insert_columns_csv, struct_name.span());
    let insert_placeholders_csv_lit =
        syn::LitStr::new(&insert_placeholders_csv, struct_name.span());
    let table_name_lit_str = syn::LitStr::new(&table_name, struct_name.span());
    let pk_column_lit_str = syn::LitStr::new(&pk_column, struct_name.span());
    let update_sql = format!(
        "UPDATE {table_name} SET {update_set_csv}, updated_at = CURRENT_TIMESTAMP WHERE {pk_column} = ${update_pk_placeholder}"
    );
    let delete_sql = format!("DELETE FROM {table_name} WHERE {pk_column} = $1");

    let insert_sql_lit = syn::LitStr::new(&insert_sql, struct_name.span());
    let update_sql_lit = syn::LitStr::new(&update_sql, struct_name.span());
    let delete_sql_lit = syn::LitStr::new(&delete_sql, struct_name.span());

    let bind_inserts = writable_field_idents.iter().map(|i| {
        quote! { let q = q.bind(&self.#i); }
    });
    let bind_updates = writable_field_idents.iter().map(|i| {
        quote! { let q = q.bind(&self.#i); }
    });

    let soft_deletes_lit = if has_soft_deletes {
        quote!(true)
    } else {
        quote!(false)
    };

    // Soft-delete variants of save/delete. `delete()` becomes UPDATE SET deleted_at = NOW().
    let soft_delete_sql =
        format!("UPDATE {table_name} SET deleted_at = CURRENT_TIMESTAMP WHERE {pk_column} = $1");
    let force_delete_sql = format!("DELETE FROM {table_name} WHERE {pk_column} = $1");
    let restore_sql = format!("UPDATE {table_name} SET deleted_at = NULL WHERE {pk_column} = $1");

    let soft_delete_sql_lit = syn::LitStr::new(&soft_delete_sql, struct_name.span());
    let force_delete_sql_lit = syn::LitStr::new(&force_delete_sql, struct_name.span());
    let restore_sql_lit = syn::LitStr::new(&restore_sql, struct_name.span());

    // Override the default delete SQL when soft deletes are enabled.
    let delete_method_body = if has_soft_deletes {
        quote! {
            ::anvilforge::cast::sqlx::query(#soft_delete_sql_lit)
                .bind(&self.#pk_field_ident)
                .execute(pool)
                .await?;
            Ok(())
        }
    } else {
        quote! {
            ::anvilforge::cast::sqlx::query(#delete_sql_lit)
                .bind(&self.#pk_field_ident)
                .execute(pool)
                .await?;
            Ok(())
        }
    };

    let output = quote! {
        impl ::anvilforge::cast::Model for #struct_name {
            type PrimaryKey = #pk_field_type;
            const SOFT_DELETES: bool = #soft_deletes_lit;
            const TABLE: &'static str = #table_lit;
            const PK_COLUMN: &'static str = #pk_lit;
            const COLUMNS: &'static [&'static str] = &[#(#columns_array),*];

            fn primary_key(&self) -> &Self::PrimaryKey {
                &self.#pk_field_ident
            }
        }

        // Register this model so `boost` / `anvil mcp` can list it.
        ::anvilforge::cast::inventory::submit! {
            ::anvilforge::cast::ModelRegistration {
                class: ::std::concat!(::std::module_path!(), "::", ::std::stringify!(#struct_name)),
                table: #table_lit,
                columns: &[#(#columns_array),*],
            }
        }

        #[doc(hidden)]
        #vis struct #columns_struct_name;

        impl #columns_struct_name {
            #(#column_accessors)*
        }

        impl #struct_name {
            pub fn columns() -> #columns_struct_name {
                #columns_struct_name
            }

            // ── Eloquent-style write API ─────────────────────────────────────

            /// Insert a new row. Returns `Self` with the primary key populated
            /// from `RETURNING id`. Mirrors Laravel's `User::create([...])` /
            /// `User::query()->insert([...])` semantics.
            ///
            /// Fields excluded from the INSERT: the primary key, `created_at`,
            /// `updated_at`, `deleted_at` — these are handled by the database default.
            pub async fn insert(
                self,
                pool: &::anvilforge::cast::sqlx::PgPool,
            ) -> ::anvilforge::cast::Result<Self> {
                let q = ::anvilforge::cast::sqlx::query_as::<_, (#pk_field_type,)>(#insert_sql_lit);
                #(#bind_inserts)*
                let row = q.fetch_one(pool).await?;
                Ok(Self { #pk_field_ident: row.0, ..self })
            }

            /// Eloquent-style alias for `.insert(pool)` — same behaviour, the
            /// name a Laravel developer reaches for. `Post::create(&pool, post)`
            /// is interchangeable with `post.insert(&pool)`.
            pub async fn create(
                pool: &::anvilforge::cast::sqlx::PgPool,
                attrs: Self,
            ) -> ::anvilforge::cast::Result<Self> {
                attrs.insert(pool).await
            }

            /// Update an existing row by primary key. Returns the updated model.
            /// Sets `updated_at = CURRENT_TIMESTAMP` automatically.
            ///
            /// Use when you've mutated fields on `self` and want to persist them:
            /// ```ignore
            /// user.name = "Renamed".into();
            /// let user = user.update(&pool).await?;
            /// ```
            pub async fn update(
                self,
                pool: &::anvilforge::cast::sqlx::PgPool,
            ) -> ::anvilforge::cast::Result<Self> {
                let q = ::anvilforge::cast::sqlx::query(#update_sql_lit);
                #(#bind_updates)*
                let q = q.bind(&self.#pk_field_ident);
                q.execute(pool).await?;
                Ok(self)
            }

            /// Save: insert if the primary key is `default()` (e.g. `0` for `i64`),
            /// otherwise update. Mirrors Eloquent's `$model->save()`.
            pub async fn save(
                self,
                pool: &::anvilforge::cast::sqlx::PgPool,
            ) -> ::anvilforge::cast::Result<Self>
            where
                #pk_field_type: ::core::default::Default + ::core::cmp::PartialEq,
            {
                if self.#pk_field_ident == <#pk_field_type as ::core::default::Default>::default() {
                    self.insert(pool).await
                } else {
                    self.update(pool).await
                }
            }

            /// Delete by primary key. For models with `#[soft_deletes]`, this is
            /// a soft delete (UPDATE deleted_at = NOW()). Otherwise it's a hard DELETE.
            /// Mirrors Eloquent's `$model->delete()`.
            pub async fn delete(
                self,
                pool: &::anvilforge::cast::sqlx::PgPool,
            ) -> ::anvilforge::cast::Result<()> {
                #delete_method_body
            }

            /// Hard delete — bypasses soft-delete tombstoning. Mirrors Eloquent's
            /// `$model->forceDelete()`.
            pub async fn force_delete(
                self,
                pool: &::anvilforge::cast::sqlx::PgPool,
            ) -> ::anvilforge::cast::Result<()> {
                ::anvilforge::cast::sqlx::query(#force_delete_sql_lit)
                    .bind(&self.#pk_field_ident)
                    .execute(pool)
                    .await?;
                Ok(())
            }

            /// Restore a soft-deleted model — sets `deleted_at = NULL`. Mirrors
            /// Eloquent's `$model->restore()`. Only meaningful for models with
            /// `#[soft_deletes]`.
            pub async fn restore(
                self,
                pool: &::anvilforge::cast::sqlx::PgPool,
            ) -> ::anvilforge::cast::Result<Self>
            where
                <Self as ::anvilforge::cast::Model>::PrimaryKey: ::core::clone::Clone,
            {
                ::anvilforge::cast::sqlx::query(#restore_sql_lit)
                    .bind(&self.#pk_field_ident)
                    .execute(pool)
                    .await?;
                // Return the freshly-loaded row so timestamps reflect reality.
                use ::anvilforge::cast::Model as _;
                let pk = ::core::clone::Clone::clone(&self.#pk_field_ident);
                <Self as ::anvilforge::cast::Model>::find(pool, pk)
                    .await?
                    .ok_or(::anvilforge::cast::Error::NotFound)
            }

            /// Clone the row with the primary key reset to its default value.
            /// Mirrors Eloquent's `$model->replicate()`.
            pub fn replicate(&self) -> Self
            where
                Self: ::core::clone::Clone,
                <Self as ::anvilforge::cast::Model>::PrimaryKey: ::core::default::Default,
            {
                let mut clone = ::core::clone::Clone::clone(self);
                clone.#pk_field_ident = <#pk_field_type as ::core::default::Default>::default();
                clone
            }

            /// Find a row matching the search predicate, or insert `default` if none exists.
            /// Mirrors Eloquent's `Model::firstOrCreate([...], [...])`.
            ///
            /// ```ignore
            /// let user = User::first_or_create(
            ///     pool,
            ///     |q| q.where_eq(User::columns().email(), "ada@x.com".to_string()),
            ///     User { id: 0, name: "Ada".into(), email: "ada@x.com".into(), ..Default::default() },
            /// ).await?;
            /// ```
            pub async fn first_or_create<F>(
                pool: &::anvilforge::cast::sqlx::PgPool,
                search: F,
                default: Self,
            ) -> ::anvilforge::cast::Result<Self>
            where
                F: FnOnce(::anvilforge::cast::QueryBuilder<Self>) -> ::anvilforge::cast::QueryBuilder<Self>,
            {
                use ::anvilforge::cast::Model as _;
                let found = search(Self::query()).first(pool).await?;
                match found {
                    Some(m) => Ok(m),
                    None => default.insert(pool).await,
                }
            }

            /// Find a row matching the search predicate and update it with `attrs`,
            /// or insert `attrs` if no match exists. Mirrors Eloquent's `Model::updateOrCreate`.
            ///
            /// On match, the existing row's primary key is preserved and the rest of
            /// the columns are replaced from `attrs`.
            pub async fn update_or_create<F>(
                pool: &::anvilforge::cast::sqlx::PgPool,
                search: F,
                attrs: Self,
            ) -> ::anvilforge::cast::Result<Self>
            where
                F: FnOnce(::anvilforge::cast::QueryBuilder<Self>) -> ::anvilforge::cast::QueryBuilder<Self>,
                <Self as ::anvilforge::cast::Model>::PrimaryKey: ::core::clone::Clone,
            {
                use ::anvilforge::cast::Model as _;
                let found = search(Self::query()).first(pool).await?;
                match found {
                    Some(existing) => {
                        let mut merged = attrs;
                        merged.#pk_field_ident = ::core::clone::Clone::clone(&existing.#pk_field_ident);
                        merged.update(pool).await
                    }
                    None => attrs.insert(pool).await,
                }
            }

            /// Postgres `ON CONFLICT ... DO UPDATE SET ...` upsert. Inserts
            /// `attrs`, or — when a row with the supplied unique columns
            /// already exists — atomically updates every non-PK column to
            /// the new values (`EXCLUDED.col`). Returns the row's primary
            /// key with the model populated.
            ///
            /// `conflict_cols` is the unique-or-PK constraint to target.
            /// Empty slice means "primary key" — equivalent to `ON CONFLICT
            /// (id) DO UPDATE`. For composite uniques (e.g. `(email, tenant_id)`)
            /// pass `&["email", "tenant_id"]`.
            ///
            /// ```ignore
            /// // Upsert by email (the natural identity for users):
            /// let user = User::upsert(
            ///     pool,
            ///     User { id: 0, email: "a@b.com".into(), name: "Alice".into(), ..Default::default() },
            ///     &["email"],
            /// ).await?;
            /// ```
            pub async fn upsert(
                pool: &::anvilforge::cast::sqlx::PgPool,
                attrs: Self,
                conflict_cols: &[&str],
            ) -> ::anvilforge::cast::Result<Self> {
                let conflict_target = if conflict_cols.is_empty() {
                    #pk_column_lit_str.to_string()
                } else {
                    conflict_cols.join(", ")
                };
                let sql = ::std::format!(
                    "INSERT INTO {} ({}) VALUES ({}) \
                     ON CONFLICT ({}) DO UPDATE SET {} \
                     RETURNING {}",
                    #table_name_lit_str,
                    #insert_columns_csv_lit,
                    #insert_placeholders_csv_lit,
                    conflict_target,
                    #upsert_set_excluded_csv_lit,
                    #pk_column_lit_str,
                );
                let q = ::anvilforge::cast::sqlx::query_as::<_, (#pk_field_type,)>(&sql);
                let attrs_ref = &attrs;
                let q = { let q = q; #(let q = q.bind(&attrs_ref.#writable_field_idents);)* q };
                let row = q.fetch_one(pool).await?;
                Ok(Self { #pk_field_ident: row.0, ..attrs })
            }

            /// Eloquent's `Model::find_or_fail`: like `find` but returns
            /// `Error::NotFound` instead of `Ok(None)`.
            pub async fn find_or_fail(
                pool: &::anvilforge::cast::sqlx::PgPool,
                id: <Self as ::anvilforge::cast::Model>::PrimaryKey,
            ) -> ::anvilforge::cast::Result<Self> {
                <Self as ::anvilforge::cast::Model>::find(pool, id)
                    .await?
                    .ok_or(::anvilforge::cast::Error::NotFound)
            }

            /// Eloquent's `Model::findMany([1, 2, 3])`. Returns models whose
            /// PK is in the supplied list.
            pub async fn find_many<I>(
                pool: &::anvilforge::cast::sqlx::PgPool,
                ids: I,
            ) -> ::anvilforge::cast::Result<::std::vec::Vec<Self>>
            where
                I: ::std::iter::IntoIterator<Item = <Self as ::anvilforge::cast::Model>::PrimaryKey>,
                <Self as ::anvilforge::cast::Model>::PrimaryKey:
                    ::core::convert::Into<::anvilforge::cast::sea_query::Value>,
            {
                use ::anvilforge::cast::Model as _;
                let ids: ::std::vec::Vec<_> = ids.into_iter().collect();
                if ids.is_empty() {
                    return Ok(::std::vec::Vec::new());
                }
                let col = ::anvilforge::cast::Column::<Self, <Self as ::anvilforge::cast::Model>::PrimaryKey>::new(
                    <Self as ::anvilforge::cast::Model>::PK_COLUMN,
                );
                Self::query().where_in(col, ids).get(pool).await
            }

            /// Eloquent's `Model::destroy([id1, id2, ...])`. Returns the row count.
            pub async fn destroy<I>(
                pool: &::anvilforge::cast::sqlx::PgPool,
                ids: I,
            ) -> ::anvilforge::cast::Result<u64>
            where
                I: ::std::iter::IntoIterator<Item = <Self as ::anvilforge::cast::Model>::PrimaryKey>,
                <Self as ::anvilforge::cast::Model>::PrimaryKey:
                    ::core::convert::Into<::anvilforge::cast::sea_query::Value>,
            {
                let ids: ::std::vec::Vec<_> = ids.into_iter().collect();
                if ids.is_empty() {
                    return Ok(0);
                }
                // Build a parameterized `WHERE id IN (...)` using sea-query.
                let stmt = ::anvilforge::cast::sea_query::Query::delete()
                    .from_table(::anvilforge::cast::sea_query::Alias::new(
                        <Self as ::anvilforge::cast::Model>::TABLE,
                    ))
                    .and_where(
                        ::anvilforge::cast::sea_query::Expr::col(
                            ::anvilforge::cast::sea_query::Alias::new(
                                <Self as ::anvilforge::cast::Model>::PK_COLUMN,
                            ),
                        )
                        .is_in(ids),
                    )
                    .to_owned();
                use ::anvilforge::cast::sea_query_binder::SqlxBinder as _;
                let (sql, values) = stmt.build_sqlx(
                    ::anvilforge::cast::sea_query::PostgresQueryBuilder,
                );
                let result = ::anvilforge::cast::sqlx::query_with(&sql, values)
                    .execute(pool)
                    .await?;
                Ok(result.rows_affected())
            }

            /// Eloquent's `Model::truncate()`. Hard-deletes every row in the table.
            pub async fn truncate(
                pool: &::anvilforge::cast::sqlx::PgPool,
            ) -> ::anvilforge::cast::Result<()> {
                let sql = format!(
                    "TRUNCATE TABLE {} RESTART IDENTITY CASCADE",
                    <Self as ::anvilforge::cast::Model>::TABLE,
                );
                ::anvilforge::cast::sqlx::query(&sql).execute(pool).await?;
                Ok(())
            }

            /// Reload `self` from the database in place. Mirrors Eloquent's
            /// `$model->refresh()`. Returns `Error::NotFound` if the row has been deleted.
            pub async fn refresh(
                &mut self,
                pool: &::anvilforge::cast::sqlx::PgPool,
            ) -> ::anvilforge::cast::Result<()>
            where
                <Self as ::anvilforge::cast::Model>::PrimaryKey: ::core::clone::Clone,
            {
                use ::anvilforge::cast::Model as _;
                let pk = ::core::clone::Clone::clone(self.primary_key());
                *self = <Self as ::anvilforge::cast::Model>::find(pool, pk)
                    .await?
                    .ok_or(::anvilforge::cast::Error::NotFound)?;
                Ok(())
            }

            /// Like `refresh` but returns a fresh copy without mutating `self`.
            /// Mirrors Eloquent's `$model->fresh()`.
            pub async fn fresh(
                &self,
                pool: &::anvilforge::cast::sqlx::PgPool,
            ) -> ::anvilforge::cast::Result<::core::option::Option<Self>>
            where
                <Self as ::anvilforge::cast::Model>::PrimaryKey: ::core::clone::Clone,
            {
                use ::anvilforge::cast::Model as _;
                let pk = ::core::clone::Clone::clone(self.primary_key());
                <Self as ::anvilforge::cast::Model>::find(pool, pk).await
            }

            #(#relation_methods)*
        }

        #(#relation_types)*

        impl<'r> ::anvilforge::cast::sqlx::FromRow<'r, ::anvilforge::cast::sqlx::postgres::PgRow> for #struct_name {
            fn from_row(row: &'r ::anvilforge::cast::sqlx::postgres::PgRow) -> ::anvilforge::cast::sqlx::Result<Self> {
                use ::anvilforge::cast::sqlx::Row as _;
                Ok(Self {
                    #(#from_row_fields)*
                })
            }
        }
    };

    Ok(output)
}

fn extract_table_name(input: &DeriveInput) -> syn::Result<String> {
    for attr in &input.attrs {
        if attr.path().is_ident("table") {
            if let Ok(lit) = attr.parse_args::<syn::LitStr>() {
                return Ok(lit.value());
            }
        }
    }
    let struct_name = input.ident.to_string();
    Ok(pluralize_snake_case(&struct_name))
}

fn extract_pk_column(input: &DeriveInput) -> Option<String> {
    for attr in &input.attrs {
        if attr.path().is_ident("primary_key") {
            if let Ok(lit) = attr.parse_args::<syn::LitStr>() {
                return Some(lit.value());
            }
        }
    }
    None
}

fn pluralize_snake_case(s: &str) -> String {
    let mut snake = String::new();
    for (i, ch) in s.chars().enumerate() {
        if ch.is_uppercase() && i > 0 {
            snake.push('_');
        }
        snake.push(ch.to_ascii_lowercase());
    }
    if snake.ends_with('s') {
        snake
    } else if snake.ends_with('y') {
        snake.pop();
        snake.push_str("ies");
        snake
    } else {
        snake.push('s');
        snake
    }
}

fn relation_type_decl(parent: &syn::Ident, rel: &RelationDecl) -> TokenStream {
    let rel_type_name = format_ident!("{}{}Rel", parent, capitalize(&rel.method_name));
    let parent_ident = parent.clone();
    let child = &rel.target;
    let local_key = &rel.local_key;
    let foreign_key = &rel.foreign_key;
    let kind = &rel.kind_token;

    quote! {
        #[doc(hidden)]
        pub struct #rel_type_name;

        impl ::anvilforge::cast::RelationDef for #rel_type_name {
            type Parent = #parent_ident;
            type Child = #child;
            type Kind = ::anvilforge::cast::#kind;
            fn local_key() -> &'static str { #local_key }
            fn foreign_key() -> &'static str { #foreign_key }
        }
    }
}

fn expand_relation(_parent: &syn::Ident, pk_field: &syn::Ident, rel: &RelationDecl) -> TokenStream {
    let method = format_ident!("{}", rel.method_name);
    let rel_method = format_ident!("{}_rel", rel.method_name);
    let rel_type_name = format_ident!("{}{}Rel", _parent, capitalize(&rel.method_name));
    let child = &rel.target;
    let foreign_key = &rel.foreign_key;
    let foreign_key_field = syn::Ident::new(foreign_key, proc_macro2::Span::call_site());

    match rel.kind.as_str() {
        "HasMany" | "HasOne" => {
            // For has_many / has_one: parent's PK value is the local value;
            // we filter the child table by `foreign_key = self.pk_field`.
            let load_method = if rel.kind == "HasMany" {
                quote! { pub async fn #method(&self, pool: &::anvilforge::cast::sqlx::PgPool) -> ::anvilforge::cast::Result<Vec<#child>> {
                    use ::anvilforge::cast::Model as _;
                    #child::query()
                        .where_eq(#child::columns().#foreign_key_field(), self.#pk_field.clone())
                        .get(pool).await
                }}
            } else {
                quote! { pub async fn #method(&self, pool: &::anvilforge::cast::sqlx::PgPool) -> ::anvilforge::cast::Result<Option<#child>> {
                    use ::anvilforge::cast::Model as _;
                    #child::query()
                        .where_eq(#child::columns().#foreign_key_field(), self.#pk_field.clone())
                        .first(pool).await
                }}
            };

            quote! {
                pub fn #rel_method() -> #rel_type_name {
                    #rel_type_name
                }

                #load_method
            }
        }
        "BelongsTo" => {
            // For belongs_to: this struct holds a FK column that points at the child's PK.
            // `foreign_key` here names the local FK field.
            quote! {
                pub fn #rel_method() -> #rel_type_name {
                    #rel_type_name
                }

                pub async fn #method(&self, pool: &::anvilforge::cast::sqlx::PgPool) -> ::anvilforge::cast::Result<Option<#child>> {
                    use ::anvilforge::cast::Model as _;
                    #child::find(pool, self.#foreign_key_field.clone()).await
                }
            }
        }
        _ => quote! {},
    }
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}
