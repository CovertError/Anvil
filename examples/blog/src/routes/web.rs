//! Web routes — HTML responses.

use anvilforge::prelude::*;

use crate::app::models::Post;

pub fn register(r: Router) -> Router {
    r.get("/", root)
        .get("/posts", list_posts)
        .get("/posts/:id", show_post)
        .get("/spark-demo", spark_demo)
        .get("/health", health)
}

async fn spark_demo() -> Result<ViewResponse> {
    // `spark::template::render_source` does the runtime lowering + MiniJinja
    // render in one shot, with `spark_mount` / `spark_scripts` already
    // registered on the Environment.
    let source = r#"<!DOCTYPE html>
<html><head><meta charset="utf-8"><title>Spark Counter Demo — Anvilforge</title></head>
<body>
<header><h1>Spark Counter Demo</h1></header>
<main>
    <p>Click the buttons — watch the count update without a page reload. The
       JS runtime is loaded by <code>@sparkScripts</code> at the bottom.</p>
    @spark("counter", { initial: 0, label: "Clicks" })
</main>
@sparkScripts
</body></html>"#;
    let rendered = spark::template::render_source(source, &serde_json::json!({}))
        .map_err(|e| Error::Internal(format!("template: {e}")))?;
    Ok(ViewResponse::new(rendered))
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
