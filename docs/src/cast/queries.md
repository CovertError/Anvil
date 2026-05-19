# The query builder

`Model::query()` returns a `QueryBuilder<Self>` with the full Eloquent surface — chainable, type-safe methods. Column references via `Model::columns().col()` give compile-time guarantees about value types.

## Where clauses

| Method                                    | SQL emitted                          | Eloquent equivalent                            |
| ----------------------------------------- | ------------------------------------ | ---------------------------------------------- |
| `.where_eq(col, val)`                     | `WHERE col = val`                    | `->where('col', $val)`                         |
| `.where_ne(col, val)`                     | `WHERE col != val`                   | `->where('col', '!=', $val)`                   |
| `.where_gt(col, val)`                     | `WHERE col > val`                    | `->where('col', '>', $val)`                    |
| `.where_gte(col, val)`                    | `WHERE col >= val`                   | `->where('col', '>=', $val)`                   |
| `.where_lt(col, val)`                     | `WHERE col < val`                    | `->where('col', '<', $val)`                    |
| `.where_lte(col, val)`                    | `WHERE col <= val`                   | `->where('col', '<=', $val)`                   |
| `.where_in(col, iter)`                    | `WHERE col IN (...)`                 | `->whereIn('col', [...])`                      |
| `.where_not_in(col, iter)`                | `WHERE col NOT IN (...)`             | `->whereNotIn('col', [...])`                   |
| `.where_null(col)`                        | `WHERE col IS NULL`                  | `->whereNull('col')`                           |
| `.where_not_null(col)`                    | `WHERE col IS NOT NULL`              | `->whereNotNull('col')`                        |
| `.where_between(col, low, high)`          | `WHERE col BETWEEN low AND high`     | `->whereBetween('col', [low, high])`           |
| `.where_not_between(col, low, high)`      | `WHERE col NOT BETWEEN low AND high` | `->whereNotBetween('col', [low, high])`        |
| `.where_like(col, pattern)`               | `WHERE col LIKE pattern`             | `->where('col', 'like', $pattern)`             |
| `.where_not_like(col, pattern)`           | `WHERE col NOT LIKE pattern`         | `->where('col', 'not like', $pattern)`         |
| `.where_column(a, b)`                     | `WHERE a = b`                        | `->whereColumn('a', '=', 'b')`                 |
| `.where_raw(expr)`                        | arbitrary sea-query `SimpleExpr`     | `->whereRaw('...')`                            |
| `.where_sql("col > NOW() - INTERVAL ...")` | raw SQL fragment as AND predicate    | `->whereRaw('...')`                            |

### OR variants

Every `where_*` method has an `or_where_*` counterpart that ORs with the running condition. Left-to-right grouping: `where_eq(a).where_eq(b).or_where_eq(c)` produces `((a AND b) OR c)`. For explicit grouping, build a `SimpleExpr` and pass to `where_raw`.

```rust
Post::query()
    .where_eq(Post::columns().author_id(), 1_i64)
    .or_where_eq(Post::columns().author_id(), 2_i64)
    .or_where_in(Post::columns().id(), vec![10_i64, 20])
    .or_where_null(Post::columns().deleted_at())
    .get(pool).await?;
```

| Method                                | SQL emitted                                |
| ------------------------------------- | ------------------------------------------ |
| `.or_where_eq(col, val)`              | `OR col = val`                             |
| `.or_where_ne` / `gt` / `gte` / `lt` / `lte` | `OR col {op} val`                   |
| `.or_where_in(col, iter)`             | `OR col IN (...)`                          |
| `.or_where_not_in(col, iter)`         | `OR col NOT IN (...)`                      |
| `.or_where_null(col)` / `not_null`    | `OR col IS [NOT] NULL`                     |
| `.or_where_between(col, low, high)`   | `OR col BETWEEN low AND high`              |
| `.or_where_like(col, pattern)`        | `OR col LIKE pattern`                      |
| `.or_where_raw(expr)`                 | OR with arbitrary sea-query SimpleExpr     |
| `.or_where_sql("...")`                | OR with arbitrary SQL fragment             |

## Aggregates

| Method                          | SQL                       | Returns                |
| ------------------------------- | ------------------------- | ---------------------- |
| `.count(pool)`                  | `SELECT COUNT(*)`         | `i64`                  |
| `.sum(col, pool)`               | `SELECT COALESCE(SUM(col)::BIGINT, 0)` | `i64`     |
| `.min(col, pool)`               | `SELECT MIN(col)`         | `Option<T>`            |
| `.max(col, pool)`               | `SELECT MAX(col)`         | `Option<T>`            |
| `.avg(col, pool)`               | `SELECT AVG(col)::float8` | `Option<f64>`          |
| `.exists(pool)`                 | `SELECT COUNT(*) > 0`     | `bool`                 |
| `.doesnt_exist(pool)`           | `SELECT COUNT(*) == 0`    | `bool`                 |

## Ordering

| Method                              | SQL                                | Notes                              |
| ----------------------------------- | ---------------------------------- | ---------------------------------- |
| `.order_by_asc(col)`                | `ORDER BY col ASC`                 |                                    |
| `.order_by_desc(col)`               | `ORDER BY col DESC`                |                                    |
| `.order_by(col, ascending)`         | conditional                        |                                    |
| `.latest()`                         | `ORDER BY created_at DESC`         | Eloquent's `->latest()`            |
| `.oldest()`                         | `ORDER BY created_at ASC`          | Eloquent's `->oldest()`            |
| `.latest_by(col)`                   | `ORDER BY col DESC`                |                                    |
| `.oldest_by(col)`                   | `ORDER BY col ASC`                 |                                    |
| `.in_random_order()`                | `ORDER BY RANDOM()`                | Postgres-only                      |
| `.reorder()`                        | clear ordering                     | Eloquent's `->reorder()`           |

## Pagination

| Method                | SQL                | Eloquent alias            |
| --------------------- | ------------------ | ------------------------- |
| `.limit(n)` / `.take(n)` | `LIMIT n`        | `->take($n)`              |
| `.offset(n)` / `.skip(n)` | `OFFSET n`      | `->skip($n)`              |

## Selection

| Method                       | SQL                          |
| ---------------------------- | ---------------------------- |
| `.select_only(&["a", "b"])`  | `SELECT a, b`                |
| `.distinct()`                | `SELECT DISTINCT`            |

## Joins

The query builder fully-qualifies its own column references (`authors.id` etc.) so joins disambiguate cleanly. Column names in join clauses are passed as strings since they often refer to other tables.

| Method                                            | SQL                                          |
| ------------------------------------------------- | -------------------------------------------- |
| `.join("posts", "authors.id", "posts.author_id")` | `INNER JOIN posts ON authors.id = posts.author_id` |
| `.left_join(...)`                                 | `LEFT JOIN ...`                              |
| `.right_join(...)`                                | `RIGHT JOIN ...`                             |
| `.cross_join("tags")`                             | `CROSS JOIN tags`                            |

```rust
// Posts written by author 1 — joined fetch
let posts_by_author: i64 = Post::query()
    .join("authors", "posts.author_id", "authors.id")
    .where_eq(Author::columns().email(), "ada@x.com".to_string())
    .count(pool).await?;
```

## Group by / having

| Method                              | SQL                          |
| ----------------------------------- | ---------------------------- |
| `.group_by(col)`                    | `GROUP BY table.col`         |
| `.group_by_raw("DATE(created_at)")` | `GROUP BY <raw>`             |
| `.having(expr)`                     | `HAVING <SimpleExpr>`        |
| `.having_raw("COUNT(*) >= 2")`      | `HAVING <raw>`               |

## Soft deletes

When a model derives `#[soft_deletes]` (and has a `deleted_at` column), `Model::query()` automatically filters out trashed rows. The scope methods toggle that:

| Method                  | Effect                                                        |
| ----------------------- | ------------------------------------------------------------- |
| (default)               | `WHERE deleted_at IS NULL` (auto-applied)                     |
| `.with_trashed()`       | No `deleted_at` filter — include trashed                      |
| `.only_trashed()`       | `WHERE deleted_at IS NOT NULL` — only trashed                 |
| `.without_trashed()`    | Explicit `WHERE deleted_at IS NULL`                           |

On the model:
- `.delete(pool)` — soft delete (UPDATE deleted_at = NOW())
- `.force_delete(pool)` — hard DELETE bypassing soft-delete
- `.restore(pool)` — UPDATE deleted_at = NULL; returns the refreshed model

```rust
#[derive(Model)]
#[table("posts")]
#[soft_deletes]
struct Post { id: i64, title: String, deleted_at: Option<DateTime<Utc>>, ... }

post.delete(pool).await?;             // soft delete
Post::query().count(pool).await?;     // excludes trashed (auto)
Post::query().only_trashed().get(pool).await?;  // just the bin
Post::query().with_trashed().count(pool).await?;  // everything

let restored = trashed_post.restore(pool).await?;   // un-trash
deleted_post.force_delete(pool).await?;             // permanent

## Terminals — fetching results

| Method                            | Returns               | Notes                                                       |
| --------------------------------- | --------------------- | ----------------------------------------------------------- |
| `.get(pool)`                      | `Vec<M>`              |                                                             |
| `.first(pool)`                    | `Option<M>`           | Adds `LIMIT 1`                                              |
| `.first_or_fail(pool)`            | `M`                   | `Error::NotFound` if no match                               |
| `.count(pool)`                    | `i64`                 |                                                             |
| `.exists(pool)` / `.doesnt_exist`| `bool`                 |                                                             |
| `.pluck(col, pool)`               | `Vec<T>`              | One column only                                             |
| `.value(col, pool)`               | `Option<T>`           | First row, single column                                    |

## Examples

```rust
use anvilforge::prelude::*;

// Where + order + limit
let recent = Post::query()
    .where_eq(Post::columns().published(), true)
    .where_gte(Post::columns().created_at(), one_week_ago)
    .where_not_null(Post::columns().approved_at())
    .latest()
    .take(10)
    .get(c.pool())
    .await?;

// IN clause
let by_authors = Post::query()
    .where_in(Post::columns().author_id(), vec![1_i64, 2, 3, 5, 8])
    .get(c.pool())
    .await?;

// Pattern match
let drafts = Post::query()
    .where_like(Post::columns().title(), "Draft%")
    .where_null(Post::columns().published_at())
    .get(c.pool())
    .await?;

// Aggregates
let total_words: i64 = Post::query().sum(Post::columns().word_count(), c.pool()).await?;
let avg_views: Option<f64> = Post::query().avg(Post::columns().view_count(), c.pool()).await?;
let any_unpublished: bool = Post::query()
    .where_eq(Post::columns().published(), false)
    .exists(c.pool())
    .await?;

// Pluck — get just the column values
let titles: Vec<String> = Post::query()
    .where_eq(Post::columns().published(), true)
    .order_by_desc(Post::columns().id())
    .limit(20)
    .pluck(Post::columns().title(), c.pool())
    .await?;

// First-row column shortcut
let latest_title: Option<String> = Post::query()
    .latest()
    .value(Post::columns().title(), c.pool())
    .await?;

// Pagination basics
let page2 = Post::query()
    .order_by_desc(Post::columns().id())
    .skip(20)
    .take(10)
    .get(c.pool())
    .await?;
```

## Model-level static helpers

These are emitted by `#[derive(Model)]` on each model:

```rust
// Reads
let user: Option<User> = User::find(pool, 1).await?;
let user: User = User::find_or_fail(pool, 1).await?;
let users: Vec<User> = User::all(pool).await?;
let some: Vec<User> = User::find_many(pool, [1, 2, 3]).await?;

// Writes (returns the updated/inserted model)
let user = User { id: 0, ..u }.insert(pool).await?;
let user = user.update(pool).await?;
let user = User { id: 0, ..u }.save(pool).await?;   // insert-or-update
user.delete(pool).await?;
let deleted_count: u64 = User::destroy(pool, [1, 2, 3]).await?;
User::truncate(pool).await?;

// Reload from DB
user.refresh(pool).await?;                            // mutates self
let f: Option<User> = user.fresh(pool).await?;        // new instance

// Replicate (clone with PK reset)
let copy = user.replicate();   // copy.id == 0; insert/save gives it a new id
let copy = copy.save(pool).await?;
```

## Pagination

`paginate(per_page, page, pool)` returns a `Paginator<M>` with the rows + metadata. Mirrors Laravel's `LengthAwarePaginator`.

```rust
let page = Post::query()
    .where_eq(Post::columns().published(), true)
    .latest()
    .paginate(20, 1, c.pool())
    .await?;

page.items          // Vec<Post>, length ≤ 20
page.total          // total matching rows (i64)
page.per_page       // 20
page.current_page   // 1
page.last_page      // ceil(total / per_page)
page.has_more_pages()
page.has_previous_pages()
page.next_page()    // Option<u64>
page.previous_page()
page.from()         // 1-indexed start position in the full result set
page.to()           // 1-indexed end position
page.map(|post| PostView::from(post))   // transform items, keep metadata
```

`Paginator<T>` is `serde::Serialize`-derivable, so you can `Json(page)` it from a handler and the client gets `{ items: [...], total, per_page, current_page, last_page }`.

## Relationship-aware queries — `whereHas` / `withCount`

`where_has` filters parents by the existence of related child rows. Mirrors Eloquent's `->whereHas('posts', fn ($q) => ...)`.

```rust
// Authors who have at least one published post:
let active: Vec<Author> = Author::query()
    .where_has(Author::posts_rel(), |q| {
        q.where_eq(Post::columns().published(), true)
    })
    .get(c.pool())
    .await?;

// Negated form — Eloquent's whereDoesntHave:
let lazy: Vec<Author> = Author::query()
    .where_doesnt_have(Author::posts_rel(), |q| q)
    .get(c.pool())
    .await?;

// OR-combined:
let either: Vec<Author> = Author::query()
    .where_eq(Author::columns().vip(), true)
    .or_where_has(Author::posts_rel(), |q| {
        q.where_eq(Post::columns().published(), true)
    })
    .get(c.pool())
    .await?;
```

`with_count_of` is Eloquent's `->withCount('posts')`. Returns `Vec<(M, i64)>`:

```rust
let users_with_counts: Vec<(Author, i64)> = Author::query()
    .order_by_asc(Author::columns().id())
    .with_count_of(Author::posts_rel(), c.pool())
    .await?;

for (author, post_count) in users_with_counts {
    println!("{} has {} posts", author.name, post_count);
}
```

## Local scopes (chained)

Mirror Eloquent's `scopeActive()` / `scopePublished()` pattern. Use the `scopes!` macro to define a user-named trait and an impl for `QueryBuilder<Model>` — scopes then chain naturally on the query builder.

```rust
use anvilforge::cast::scopes;

scopes!(UserScopes for User {
    fn active(q) -> q.where_eq(User::columns().active(), true);
    fn verified(q) -> q.where_not_null(User::columns().email_verified_at());
    fn role(q, name: String) -> q.where_eq(User::columns().role(), name);
});
```

Then bring the trait into scope wherever you want to use it:

```rust
use crate::app::Models::UserScopes;

let admins = User::query()
    .active()
    .verified()
    .role("admin".to_string())
    .get(c.pool())
    .await?;
```

The macro supports scope arguments (typed like normal Rust function args). Multi-scope queries chain in any order.

### find / create / update helpers

```rust
// Find by criteria; insert default if no match. Mirrors firstOrCreate.
let user = User::first_or_create(
    pool,
    |q| q.where_eq(User::columns().email(), "ada@x.com".to_string()),
    User { id: 0, name: "Ada".into(), email: "ada@x.com".into(), ..Default::default() },
).await?;

// Find by criteria and UPDATE with attrs, or INSERT if no match. Mirrors updateOrCreate.
let user = User::update_or_create(
    pool,
    |q| q.where_eq(User::columns().email(), "ada@x.com".to_string()),
    User { id: 0, name: "Renamed".into(), email: "ada@x.com".into(), ..Default::default() },
).await?;
```

[Next: relationships →](relationships.md)
