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

## Laravel rules → Garde attributes

If you reach for Laravel's pipe-separated rule strings (`'required|string|max:255'`),
here's the direct translation table. Each Laravel rule maps to one (or
zero) `#[garde(...)]` attribute on the field.

| Laravel rule | Garde attribute | Notes |
|---|---|---|
| `required` | (use a non-`Option` field type) | Rust's type system already enforces this — make the field `String` not `Option<String>`. |
| `nullable` | `Option<T>` + `#[garde(skip)]` (when present) | A nullable optional field. |
| `string` | (field type `String`) | Type-enforced. |
| `integer` | (field type `i64` / `i32`) | Type-enforced. |
| `boolean` | (field type `bool`) | Type-enforced. |
| `min:N` (string/array) | `#[garde(length(min = N))]` | |
| `max:N` (string/array) | `#[garde(length(max = N))]` | |
| `min:N` (numeric) | `#[garde(range(min = N))]` | |
| `max:N` (numeric) | `#[garde(range(max = N))]` | |
| `between:A,B` | `#[garde(length(min = A, max = B))]` or `range(min = A, max = B)` | |
| `email` | `#[garde(email)]` | |
| `url` | `#[garde(url)]` | |
| `uuid` | `#[garde(pattern(r"^[0-9a-fA-F-]{36}$"))]` (or field type `uuid::Uuid`) | |
| `ip` / `ipv4` / `ipv6` | `#[garde(ip)]` | |
| `regex:/pattern/` | `#[garde(pattern(r"pattern"))]` | Rust regex syntax. |
| `in:foo,bar,baz` | `#[garde(custom(in_set))]` + a small helper | Or use a Rust `enum`. |
| `confirmed` (e.g. `password`) | Two fields + `#[garde(custom(...))]` checking equality | |
| `exists:posts,id` | `#[garde(custom(...))]` querying the DB | Or check in the handler. |
| `unique:users,email` | `#[garde(custom(...))]` querying the DB | Often cleaner as a handler-side check. |
| `same:other_field` | `#[garde(custom(...))]` with cross-field access | Garde gives you the full struct in context. |

**Worked example** — Laravel:

```php
public function rules() {
    return [
        'title'     => 'required|string|min:1|max:200',
        'body'      => 'required|string',
        'published' => 'nullable|boolean',
    ];
}
```

Anvilforge:

```rust
#[derive(Debug, Deserialize, Validate, FormRequest)]
pub struct StorePostRequest {
    #[garde(length(min = 1, max = 200))]
    pub title: String,

    #[garde(length(min = 1))]
    pub body: String,

    #[garde(skip)]
    pub published: Option<bool>,
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
