# Cast models

Cast is Anvilforge's ORM — Laravel's Eloquent rebuilt as proc-macro-driven, statically typed code on top of [sqlx](https://docs.rs/sqlx). A model is just a struct with `#[derive(Model)]`.

## Defining a model

```bash
smith make:model Post --with-migration
```

```rust
use anvilforge::prelude::*;

#[derive(Debug, Clone, Serialize, Deserialize, Model)]
#[table("posts")]
pub struct Post {
    pub id: i64,
    pub title: String,
    pub body: String,
    pub published: bool,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}
```

The `#[derive(Model)]` macro generates:

- A `Model` trait implementation: `Post::TABLE`, `Post::COLUMNS`, `Post::PK_COLUMN`.
- A typed `Columns` accessor: `Post::columns().email()` returns `Column<Post, String>`.
- A `sqlx::FromRow` implementation, decoding rows by column name.
- Inherent helpers: `Post::find(pool, id)`, `Post::query()`, `Post::all(pool)`.

## Attribute reference

| Attribute                                   | Meaning                                     |
| ------------------------------------------- | ------------------------------------------- |
| `#[table("name")]`                          | Override the table name (default: pluralized snake_case of struct name). |
| `#[primary_key("col")]`                     | Override the primary key column (default: `id`). |
| `#[has_many(Other, foreign_key = "...")]`   | Declare a 1-to-many relationship.           |
| `#[has_one(Other, foreign_key = "...")]`    | Declare a 1-to-1 relationship.              |
| `#[belongs_to(Other, foreign_key = "...")]` | Declare an inverse relationship.            |

If you omit `foreign_key`, the macro infers it (`user_id` for `belongs_to(User)`, etc.).

## Eloquent-style API — read + write

The derive emits the full Laravel `Model` surface — find, save, insert, update, delete, find-or-fail:

```rust
// Reads
let post: Option<Post> = Post::find(c.pool(), 1).await?;
let post: Post = Post::find_or_fail(c.pool(), 1).await?;   // 404 on missing
let posts: Vec<Post> = Post::all(c.pool()).await?;

// Insert — primary key set via `RETURNING id` automatically
let post = Post {
    id: 0,
    title: "Hello".into(),
    body: "World".into(),
    published: true,
    created_at: None,
    updated_at: None,
}
.insert(c.pool())
.await?;
assert!(post.id > 0);

// Update — bumps `updated_at` automatically
let mut post = Post::find_or_fail(c.pool(), 1).await?;
post.title = "Renamed".into();
let post = post.update(c.pool()).await?;

// save() — INSERT if `id == 0`, UPDATE otherwise (matches Eloquent's $model->save())
let post = Post { id: 0, ..post_data }.save(c.pool()).await?;

// Delete
post.delete(c.pool()).await?;
```

Fields automatically excluded from `INSERT` / `UPDATE`:
- The primary key (filled in via `RETURNING`)
- `created_at`, `updated_at`, `deleted_at` (DB defaults; `updated_at` is bumped on every `update()`)

## Compile-time type safety

Cast's killer feature: the query builder catches type mismatches at compile time.

```rust
// ✓ compiles
Post::query()
    .where_eq(Post::columns().published(), true)
    .get(pool)
    .await?;

// ✗ compile error: expected bool, got i32
Post::query()
    .where_eq(Post::columns().published(), 42)
    .get(pool)
    .await?;
```

The error points right at the literal, with a readable type-mismatch message. This is the central design bet of Cast — Eloquent-shaped ergonomics with the safety of Diesel.

[Next: query builder →](queries.md)
