# Relationships

Declare relationships as struct attributes on the model. Cast generates the per-row loader methods at compile time.

## `has_many`

```rust
#[derive(Model, Serialize, Deserialize, Clone, Debug)]
#[table("authors")]
#[has_many(crate::app::models::Post, foreign_key = "author_id")]
pub struct Author {
    pub id: i64,
    pub name: String,
    pub email: String,
}
```

Cast generates:

```rust
impl Author {
    pub async fn posts(&self, pool: &Pool) -> Result<Vec<Post>> { /* ... */ }
    pub fn posts_rel() -> AuthorPostsRel { /* ZST for eager loading */ }
}
```

Use it:

```rust
let author = Author::find(pool, 1).await?.unwrap();
let posts = author.posts(pool).await?;
```

## `belongs_to`

```rust
#[derive(Model, Serialize, Deserialize, Clone, Debug)]
#[table("posts")]
#[belongs_to(crate::app::models::Author, foreign_key = "author_id")]
pub struct Post {
    pub id: i64,
    pub author_id: i64,
    pub title: String,
    pub body: String,
}
```

Generates:

```rust
impl Post {
    pub async fn author(&self, pool: &Pool) -> Result<Option<Author>> { /* ... */ }
}
```

## `has_one`

Like `has_many` but returns `Option<C>` instead of `Vec<C>`:

```rust
#[has_one(crate::app::models::Profile, foreign_key = "user_id")]
pub struct User { ... }
```

## Eager loading (deferred to v0.2)

The plan is for `Model::query().with(Author::posts_rel()).get(pool)` to fetch the parents and run a second `WHERE foreign_key IN (...)` to bulk-load related rows — exactly the way Eloquent's `with()` does. The infrastructure types (`RelationDef`, `Loaded<M>`) are in place; the `.with()` query builder method ships in v0.2.

For now, eager-loading-in-spirit looks like:

```rust
let authors = Author::all(pool).await?;
let author_ids: Vec<i64> = authors.iter().map(|a| a.id).collect();
let all_posts: Vec<Post> = Post::query()
    .where_in(Post::columns().author_id(), author_ids)
    .get(pool)
    .await?;
// ... bucket them yourself
```

The query builder's typed-IN clause makes this almost as clean as `.with()`. v0.2 will sugar it.

[Next: migrations →](migrations.md)
