//! API routes — JSON responses.

use anvilforge::prelude::*;

use crate::app::models::{Author, Post};
use crate::app::requests::StorePostRequest;

pub fn register(r: Router) -> Router {
    r.get("/authors", list_authors)
        .get("/posts", list_posts)
        .post("/posts", create_post)
}

async fn list_authors(State(c): State<Container>) -> Result<Json<Vec<Author>>> {
    Ok(Json(Author::all(c.pool()).await?))
}

async fn list_posts(State(c): State<Container>) -> Result<Json<Vec<Post>>> {
    Ok(Json(Post::all(c.pool()).await?))
}

async fn create_post(State(c): State<Container>, payload: StorePostRequest) -> Result<Json<Post>> {
    // `Post::create(pool, attrs)` is the Eloquent-shaped helper:
    // construct the struct (id defaults to 0, the DB assigns it on INSERT),
    // and the derive-generated method does the INSERT + RETURNING.
    // Compare to Laravel's `Post::create($request->validated())`.
    let post = Post::create(
        c.pool(),
        Post {
            id: 0,
            author_id: payload.author_id,
            title: payload.title,
            body: payload.body,
            published: payload.published.unwrap_or(false),
            created_at: None,
            updated_at: None,
        },
    )
    .await?;
    Ok(Json(post))
}
