//! Web routes — HTML responses.

use anvil::prelude::*;

use crate::app::models::Post;

pub fn register(r: Router) -> Router {
    r.get("/", root)
        .get("/posts", list_posts)
        .get("/posts/:id", show_post)
        .get("/health", health)
}

async fn root() -> Result<ViewResponse> {
    Ok(ViewResponse::new(
        "<!DOCTYPE html><html><head><title>Anvil POC</title></head><body><h1>Anvil POC blog</h1><p><a href=\"/posts\">posts</a></p></body></html>",
    ))
}

async fn health() -> &'static str {
    "ok"
}

async fn list_posts(State(c): State<Container>) -> Result<ViewResponse> {
    let posts = Post::query().get(c.pool()).await?;
    let mut html = String::from("<h1>posts</h1><ul>");
    for p in &posts {
        html.push_str(&format!(
            "<li><a href=\"/posts/{}\">{}</a></li>",
            p.id,
            html_escape(&p.title)
        ));
    }
    html.push_str("</ul>");
    Ok(ViewResponse::new(html))
}

async fn show_post(State(c): State<Container>, Path(id): Path<i64>) -> Result<ViewResponse> {
    let post = Post::find(c.pool(), id).await?.ok_or(Error::NotFound)?;
    let html = format!(
        "<h1>{}</h1><div>{}</div>",
        html_escape(&post.title),
        html_escape(&post.body)
    );
    Ok(ViewResponse::new(html))
}

fn html_escape(s: &str) -> String {
    forge::escape::html(s)
}
