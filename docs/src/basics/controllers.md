# Controllers & responses

A controller is just a struct with associated async methods. Generate one with:

```bash
smith make:controller PostController --resource
```

This generates the seven REST methods (index/create/store/show/edit/update/destroy):

```rust
use anvilforge::prelude::*;

pub struct PostController;

impl PostController {
    pub async fn index(State(c): State<Container>) -> Result<ViewResponse> { ... }
    pub async fn show(Path(id): Path<i64>) -> Result<ViewResponse> { ... }
    pub async fn create() -> Result<ViewResponse> { ... }
    pub async fn store() -> Result<Redirect> { ... }
    pub async fn edit(Path(_id): Path<i64>) -> Result<ViewResponse> { ... }
    pub async fn update(Path(_id): Path<i64>) -> Result<Redirect> { ... }
    pub async fn destroy(Path(_id): Path<i64>) -> Result<Redirect> { ... }
}
```

Wire each method into the router:

```rust
r.get("/posts", PostController::index)
    .get("/posts/:id", PostController::show)
    .get("/posts/create", PostController::create)
    .post("/posts", PostController::store)
    .get("/posts/:id/edit", PostController::edit)
    .put("/posts/:id", PostController::update)
    .delete("/posts/:id", PostController::destroy)
```

## Response types

Anvilforge handlers return `Result<R, Error>` where `R` is any type that implements `IntoResponse`. The most useful ones:

### `ViewResponse` — HTML

```rust
async fn home() -> Result<ViewResponse> {
    Ok(ViewResponse::new("<h1>Hello</h1>"))
}
```

For Forge templates, see [Templates](templates.md).

### `Json<T>` — JSON

```rust
async fn index() -> Result<Json<Vec<Post>>> {
    Ok(Json(posts))
}
```

### `Redirect` — redirects

```rust
async fn store() -> Result<Redirect> {
    Ok(Redirect::to("/posts").with("success", "Created!"))
}
```

`.with(key, value)` flashes a message into the session — readable on the next request.

### `&'static str` / `String` — plain text

```rust
async fn health() -> &'static str {
    "ok"
}
```

## Errors

Handlers `?`-propagate freely. Anvilforge's `Error` type implements `IntoResponse`, so common error kinds become the right HTTP status automatically:

| Error                          | Status |
| ------------------------------ | ------ |
| `Error::NotFound`              | 404    |
| `Error::Unauthenticated`       | 401    |
| `Error::Forbidden(_)`          | 403    |
| `Error::Validation(_)`         | 422    |
| `Error::BadRequest(_)`         | 400    |
| `Error::Conflict(_)`           | 409    |
| sqlx `RowNotFound`             | 404    |
| anything else                  | 500    |

```rust
async fn show(State(c): State<Container>, Path(id): Path<i64>) -> Result<Json<Post>> {
    let post = Post::find(c.pool(), id).await?.ok_or(Error::NotFound)?;
    Ok(Json(post))
}
```

## Route model binding

Cast models implement `FromRequestParts` for `Path<Self>`, so you can do this:

```rust
async fn show(Path(post): Path<Post>) -> Result<Json<Post>> {
    Ok(Json(post))
}
```

The extractor calls `Post::find(pool, id).await?.ok_or(NotFound)` internally.

[Next: Forge templates →](templates.md)
