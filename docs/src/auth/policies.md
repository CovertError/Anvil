# Policies

Policies are Anvilforge's authorization mechanism. Each is a unit struct implementing `Policy<User, Subject>`:

```rust
use anvilforge::auth::Policy;

pub struct PostPolicy;

impl Policy<User, Post> for PostPolicy {
    fn check(user: &User, ability: &str, post: &Post) -> bool {
        match ability {
            "view"   => true,                          // anyone can view
            "update" => user.id == post.author_id,
            "delete" => user.id == post.author_id || user.is_admin,
            _ => false,
        }
    }
}
```

## Authorizing

```rust
use anvilforge::auth::authorize;

async fn update(
    State(c): State<Container>,
    Auth(user): Auth<User>,
    Path(id): Path<i64>,
    payload: UpdatePostRequest,
) -> Result<Redirect> {
    let post = Post::find(c.pool(), id).await?.ok_or(Error::NotFound)?;
    authorize::<PostPolicy, _, _>(&user, "update", &post)?;
    // ... do the update
    Ok(Redirect::to(format!("/posts/{}", id)))
}
```

`authorize::<P, _, _>(user, ability, subject)` returns `Result<(), Error>` — the policy either passes through or you get a 403.

## In Forge templates

```forge
@can("update", post)
    <a href="/posts/{{ post.id }}/edit">Edit</a>
@endcan
```

The `@can` directive calls `Policy::check` at render time. (Currently the policy is resolved by convention from the type — full template-side dispatch ships in v0.2.)

[Next: smith make:auth →](scaffold.md)
