# Form requests & validation

A form request bundles input parsing and validation into a single struct that's used directly as an Axum extractor. Anvilforge's validation is powered by [`garde`](https://github.com/jprochazk/garde) under the hood.

## Define a request

```bash
smith make:request StorePostRequest
```

```rust
use anvilforge::prelude::*;
use garde::Validate;

#[derive(Debug, Deserialize, Validate, FormRequest)]
pub struct StorePostRequest {
    #[garde(length(min = 1, max = 200))]
    pub title: String,

    #[garde(length(min = 1))]
    pub body: String,

    #[garde(skip)]
    pub author_id: i64,

    #[garde(skip)]
    pub published: Option<bool>,
}
```

The `#[derive(FormRequest)]` macro implements `FromRequest` on the struct. It:

1. Reads the request body (handles JSON or form-urlencoded based on content-type).
2. Deserializes into the typed struct via `serde`.
3. Runs validation via `garde`.
4. Returns `422 Unprocessable Entity` with field errors on failure.

## Use it in a handler

```rust
async fn store(
    State(c): State<Container>,
    Auth(user): Auth<User>,
    payload: StorePostRequest,
) -> Result<Redirect> {
    let row: (i64,) = sqlx::query_as(
        "INSERT INTO posts (author_id, title, body, published) VALUES ($1, $2, $3, $4) RETURNING id"
    )
    .bind(user.id)
    .bind(&payload.title)
    .bind(&payload.body)
    .bind(payload.published.unwrap_or(false))
    .fetch_one(c.pool())
    .await
    .map_err(Error::Database)?;

    Ok(Redirect::to(format!("/posts/{}", row.0)))
}
```

## Garde rule cheatsheet

```rust
#[garde(email)]                          // valid email
#[garde(url)]                            // valid URL
#[garde(ip)]                             // valid IPv4 or IPv6
#[garde(length(min = 1))]                // string/collection length
#[garde(length(min = 1, max = 80))]
#[garde(range(min = 18, max = 120))]     // numeric range
#[garde(pattern(r"^[a-z0-9_]+$"))]       // regex
#[garde(contains("@"))]                  // substring
#[garde(custom(my_validator_fn))]        // custom validator
#[garde(skip)]                           // skip validation
```

Full reference: [garde documentation](https://docs.rs/garde).

## Error response shape

A failed request returns 422 with this JSON body:

```json
{
  "message": "The given data was invalid.",
  "errors": {
    "title": ["length is required to be at least 1"],
    "body":  ["length is required to be at least 1"]
  }
}
```

For Forge views, you can read these errors from `session::take_flash(session, "_errors")` after a redirect.

[Next: the service container →](container.md)
