//! Query builder. Typed where clauses on top of sea-query + sqlx.

use std::marker::PhantomData;

use sea_query::{Expr, Order, PostgresQueryBuilder, Query, SimpleExpr};
use sea_query_binder::SqlxBinder;
use sqlx::{FromRow, postgres::PgRow};

use crate::column::Column;
use crate::model::Model;
use crate::pool::Pool;
use crate::Error;

pub struct QueryBuilder<M: Model> {
    select: sea_query::SelectStatement,
    _marker: PhantomData<M>,
}

impl<M: Model> Default for QueryBuilder<M> {
    fn default() -> Self {
        Self::new()
    }
}

impl<M: Model> QueryBuilder<M>
where
    for<'r> M: FromRow<'r, PgRow>,
{
    pub fn new() -> Self {
        let mut select = Query::select();
        select
            .from(sea_query::Alias::new(M::TABLE))
            .columns(M::COLUMNS.iter().map(|c| sea_query::Alias::new(*c)));
        Self {
            select,
            _marker: PhantomData,
        }
    }

    /// Type-safe equality: column type must match value type.
    pub fn where_eq<T>(mut self, column: Column<M, T>, value: T) -> Self
    where
        T: Into<sea_query::Value>,
    {
        self.select
            .and_where(Expr::col(sea_query::Alias::new(column.name())).eq(value));
        self
    }

    pub fn where_ne<T>(mut self, column: Column<M, T>, value: T) -> Self
    where
        T: Into<sea_query::Value>,
    {
        self.select
            .and_where(Expr::col(sea_query::Alias::new(column.name())).ne(value));
        self
    }

    pub fn where_gt<T>(mut self, column: Column<M, T>, value: T) -> Self
    where
        T: Into<sea_query::Value>,
    {
        self.select
            .and_where(Expr::col(sea_query::Alias::new(column.name())).gt(value));
        self
    }

    pub fn where_lt<T>(mut self, column: Column<M, T>, value: T) -> Self
    where
        T: Into<sea_query::Value>,
    {
        self.select
            .and_where(Expr::col(sea_query::Alias::new(column.name())).lt(value));
        self
    }

    pub fn where_in<T, I>(mut self, column: Column<M, T>, values: I) -> Self
    where
        T: Into<sea_query::Value>,
        I: IntoIterator<Item = T>,
    {
        self.select
            .and_where(Expr::col(sea_query::Alias::new(column.name())).is_in(values));
        self
    }

    pub fn where_raw(mut self, raw: SimpleExpr) -> Self {
        self.select.and_where(raw);
        self
    }

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

    pub fn limit(mut self, n: u64) -> Self {
        self.select.limit(n);
        self
    }

    pub fn offset(mut self, n: u64) -> Self {
        self.select.offset(n);
        self
    }

    pub async fn get(self, pool: &Pool) -> Result<Vec<M>, Error> {
        let (sql, values) = self.select.build_sqlx(PostgresQueryBuilder);
        let rows = sqlx::query_as_with::<_, M, _>(&sql, values)
            .fetch_all(pool)
            .await?;
        Ok(rows)
    }

    pub async fn first(mut self, pool: &Pool) -> Result<Option<M>, Error> {
        self.select.limit(1);
        let (sql, values) = self.select.build_sqlx(PostgresQueryBuilder);
        let row = sqlx::query_as_with::<_, M, _>(&sql, values)
            .fetch_optional(pool)
            .await?;
        Ok(row)
    }

    pub async fn count(self, pool: &Pool) -> Result<i64, Error> {
        let mut count_query = self.select.clone();
        count_query.clear_selects();
        count_query.expr(Expr::count(Expr::col(sea_query::Alias::new("*"))));
        let (sql, values) = count_query.build_sqlx(PostgresQueryBuilder);
        let (count,): (i64,) = sqlx::query_as_with(&sql, values).fetch_one(pool).await?;
        Ok(count)
    }
}
