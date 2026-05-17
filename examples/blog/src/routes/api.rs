//! API routes — JSON responses.

use anvil::prelude::*;

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

async fn create_post(
    State(c): State<Container>,
    payload: StorePostRequest,
) -> Result<Json<Post>> {
    let row: (i64,) = sqlx::query_as(
        "INSERT INTO posts (author_id, title, body, published) VALUES ($1, $2, $3, $4) RETURNING id",
    )
    .bind(payload.author_id)
    .bind(&payload.title)
    .bind(&payload.body)
    .bind(payload.published.unwrap_or(false))
    .fetch_one(c.pool())
    .await
    .map_err(Error::Database)?;
    let post = Post::find(c.pool(), row.0).await?.ok_or(Error::NotFound)?;
    Ok(Json(post))
}
