//! Query builder. Typed where clauses on top of sea-query + sqlx.
//!
//! Mirrors Eloquent's full query-builder surface: `where`/`orWhere` family,
//! `whereIn`/`whereNull`/`whereBetween`/`whereLike`, aggregates
//! (`sum`/`avg`/`min`/`max`/`exists`), sorting (`latest`/`oldest`/`inRandomOrder`),
//! terminals (`pluck`/`value`/`firstOrFail`), joins, group-by/having, and
//! soft-delete scopes (`withTrashed`/`onlyTrashed`/`withoutTrashed`).
//!
//! WHERE conditions are tracked as a single `Option<SimpleExpr>` and folded
//! left-to-right with AND/OR junctions — so `where_eq.where_eq.or_where_eq`
//! becomes `((a AND b) OR c)`. To group explicitly, build the SimpleExpr by hand
//! and pass to `where_raw`.

use std::marker::PhantomData;

use sea_query::{Expr, Order, PostgresQueryBuilder, Query, SimpleExpr};
use sea_query_binder::SqlxBinder;
use sqlx::{postgres::PgRow, FromRow};

use crate::column::Column;
use crate::model::Model;
use crate::Error;

#[derive(Clone, Copy, PartialEq, Eq)]
enum SoftDeleteMode {
    /// Apply the deleted_at IS NULL filter if the model derives `#[soft_deletes]`.
    Default,
    /// `.with_trashed()` — include soft-deleted rows.
    WithTrashed,
    /// `.only_trashed()` — only soft-deleted rows.
    OnlyTrashed,
    /// `.without_trashed()` — explicitly exclude soft-deleted rows.
    WithoutTrashed,
}

pub struct QueryBuilder<M: Model> {
    select: sea_query::SelectStatement,
    where_clause: Option<SimpleExpr>,
    soft_delete_mode: SoftDeleteMode,
    _marker: PhantomData<M>,
}

impl<M: Model> Default for QueryBuilder<M> {
    fn default() -> Self {
        Self::new()
    }
}

impl<M: Model> Clone for QueryBuilder<M> {
    fn clone(&self) -> Self {
        Self {
            select: self.select.clone(),
            where_clause: self.where_clause.clone(),
            soft_delete_mode: self.soft_delete_mode,
            _marker: PhantomData,
        }
    }
}

impl<M: Model> QueryBuilder<M>
where
    for<'r> M: FromRow<'r, PgRow>,
{
    pub fn new() -> Self {
        let mut select = Query::select();
        select.from(sea_query::Alias::new(M::TABLE));
        // Fully-qualify every column with the table name so joins disambiguate
        // shared column names (`id`, `created_at`, etc.).
        for c in M::COLUMNS {
            select.column((sea_query::Alias::new(M::TABLE), sea_query::Alias::new(*c)));
        }
        Self {
            select,
            where_clause: None,
            soft_delete_mode: SoftDeleteMode::Default,
            _marker: PhantomData,
        }
    }

    /// Build a fully-qualified `table.column` expression so joins disambiguate.
    fn col_of(name: &str) -> sea_query::Expr {
        sea_query::Expr::col((sea_query::Alias::new(M::TABLE), sea_query::Alias::new(name)))
    }

    fn add_and(mut self, expr: SimpleExpr) -> Self {
        self.where_clause = Some(match self.where_clause.take() {
            None => expr,
            Some(prev) => prev.and(expr),
        });
        self
    }

    fn add_or(mut self, expr: SimpleExpr) -> Self {
        self.where_clause = Some(match self.where_clause.take() {
            None => expr,
            Some(prev) => prev.or(expr),
        });
        self
    }

    // ── Selection / projection ───────────────────────────────────────────────

    pub fn select_only(mut self, columns: &[&str]) -> Self {
        self.select.clear_selects();
        for c in columns {
            self.select.column(sea_query::Alias::new(*c));
        }
        self
    }

    pub fn distinct(mut self) -> Self {
        self.select.distinct();
        self
    }

    // ── Equality / comparison ────────────────────────────────────────────────

    pub fn where_eq<T>(self, column: Column<M, T>, value: T) -> Self
    where
        T: Into<sea_query::Value>,
    {
        self.add_and(Self::col_of(column.name()).eq(value))
    }

    pub fn or_where_eq<T>(self, column: Column<M, T>, value: T) -> Self
    where
        T: Into<sea_query::Value>,
    {
        self.add_or(Self::col_of(column.name()).eq(value))
    }

    pub fn where_ne<T>(self, column: Column<M, T>, value: T) -> Self
    where
        T: Into<sea_query::Value>,
    {
        self.add_and(Self::col_of(column.name()).ne(value))
    }

    pub fn or_where_ne<T>(self, column: Column<M, T>, value: T) -> Self
    where
        T: Into<sea_query::Value>,
    {
        self.add_or(Self::col_of(column.name()).ne(value))
    }

    pub fn where_gt<T>(self, column: Column<M, T>, value: T) -> Self
    where
        T: Into<sea_query::Value>,
    {
        self.add_and(Self::col_of(column.name()).gt(value))
    }

    pub fn or_where_gt<T>(self, column: Column<M, T>, value: T) -> Self
    where
        T: Into<sea_query::Value>,
    {
        self.add_or(Self::col_of(column.name()).gt(value))
    }

    pub fn where_gte<T>(self, column: Column<M, T>, value: T) -> Self
    where
        T: Into<sea_query::Value>,
    {
        self.add_and(Self::col_of(column.name()).gte(value))
    }

    pub fn or_where_gte<T>(self, column: Column<M, T>, value: T) -> Self
    where
        T: Into<sea_query::Value>,
    {
        self.add_or(Self::col_of(column.name()).gte(value))
    }

    pub fn where_lt<T>(self, column: Column<M, T>, value: T) -> Self
    where
        T: Into<sea_query::Value>,
    {
        self.add_and(Self::col_of(column.name()).lt(value))
    }

    pub fn or_where_lt<T>(self, column: Column<M, T>, value: T) -> Self
    where
        T: Into<sea_query::Value>,
    {
        self.add_or(Self::col_of(column.name()).lt(value))
    }

    pub fn where_lte<T>(self, column: Column<M, T>, value: T) -> Self
    where
        T: Into<sea_query::Value>,
    {
        self.add_and(Self::col_of(column.name()).lte(value))
    }

    pub fn or_where_lte<T>(self, column: Column<M, T>, value: T) -> Self
    where
        T: Into<sea_query::Value>,
    {
        self.add_or(Self::col_of(column.name()).lte(value))
    }

    // ── IN / NOT IN ──────────────────────────────────────────────────────────

    pub fn where_in<T, I>(self, column: Column<M, T>, values: I) -> Self
    where
        T: Into<sea_query::Value>,
        I: IntoIterator<Item = T>,
    {
        self.add_and(Self::col_of(column.name()).is_in(values))
    }

    pub fn or_where_in<T, I>(self, column: Column<M, T>, values: I) -> Self
    where
        T: Into<sea_query::Value>,
        I: IntoIterator<Item = T>,
    {
        self.add_or(Self::col_of(column.name()).is_in(values))
    }

    pub fn where_not_in<T, I>(self, column: Column<M, T>, values: I) -> Self
    where
        T: Into<sea_query::Value>,
        I: IntoIterator<Item = T>,
    {
        self.add_and(Self::col_of(column.name()).is_not_in(values))
    }

    pub fn or_where_not_in<T, I>(self, column: Column<M, T>, values: I) -> Self
    where
        T: Into<sea_query::Value>,
        I: IntoIterator<Item = T>,
    {
        self.add_or(Self::col_of(column.name()).is_not_in(values))
    }

    // ── NULL / NOT NULL ──────────────────────────────────────────────────────

    pub fn where_null<T>(self, column: Column<M, T>) -> Self {
        self.add_and(Self::col_of(column.name()).is_null())
    }

    pub fn or_where_null<T>(self, column: Column<M, T>) -> Self {
        self.add_or(Self::col_of(column.name()).is_null())
    }

    pub fn where_not_null<T>(self, column: Column<M, T>) -> Self {
        self.add_and(Self::col_of(column.name()).is_not_null())
    }

    pub fn or_where_not_null<T>(self, column: Column<M, T>) -> Self {
        self.add_or(Self::col_of(column.name()).is_not_null())
    }

    // ── BETWEEN ──────────────────────────────────────────────────────────────

    pub fn where_between<T>(self, column: Column<M, T>, low: T, high: T) -> Self
    where
        T: Into<sea_query::Value>,
    {
        self.add_and(Self::col_of(column.name()).between(low, high))
    }

    pub fn or_where_between<T>(self, column: Column<M, T>, low: T, high: T) -> Self
    where
        T: Into<sea_query::Value>,
    {
        self.add_or(Self::col_of(column.name()).between(low, high))
    }

    pub fn where_not_between<T>(self, column: Column<M, T>, low: T, high: T) -> Self
    where
        T: Into<sea_query::Value>,
    {
        self.add_and(Self::col_of(column.name()).not_between(low, high))
    }

    // ── LIKE ─────────────────────────────────────────────────────────────────

    pub fn where_like(self, column: Column<M, String>, pattern: impl Into<String>) -> Self {
        self.add_and(Self::col_of(column.name()).like(pattern.into()))
    }

    pub fn or_where_like(self, column: Column<M, String>, pattern: impl Into<String>) -> Self {
        self.add_or(Self::col_of(column.name()).like(pattern.into()))
    }

    pub fn where_not_like(self, column: Column<M, String>, pattern: impl Into<String>) -> Self {
        self.add_and(Self::col_of(column.name()).not_like(pattern.into()))
    }

    // ── Column comparison ────────────────────────────────────────────────────

    pub fn where_column<T>(self, a: Column<M, T>, b: Column<M, T>) -> Self {
        self.add_and(Self::col_of(a.name()).equals((
            sea_query::Alias::new(M::TABLE),
            sea_query::Alias::new(b.name()),
        )))
    }

    // ── Raw escape hatches ───────────────────────────────────────────────────

    pub fn where_raw(self, raw: SimpleExpr) -> Self {
        self.add_and(raw)
    }

    pub fn or_where_raw(self, raw: SimpleExpr) -> Self {
        self.add_or(raw)
    }

    pub fn where_sql(self, sql: impl Into<String>) -> Self {
        self.add_and(Expr::cust(sql.into()))
    }

    pub fn or_where_sql(self, sql: impl Into<String>) -> Self {
        self.add_or(Expr::cust(sql.into()))
    }

    // ── Joins ────────────────────────────────────────────────────────────────

    /// `INNER JOIN table ON left_column = right_column`. Mirrors Eloquent's `join`.
    /// Columns are passed as fully-qualified strings (e.g. `"users.id"`).
    pub fn join(mut self, table: &str, left_column: &str, right_column: &str) -> Self {
        self.select.inner_join(
            sea_query::Alias::new(table),
            Expr::cust(&format!("{left_column} = {right_column}")),
        );
        self
    }

    /// `LEFT JOIN table ON ...`.
    pub fn left_join(mut self, table: &str, left_column: &str, right_column: &str) -> Self {
        self.select.left_join(
            sea_query::Alias::new(table),
            Expr::cust(&format!("{left_column} = {right_column}")),
        );
        self
    }

    /// `RIGHT JOIN table ON ...`.
    pub fn right_join(mut self, table: &str, left_column: &str, right_column: &str) -> Self {
        self.select.right_join(
            sea_query::Alias::new(table),
            Expr::cust(&format!("{left_column} = {right_column}")),
        );
        self
    }

    /// `CROSS JOIN table` (no ON clause).
    pub fn cross_join(mut self, table: &str) -> Self {
        self.select
            .cross_join(sea_query::Alias::new(table), Expr::cust("TRUE"));
        self
    }

    // ── Group / Having ───────────────────────────────────────────────────────

    pub fn group_by<T>(mut self, column: Column<M, T>) -> Self {
        self.select
            .add_group_by([Self::col_of(column.name()).into()]);
        self
    }

    /// `GROUP BY raw_sql`.
    pub fn group_by_raw(mut self, raw: impl Into<String>) -> Self {
        self.select.add_group_by([Expr::cust(&raw.into())]);
        self
    }

    pub fn having(mut self, expr: SimpleExpr) -> Self {
        self.select.and_having(expr);
        self
    }

    pub fn having_raw(mut self, sql: impl Into<String>) -> Self {
        self.select.and_having(Expr::cust(&sql.into()));
        self
    }

    // ── Soft deletes ─────────────────────────────────────────────────────────

    /// Include soft-deleted rows. Eloquent's `->withTrashed()`.
    pub fn with_trashed(mut self) -> Self {
        self.soft_delete_mode = SoftDeleteMode::WithTrashed;
        self
    }

    /// Only soft-deleted rows. Eloquent's `->onlyTrashed()`.
    pub fn only_trashed(mut self) -> Self {
        self.soft_delete_mode = SoftDeleteMode::OnlyTrashed;
        self
    }

    /// Explicitly exclude soft-deleted rows (this is the default for models
    /// with `#[soft_deletes]`). Eloquent's `->withoutTrashed()`.
    pub fn without_trashed(mut self) -> Self {
        self.soft_delete_mode = SoftDeleteMode::WithoutTrashed;
        self
    }

    // ── whereHas / withCount (relationship-aware subqueries) ─────────────────

    /// Filter parent rows by the existence of related child rows. Mirrors
    /// Eloquent's `->whereHas('posts', fn ($q) => $q->where(...))`.
    ///
    /// Emits `WHERE EXISTS (SELECT 1 FROM child WHERE child.fk = parent.lk AND <closure conditions>)`.
    ///
    /// ```ignore
    /// User::query()
    ///     .where_has(User::posts_rel(), |q| q.where_eq(Post::columns().published(), true))
    ///     .get(pool).await?;
    /// ```
    pub fn where_has<R, F>(self, _rel: R, f: F) -> Self
    where
        R: crate::relation::RelationDef<Parent = M>,
        R::Child: Model,
        for<'r> R::Child: FromRow<'r, PgRow>,
        F: FnOnce(QueryBuilder<R::Child>) -> QueryBuilder<R::Child>,
    {
        let exists_expr = build_exists_subquery::<M, R, F>(f, false);
        self.add_and(exists_expr)
    }

    /// Negated form of `where_has`. Mirrors Eloquent's `->whereDoesntHave(...)`.
    pub fn where_doesnt_have<R, F>(self, _rel: R, f: F) -> Self
    where
        R: crate::relation::RelationDef<Parent = M>,
        R::Child: Model,
        for<'r> R::Child: FromRow<'r, PgRow>,
        F: FnOnce(QueryBuilder<R::Child>) -> QueryBuilder<R::Child>,
    {
        let exists_expr = build_exists_subquery::<M, R, F>(f, true);
        self.add_and(exists_expr)
    }

    /// OR-combined `where_has`.
    pub fn or_where_has<R, F>(self, _rel: R, f: F) -> Self
    where
        R: crate::relation::RelationDef<Parent = M>,
        R::Child: Model,
        for<'r> R::Child: FromRow<'r, PgRow>,
        F: FnOnce(QueryBuilder<R::Child>) -> QueryBuilder<R::Child>,
    {
        let exists_expr = build_exists_subquery::<M, R, F>(f, false);
        self.add_or(exists_expr)
    }

    /// Pagination. Mirrors Eloquent's `->paginate($perPage, ['*'], 'page', $page)`.
    pub async fn paginate(
        self,
        per_page: u64,
        page: u64,
        pool: &sqlx::PgPool,
    ) -> Result<crate::paginator::Paginator<M>, Error> {
        let total = self.clone().count(pool).await?;
        let page = page.max(1);
        let per_page = per_page.max(1);
        let items = self
            .skip((page - 1) * per_page)
            .take(per_page)
            .get(pool)
            .await?;
        Ok(crate::paginator::Paginator::new(
            items, total, per_page, page,
        ))
    }

    /// Fetch the parent rows along with a related-row count. Mirrors Eloquent's
    /// `->withCount('posts')`. Returns `Vec<(M, i64)>` instead of dynamic attributes.
    ///
    /// ```ignore
    /// let users_with_counts: Vec<(User, i64)> = User::query()
    ///     .with_count_of(User::posts_rel(), pool)
    ///     .await?;
    /// ```
    pub async fn with_count_of<R>(
        self,
        _rel: R,
        pool: &sqlx::PgPool,
    ) -> Result<Vec<(M, i64)>, Error>
    where
        R: crate::relation::RelationDef<Parent = M>,
        R::Child: Model,
    {
        use sqlx::Row as _;
        const COUNT_ALIAS: &str = "__related_count";
        // Build the existing SELECT, then add a correlated subquery column.
        let mut select = self.prepare();
        let subquery_sql = format!(
            "(SELECT COUNT(*) FROM {child} WHERE {child}.{fk} = {parent}.{lk})",
            child = R::Child::TABLE,
            fk = R::foreign_key(),
            parent = M::TABLE,
            lk = R::local_key(),
        );
        select.expr_as(
            Expr::cust(&subquery_sql),
            sea_query::Alias::new(COUNT_ALIAS),
        );
        let (sql, values) = select.build_sqlx(PostgresQueryBuilder);
        let rows = sqlx::query_with(&sql, values).fetch_all(pool).await?;
        let mut out = Vec::with_capacity(rows.len());
        for row in &rows {
            let model = <M as FromRow<PgRow>>::from_row(row)?;
            let count: i64 = row.try_get(COUNT_ALIAS)?;
            out.push((model, count));
        }
        Ok(out)
    }

    // ── Order / pagination ───────────────────────────────────────────────────

    pub fn order_by<T>(mut self, column: Column<M, T>, ascending: bool) -> Self {
        self.select.order_by(
            sea_query::Alias::new(column.name()),
            if ascending { Order::Asc } else { Order::Desc },
        );
        self
    }

    pub fn order_by_asc<T>(self, column: Column<M, T>) -> Self {
        self.order_by(column, true)
    }

    pub fn order_by_desc<T>(self, column: Column<M, T>) -> Self {
        self.order_by(column, false)
    }

    pub fn latest(mut self) -> Self {
        self.select
            .order_by(sea_query::Alias::new("created_at"), Order::Desc);
        self
    }

    pub fn oldest(mut self) -> Self {
        self.select
            .order_by(sea_query::Alias::new("created_at"), Order::Asc);
        self
    }

    pub fn latest_by<T>(self, column: Column<M, T>) -> Self {
        self.order_by_desc(column)
    }

    pub fn oldest_by<T>(self, column: Column<M, T>) -> Self {
        self.order_by_asc(column)
    }

    pub fn in_random_order(mut self) -> Self {
        self.select
            .order_by_expr(Expr::cust("RANDOM()"), Order::Asc);
        self
    }

    pub fn reorder(mut self) -> Self {
        self.select.clear_order_by();
        self
    }

    pub fn limit(mut self, n: u64) -> Self {
        self.select.limit(n);
        self
    }

    pub fn take(self, n: u64) -> Self {
        self.limit(n)
    }

    pub fn offset(mut self, n: u64) -> Self {
        self.select.offset(n);
        self
    }

    pub fn skip(self, n: u64) -> Self {
        self.offset(n)
    }

    // ── Internal: prepare the SelectStatement for execution ──────────────────

    /// Apply the accumulated `where_clause` + the soft-delete filter to a clone
    /// of `select`, ready to be passed to sea-query.
    fn prepare(&self) -> sea_query::SelectStatement {
        let mut select = self.select.clone();
        let mut combined = self.where_clause.clone();

        // Apply the soft-delete filter if the model opted in.
        if M::SOFT_DELETES {
            let deleted_at = Expr::col(sea_query::Alias::new("deleted_at"));
            let filter = match self.soft_delete_mode {
                SoftDeleteMode::Default | SoftDeleteMode::WithoutTrashed => {
                    Some(deleted_at.is_null())
                }
                SoftDeleteMode::OnlyTrashed => Some(deleted_at.is_not_null()),
                SoftDeleteMode::WithTrashed => None,
            };
            if let Some(f) = filter {
                combined = Some(match combined {
                    None => f,
                    Some(prev) => prev.and(f),
                });
            }
        }

        if let Some(w) = combined {
            select.and_where(w);
        }
        select
    }

    // ── Terminals: rows ──────────────────────────────────────────────────────

    pub async fn get(self, pool: &sqlx::PgPool) -> Result<Vec<M>, Error> {
        let select = self.prepare();
        let (sql, values) = select.build_sqlx(PostgresQueryBuilder);
        let rows = sqlx::query_as_with::<_, M, _>(&sql, values)
            .fetch_all(pool)
            .await?;
        Ok(rows)
    }

    pub async fn first(self, pool: &sqlx::PgPool) -> Result<Option<M>, Error> {
        let mut select = self.prepare();
        select.limit(1);
        let (sql, values) = select.build_sqlx(PostgresQueryBuilder);
        let row = sqlx::query_as_with::<_, M, _>(&sql, values)
            .fetch_optional(pool)
            .await?;
        Ok(row)
    }

    pub async fn first_or_fail(self, pool: &sqlx::PgPool) -> Result<M, Error> {
        self.first(pool).await?.ok_or(Error::NotFound)
    }

    pub async fn pluck<T>(self, column: Column<M, T>, pool: &sqlx::PgPool) -> Result<Vec<T>, Error>
    where
        T: for<'r> sqlx::Decode<'r, sqlx::Postgres> + sqlx::Type<sqlx::Postgres> + Send + Unpin,
    {
        let mut select = self.prepare();
        select.clear_selects();
        select.column(sea_query::Alias::new(column.name()));
        let (sql, values) = select.build_sqlx(PostgresQueryBuilder);
        let rows: Vec<(T,)> = sqlx::query_as_with(&sql, values).fetch_all(pool).await?;
        Ok(rows.into_iter().map(|(v,)| v).collect())
    }

    pub async fn value<T>(
        self,
        column: Column<M, T>,
        pool: &sqlx::PgPool,
    ) -> Result<Option<T>, Error>
    where
        T: for<'r> sqlx::Decode<'r, sqlx::Postgres> + sqlx::Type<sqlx::Postgres> + Send + Unpin,
    {
        let mut select = self.prepare();
        select.clear_selects();
        select.column(sea_query::Alias::new(column.name()));
        select.limit(1);
        let (sql, values) = select.build_sqlx(PostgresQueryBuilder);
        let row: Option<(T,)> = sqlx::query_as_with(&sql, values)
            .fetch_optional(pool)
            .await?;
        Ok(row.map(|(v,)| v))
    }

    // ── Terminals: aggregates ────────────────────────────────────────────────

    pub async fn count(self, pool: &sqlx::PgPool) -> Result<i64, Error> {
        self.aggregate_i64(pool, "COUNT(*)").await
    }

    pub async fn min<T>(self, column: Column<M, T>, pool: &sqlx::PgPool) -> Result<Option<T>, Error>
    where
        T: for<'r> sqlx::Decode<'r, sqlx::Postgres> + sqlx::Type<sqlx::Postgres> + Send + Unpin,
    {
        self.aggregate_one_value(pool, &format!("MIN({})", column.name()))
            .await
    }

    pub async fn max<T>(self, column: Column<M, T>, pool: &sqlx::PgPool) -> Result<Option<T>, Error>
    where
        T: for<'r> sqlx::Decode<'r, sqlx::Postgres> + sqlx::Type<sqlx::Postgres> + Send + Unpin,
    {
        self.aggregate_one_value(pool, &format!("MAX({})", column.name()))
            .await
    }

    pub async fn sum<T>(self, column: Column<M, T>, pool: &sqlx::PgPool) -> Result<i64, Error> {
        self.aggregate_i64(
            pool,
            &format!("COALESCE(SUM({})::BIGINT, 0)", column.name()),
        )
        .await
    }

    pub async fn avg<T>(
        self,
        column: Column<M, T>,
        pool: &sqlx::PgPool,
    ) -> Result<Option<f64>, Error> {
        self.aggregate_one_value(pool, &format!("AVG({})::float8", column.name()))
            .await
    }

    pub async fn exists(self, pool: &sqlx::PgPool) -> Result<bool, Error> {
        Ok(self.count(pool).await? > 0)
    }

    pub async fn doesnt_exist(self, pool: &sqlx::PgPool) -> Result<bool, Error> {
        Ok(self.count(pool).await? == 0)
    }

    // ── helpers ──────────────────────────────────────────────────────────────

    async fn aggregate_i64(self, pool: &sqlx::PgPool, expr: &str) -> Result<i64, Error> {
        let mut q = self.prepare();
        q.clear_selects();
        // Drop ORDER BY / LIMIT / OFFSET on aggregates — Postgres rejects them
        // alongside a bare COUNT(*).
        q.clear_order_by();
        q.reset_limit();
        q.reset_offset();
        q.expr(Expr::cust(expr));
        let (sql, values) = q.build_sqlx(PostgresQueryBuilder);
        let (v,): (i64,) = sqlx::query_as_with(&sql, values).fetch_one(pool).await?;
        Ok(v)
    }

    async fn aggregate_one_value<T>(
        self,
        pool: &sqlx::PgPool,
        expr: &str,
    ) -> Result<Option<T>, Error>
    where
        T: for<'r> sqlx::Decode<'r, sqlx::Postgres> + sqlx::Type<sqlx::Postgres> + Send + Unpin,
    {
        let mut q = self.prepare();
        q.clear_selects();
        q.clear_order_by();
        q.reset_limit();
        q.reset_offset();
        q.expr(Expr::cust(expr));
        let (sql, values) = q.build_sqlx(PostgresQueryBuilder);
        let row: Option<(Option<T>,)> = sqlx::query_as_with(&sql, values)
            .fetch_optional(pool)
            .await?;
        Ok(row.and_then(|(v,)| v))
    }
}

// ─── whereHas / whereDoesntHave subquery builder ───────────────────────────

fn build_exists_subquery<M, R, F>(f: F, negate: bool) -> SimpleExpr
where
    M: Model,
    for<'r> M: FromRow<'r, PgRow>,
    R: crate::relation::RelationDef<Parent = M>,
    R::Child: Model,
    for<'r> R::Child: FromRow<'r, PgRow>,
    F: FnOnce(QueryBuilder<R::Child>) -> QueryBuilder<R::Child>,
{
    // Build the inner SELECT against the child model, then apply user filters
    // and the correlation `child.fk = parent.lk`.
    let inner = f(QueryBuilder::<R::Child>::new());
    let mut child_select = inner.prepare();
    // Drop the FROM-clauses sea-query computed and replace with just the child
    // table + a SELECT 1 — we only need EXISTS semantics, not the columns.
    child_select.clear_selects();
    child_select.expr(Expr::cust("1"));
    // Add the correlation predicate.
    let correlate = Expr::cust(&format!(
        "{child}.{fk} = {parent}.{lk}",
        child = R::Child::TABLE,
        fk = R::foreign_key(),
        parent = M::TABLE,
        lk = R::local_key(),
    ));
    child_select.and_where(correlate);

    // sea-query's `Expr::exists` takes a SubQueryStatement.
    let exists = sea_query::Expr::exists(child_select);
    if negate {
        exists.not()
    } else {
        exists
    }
}
