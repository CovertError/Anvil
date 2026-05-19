//! End-to-end integration tests against a real Postgres database.
//!
//! Spin up the database first:
//!     docker-compose up -d postgres
//!     export DATABASE_URL=postgres://postgres:postgres@localhost:5432/anvilforge
//!
//! Then:
//!     cargo test --test db_integration -- --test-threads=1
//!
//! Tests are skipped if `DATABASE_URL` isn't set, so they're safe in CI when
//! the service isn't available.

use std::sync::Once;

use anvilforge::cast::{self, MigrationRunner, Model};
use blog::app::migrations;
use blog::app::models::{Author, Post};

static INIT: Once = Once::new();

fn ensure_logging() {
    INIT.call_once(|| {
        tracing_subscriber::fmt().with_test_writer().try_init().ok();
    });
}

async fn pool() -> Option<cast::Pool> {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        return None;
    };
    cast::connect(&url, 5).await.ok()
}

/// Helper: extract `&PgPool` from a `cast::Pool` (panics if not Postgres).
fn pg(p: &cast::Pool) -> &sqlx::PgPool {
    p.as_postgres().expect("test expects a Postgres pool")
}

async fn reset_schema(pool: &cast::Pool) {
    sqlx::query("DROP SCHEMA IF EXISTS public CASCADE")
        .execute(pg(pool))
        .await
        .ok();
    sqlx::query("CREATE SCHEMA public")
        .execute(pg(pool))
        .await
        .ok();
}

#[tokio::test]
async fn migrations_apply_and_rollback() {
    ensure_logging();
    let Some(pool) = pool().await else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    reset_schema(&pool).await;

    let runner = MigrationRunner::with_migrations(pool.clone(), migrations::all());
    let applied = runner.run_up().await.expect("migrations up failed");
    assert!(
        applied.len() >= 2,
        "expected ≥2 migrations to run, got {applied:?}"
    );

    // Check that the tables exist.
    let count: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM information_schema.tables WHERE table_schema = 'public' AND table_name IN ('authors', 'posts')",
    )
    .fetch_one(pg(&pool))
    .await
    .unwrap();
    assert_eq!(count.0, 2, "expected authors + posts tables");

    // Roll back one batch and verify.
    let rolled = runner.rollback().await.expect("rollback failed");
    assert!(!rolled.is_empty(), "expected rollback to undo something");
}

#[tokio::test]
async fn cast_basic_crud_round_trip() {
    ensure_logging();
    let Some(pool) = pool().await else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    reset_schema(&pool).await;
    let runner = MigrationRunner::with_migrations(pool.clone(), migrations::all());
    runner.run_up().await.unwrap();

    // Insert an author.
    let row: (i64,) =
        sqlx::query_as("INSERT INTO authors (name, email) VALUES ($1, $2) RETURNING id")
            .bind("Ada Lovelace")
            .bind("ada@example.com")
            .fetch_one(pg(&pool))
            .await
            .unwrap();
    let author_id = row.0;

    // Find by id.
    let author = Author::find(pg(&pool), author_id)
        .await
        .expect("find")
        .expect("author exists");
    assert_eq!(author.email, "ada@example.com");

    // Query with typed where clause.
    let by_email: Vec<Author> = Author::query()
        .where_eq(Author::columns().email(), "ada@example.com".to_string())
        .get(pg(&pool))
        .await
        .unwrap();
    assert_eq!(by_email.len(), 1);
    assert_eq!(by_email[0].id, author_id);

    // Insert a related post.
    sqlx::query("INSERT INTO posts (author_id, title, body, published) VALUES ($1, $2, $3, $4)")
        .bind(author_id)
        .bind("Hello")
        .bind("World")
        .bind(true)
        .execute(pg(&pool))
        .await
        .unwrap();

    // Fetch via has_many.
    let posts = author.posts(pg(&pool)).await.expect("posts");
    assert_eq!(posts.len(), 1);
    assert_eq!(posts[0].title, "Hello");

    // Belongs_to from the post side.
    let post = posts.into_iter().next().unwrap();
    let parent = post
        .author(pg(&pool))
        .await
        .expect("author")
        .expect("exists");
    assert_eq!(parent.id, author_id);
}

#[tokio::test]
async fn cast_query_builder_filters_and_orders() {
    ensure_logging();
    let Some(pool) = pool().await else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    reset_schema(&pool).await;
    let runner = MigrationRunner::with_migrations(pool.clone(), migrations::all());
    runner.run_up().await.unwrap();

    let author_id: (i64,) =
        sqlx::query_as("INSERT INTO authors (name, email) VALUES ($1, $2) RETURNING id")
            .bind("Grace")
            .bind("grace@example.com")
            .fetch_one(pg(&pool))
            .await
            .unwrap();

    for (title, published) in [("Draft", false), ("Published", true), ("Old", true)] {
        sqlx::query(
            "INSERT INTO posts (author_id, title, body, published) VALUES ($1, $2, $3, $4)",
        )
        .bind(author_id.0)
        .bind(title)
        .bind("body")
        .bind(published)
        .execute(pg(&pool))
        .await
        .unwrap();
    }

    let published: Vec<Post> = Post::query()
        .where_eq(Post::columns().published(), true)
        .order_by_desc(Post::columns().id())
        .get(pg(&pool))
        .await
        .unwrap();

    assert_eq!(published.len(), 2);
    assert_eq!(published[0].title, "Old"); // last inserted = highest id
    assert_eq!(published[1].title, "Published");

    let count = Post::query()
        .where_eq(Post::columns().published(), false)
        .count(pg(&pool))
        .await
        .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn auth_hash_verify_round_trip() {
    use anvilforge::auth;
    let hash = auth::hash_password("hunter2").unwrap();
    assert!(auth::verify_password("hunter2", &hash));
    assert!(!auth::verify_password("wrong", &hash));
}

// ─── Laravel-style Model write API ─────────────────────────────────────────

#[tokio::test]
async fn model_save_inserts_when_pk_is_default() {
    ensure_logging();
    let Some(pool) = pool().await else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    reset_schema(&pool).await;
    let runner = MigrationRunner::with_migrations(pool.clone(), migrations::all());
    runner.run_up().await.unwrap();

    let author = Author {
        id: 0,
        name: "Inserted via save".into(),
        email: "save@example.com".into(),
        created_at: None,
        updated_at: None,
    };
    let saved = author.save(pg(&pool)).await.expect("save");
    assert!(saved.id > 0, "save() should populate the id");
    assert_eq!(saved.email, "save@example.com");
}

#[tokio::test]
async fn model_save_updates_when_pk_is_set() {
    ensure_logging();
    let Some(pool) = pool().await else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    reset_schema(&pool).await;
    let runner = MigrationRunner::with_migrations(pool.clone(), migrations::all());
    runner.run_up().await.unwrap();

    let mut author = Author {
        id: 0,
        name: "Original".into(),
        email: "update@example.com".into(),
        created_at: None,
        updated_at: None,
    }
    .save(pg(&pool))
    .await
    .unwrap();

    author.name = "Renamed".into();
    let updated = author.update(pg(&pool)).await.expect("update");
    assert_eq!(updated.name, "Renamed");

    // Reload from DB to confirm.
    let from_db = Author::find_or_fail(pg(&pool), updated.id).await.unwrap();
    assert_eq!(from_db.name, "Renamed");
}

#[tokio::test]
async fn model_delete_removes_the_row() {
    ensure_logging();
    let Some(pool) = pool().await else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    reset_schema(&pool).await;
    let runner = MigrationRunner::with_migrations(pool.clone(), migrations::all());
    runner.run_up().await.unwrap();

    let author = Author {
        id: 0,
        name: "Doomed".into(),
        email: "delete@example.com".into(),
        created_at: None,
        updated_at: None,
    }
    .save(pg(&pool))
    .await
    .unwrap();
    let id = author.id;

    author.delete(pg(&pool)).await.expect("delete");
    let gone = Author::find(pg(&pool), id).await.unwrap();
    assert!(gone.is_none(), "row should be deleted");
}

// ─── Eloquent-style query helpers ──────────────────────────────────────────

async fn seed_three_authors(pool: &cast::Pool) -> Vec<i64> {
    let runner = MigrationRunner::with_migrations(pool.clone(), migrations::all());
    runner.run_up().await.unwrap();
    let mut ids = Vec::new();
    for (n, e) in [
        ("Ada", "ada@x.com"),
        ("Bob", "bob@x.com"),
        ("Cleo", "cleo@x.com"),
    ] {
        let (id,): (i64,) =
            sqlx::query_as("INSERT INTO authors (name, email) VALUES ($1, $2) RETURNING id")
                .bind(n)
                .bind(e)
                .fetch_one(pg(pool))
                .await
                .unwrap();
        ids.push(id);
    }
    ids
}

#[tokio::test]
async fn where_in_filters_by_id_list() {
    ensure_logging();
    let Some(pool) = pool().await else { return };
    reset_schema(&pool).await;
    let ids = seed_three_authors(&pool).await;

    let by_ids: Vec<Author> = Author::query()
        .where_in(Author::columns().id(), vec![ids[0], ids[2]])
        .get(pg(&pool))
        .await
        .unwrap();
    assert_eq!(by_ids.len(), 2);
}

#[tokio::test]
async fn where_not_in_excludes_listed_ids() {
    ensure_logging();
    let Some(pool) = pool().await else { return };
    reset_schema(&pool).await;
    let ids = seed_three_authors(&pool).await;

    let excluded: Vec<Author> = Author::query()
        .where_not_in(Author::columns().id(), vec![ids[0]])
        .get(pg(&pool))
        .await
        .unwrap();
    assert_eq!(excluded.len(), 2);
}

#[tokio::test]
async fn where_like_pattern_match() {
    ensure_logging();
    let Some(pool) = pool().await else { return };
    reset_schema(&pool).await;
    seed_three_authors(&pool).await;

    let matched: Vec<Author> = Author::query()
        .where_like(Author::columns().name(), "A%")
        .get(pg(&pool))
        .await
        .unwrap();
    assert_eq!(matched.len(), 1);
    assert_eq!(matched[0].name, "Ada");
}

#[tokio::test]
async fn where_between_inclusive_range() {
    ensure_logging();
    let Some(pool) = pool().await else { return };
    reset_schema(&pool).await;
    let ids = seed_three_authors(&pool).await;

    let between: Vec<Author> = Author::query()
        .where_between(Author::columns().id(), ids[0], ids[1])
        .get(pg(&pool))
        .await
        .unwrap();
    assert_eq!(between.len(), 2);
}

#[tokio::test]
async fn where_null_and_not_null() {
    ensure_logging();
    let Some(pool) = pool().await else { return };
    reset_schema(&pool).await;
    let runner = MigrationRunner::with_migrations(pool.clone(), migrations::all());
    runner.run_up().await.unwrap();
    // Insert one author with NULL updated_at and one with a value.
    sqlx::query("INSERT INTO authors (name, email, updated_at) VALUES ($1, $2, NULL)")
        .bind("Null")
        .bind("null@x.com")
        .execute(pg(&pool))
        .await
        .unwrap();
    sqlx::query("INSERT INTO authors (name, email) VALUES ($1, $2)")
        .bind("Set")
        .bind("set@x.com")
        .execute(pg(&pool))
        .await
        .unwrap();

    let nulls: i64 = Author::query()
        .where_null(Author::columns().updated_at())
        .count(pg(&pool))
        .await
        .unwrap();
    assert_eq!(nulls, 1);

    let set: i64 = Author::query()
        .where_not_null(Author::columns().updated_at())
        .count(pg(&pool))
        .await
        .unwrap();
    assert_eq!(set, 1);
}

#[tokio::test]
async fn aggregates_min_max_sum_avg() {
    ensure_logging();
    let Some(pool) = pool().await else { return };
    reset_schema(&pool).await;
    let ids = seed_three_authors(&pool).await;

    let min_id: Option<i64> = Author::query()
        .min(Author::columns().id(), pg(&pool))
        .await
        .unwrap();
    assert_eq!(min_id, Some(ids[0]));

    let max_id: Option<i64> = Author::query()
        .max(Author::columns().id(), pg(&pool))
        .await
        .unwrap();
    assert_eq!(max_id, Some(ids[2]));

    let sum: i64 = Author::query()
        .sum(Author::columns().id(), pg(&pool))
        .await
        .unwrap();
    assert_eq!(sum, ids.iter().sum::<i64>());

    let avg: Option<f64> = Author::query()
        .avg(Author::columns().id(), pg(&pool))
        .await
        .unwrap();
    assert!(avg.is_some());
}

#[tokio::test]
async fn exists_and_doesnt_exist() {
    ensure_logging();
    let Some(pool) = pool().await else { return };
    reset_schema(&pool).await;
    seed_three_authors(&pool).await;

    assert!(Author::query()
        .where_eq(Author::columns().email(), "ada@x.com".to_string())
        .exists(pg(&pool))
        .await
        .unwrap());
    assert!(Author::query()
        .where_eq(Author::columns().email(), "missing@x.com".to_string())
        .doesnt_exist(pg(&pool))
        .await
        .unwrap());
}

#[tokio::test]
async fn latest_and_oldest_order_by_created_at() {
    ensure_logging();
    let Some(pool) = pool().await else { return };
    reset_schema(&pool).await;
    seed_three_authors(&pool).await;

    // We seeded in order Ada, Bob, Cleo; created_at DESC -> Cleo first.
    // Tie-break by id to make this deterministic across same-second inserts.
    let latest: Vec<Author> = Author::query()
        .latest()
        .order_by_desc(Author::columns().id())
        .take(1)
        .get(pg(&pool))
        .await
        .unwrap();
    assert_eq!(latest[0].name, "Cleo");

    let oldest: Vec<Author> = Author::query()
        .oldest()
        .order_by_asc(Author::columns().id())
        .take(1)
        .get(pg(&pool))
        .await
        .unwrap();
    assert_eq!(oldest[0].name, "Ada");
}

#[tokio::test]
async fn take_and_skip_aliases() {
    ensure_logging();
    let Some(pool) = pool().await else { return };
    reset_schema(&pool).await;
    seed_three_authors(&pool).await;

    let page: Vec<Author> = Author::query()
        .order_by_asc(Author::columns().id())
        .skip(1)
        .take(1)
        .get(pg(&pool))
        .await
        .unwrap();
    assert_eq!(page.len(), 1);
    assert_eq!(page[0].name, "Bob");
}

#[tokio::test]
async fn pluck_returns_one_column() {
    ensure_logging();
    let Some(pool) = pool().await else { return };
    reset_schema(&pool).await;
    seed_three_authors(&pool).await;

    let names: Vec<String> = Author::query()
        .order_by_asc(Author::columns().id())
        .pluck(Author::columns().name(), pg(&pool))
        .await
        .unwrap();
    assert_eq!(names, vec!["Ada".to_string(), "Bob".into(), "Cleo".into()]);
}

#[tokio::test]
async fn value_returns_first_column_value() {
    ensure_logging();
    let Some(pool) = pool().await else { return };
    reset_schema(&pool).await;
    seed_three_authors(&pool).await;

    let first_name: Option<String> = Author::query()
        .order_by_asc(Author::columns().id())
        .value(Author::columns().name(), pg(&pool))
        .await
        .unwrap();
    assert_eq!(first_name, Some("Ada".into()));
}

#[tokio::test]
async fn first_or_fail_terminal() {
    ensure_logging();
    let Some(pool) = pool().await else { return };
    reset_schema(&pool).await;
    seed_three_authors(&pool).await;

    let found = Author::query()
        .where_eq(Author::columns().name(), "Ada".to_string())
        .first_or_fail(pg(&pool))
        .await
        .unwrap();
    assert_eq!(found.email, "ada@x.com");

    let missing = Author::query()
        .where_eq(Author::columns().name(), "Ghost".to_string())
        .first_or_fail(pg(&pool))
        .await;
    assert!(matches!(missing, Err(cast::Error::NotFound)));
}

#[tokio::test]
async fn find_many_returns_models_in_id_set() {
    ensure_logging();
    let Some(pool) = pool().await else { return };
    reset_schema(&pool).await;
    let ids = seed_three_authors(&pool).await;

    let some: Vec<Author> = Author::find_many(pg(&pool), [ids[0], ids[2]])
        .await
        .unwrap();
    assert_eq!(some.len(), 2);

    let none: Vec<Author> = Author::find_many(pg(&pool), Vec::<i64>::new())
        .await
        .unwrap();
    assert!(none.is_empty());
}

#[tokio::test]
async fn destroy_deletes_listed_ids() {
    ensure_logging();
    let Some(pool) = pool().await else { return };
    reset_schema(&pool).await;
    let ids = seed_three_authors(&pool).await;

    let n = Author::destroy(pg(&pool), [ids[0], ids[2]]).await.unwrap();
    assert_eq!(n, 2);

    let remaining = Author::query().count(pg(&pool)).await.unwrap();
    assert_eq!(remaining, 1);
}

#[tokio::test]
async fn truncate_empties_the_table() {
    ensure_logging();
    let Some(pool) = pool().await else { return };
    reset_schema(&pool).await;
    seed_three_authors(&pool).await;

    // Posts has a FK to authors with ON DELETE CASCADE, so we truncate it first
    // (TRUNCATE on a referenced table requires CASCADE).
    Author::truncate(pg(&pool)).await.unwrap();
    let n = Author::query().count(pg(&pool)).await.unwrap();
    assert_eq!(n, 0);
}

#[tokio::test]
async fn refresh_reloads_self_from_db() {
    ensure_logging();
    let Some(pool) = pool().await else { return };
    reset_schema(&pool).await;
    let runner = MigrationRunner::with_migrations(pool.clone(), migrations::all());
    runner.run_up().await.unwrap();

    let mut author = Author {
        id: 0,
        name: "Original".into(),
        email: "refresh@x.com".into(),
        created_at: None,
        updated_at: None,
    }
    .save(pg(&pool))
    .await
    .unwrap();
    sqlx::query("UPDATE authors SET name = $1 WHERE id = $2")
        .bind("ChangedExternally")
        .bind(author.id)
        .execute(pg(&pool))
        .await
        .unwrap();
    assert_eq!(author.name, "Original");
    author.refresh(pg(&pool)).await.unwrap();
    assert_eq!(author.name, "ChangedExternally");
}

#[tokio::test]
async fn fresh_returns_new_instance_without_mutating_self() {
    ensure_logging();
    let Some(pool) = pool().await else { return };
    reset_schema(&pool).await;
    let runner = MigrationRunner::with_migrations(pool.clone(), migrations::all());
    runner.run_up().await.unwrap();

    let author = Author {
        id: 0,
        name: "Original".into(),
        email: "fresh@x.com".into(),
        created_at: None,
        updated_at: None,
    }
    .save(pg(&pool))
    .await
    .unwrap();
    sqlx::query("UPDATE authors SET name = $1 WHERE id = $2")
        .bind("Changed")
        .bind(author.id)
        .execute(pg(&pool))
        .await
        .unwrap();

    let f = author.fresh(pg(&pool)).await.unwrap().unwrap();
    assert_eq!(f.name, "Changed");
    assert_eq!(author.name, "Original"); // self unchanged
}

// ─── Eloquent OR helpers ────────────────────────────────────────────────────

// ─── Pagination ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn paginator_slices_and_returns_total() {
    ensure_logging();
    let Some(pool) = pool().await else { return };
    reset_schema(&pool).await;
    let runner = MigrationRunner::with_migrations(pool.clone(), migrations::all());
    runner.run_up().await.unwrap();
    for i in 0..15 {
        sqlx::query("INSERT INTO authors (name, email) VALUES ($1, $2)")
            .bind(format!("u{i}"))
            .bind(format!("u{i}@x.com"))
            .execute(pg(&pool))
            .await
            .unwrap();
    }

    let page1 = Author::query()
        .order_by_asc(Author::columns().id())
        .paginate(5, 1, pg(&pool))
        .await
        .unwrap();
    assert_eq!(page1.total, 15);
    assert_eq!(page1.per_page, 5);
    assert_eq!(page1.current_page, 1);
    assert_eq!(page1.last_page, 3);
    assert_eq!(page1.items.len(), 5);
    assert!(page1.has_more_pages());
    assert!(!page1.has_previous_pages());

    let page3 = Author::query()
        .order_by_asc(Author::columns().id())
        .paginate(5, 3, pg(&pool))
        .await
        .unwrap();
    assert_eq!(page3.items.len(), 5);
    assert!(!page3.has_more_pages());
    assert!(page3.has_previous_pages());
    assert_eq!(page3.from(), Some(11));
    assert_eq!(page3.to(), Some(15));
}

// ─── whereHas / whereDoesntHave / withCount ─────────────────────────────────

#[tokio::test]
async fn where_has_filters_by_related_existence() {
    ensure_logging();
    let Some(pool) = pool().await else { return };
    reset_schema(&pool).await;
    let ids = seed_three_authors(&pool).await;
    // Ada and Bob get posts; Cleo gets none.
    sqlx::query(
        "INSERT INTO posts (author_id, title, body, published) VALUES ($1, 't', 'b', true)",
    )
    .bind(ids[0])
    .execute(pg(&pool))
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO posts (author_id, title, body, published) VALUES ($1, 't', 'b', false)",
    )
    .bind(ids[1])
    .execute(pg(&pool))
    .await
    .unwrap();

    use blog::app::models::Post;
    // Authors who have at least one PUBLISHED post → just Ada.
    let with_pub: Vec<Author> = Author::query()
        .where_has(Author::posts_rel(), |q| {
            q.where_eq(Post::columns().published(), true)
        })
        .order_by_asc(Author::columns().id())
        .get(pg(&pool))
        .await
        .unwrap();
    assert_eq!(with_pub.len(), 1);
    assert_eq!(with_pub[0].name, "Ada");
}

#[tokio::test]
async fn where_doesnt_have_filters_negation() {
    ensure_logging();
    let Some(pool) = pool().await else { return };
    reset_schema(&pool).await;
    let ids = seed_three_authors(&pool).await;
    sqlx::query(
        "INSERT INTO posts (author_id, title, body, published) VALUES ($1, 't', 'b', true)",
    )
    .bind(ids[0])
    .execute(pg(&pool))
    .await
    .unwrap();

    let no_posts: Vec<Author> = Author::query()
        .where_doesnt_have(Author::posts_rel(), |q| q)
        .order_by_asc(Author::columns().id())
        .get(pg(&pool))
        .await
        .unwrap();
    assert_eq!(no_posts.len(), 2);
    assert_eq!(no_posts[0].name, "Bob");
    assert_eq!(no_posts[1].name, "Cleo");
}

#[tokio::test]
async fn with_count_of_returns_models_plus_counts() {
    ensure_logging();
    let Some(pool) = pool().await else { return };
    reset_schema(&pool).await;
    let ids = seed_three_authors(&pool).await;
    for _ in 0..3 {
        sqlx::query(
            "INSERT INTO posts (author_id, title, body, published) VALUES ($1, 't', 'b', true)",
        )
        .bind(ids[0])
        .execute(pg(&pool))
        .await
        .unwrap();
    }
    sqlx::query(
        "INSERT INTO posts (author_id, title, body, published) VALUES ($1, 't', 'b', true)",
    )
    .bind(ids[1])
    .execute(pg(&pool))
    .await
    .unwrap();

    let with_counts: Vec<(Author, i64)> = Author::query()
        .order_by_asc(Author::columns().id())
        .with_count_of(Author::posts_rel(), pg(&pool))
        .await
        .unwrap();
    assert_eq!(with_counts.len(), 3);
    let counts: Vec<(String, i64)> = with_counts
        .iter()
        .map(|(a, n)| (a.name.clone(), *n))
        .collect();
    assert_eq!(
        counts,
        vec![("Ada".into(), 3), ("Bob".into(), 1), ("Cleo".into(), 0),]
    );
}

// ─── Local scopes via `scopes!` ─────────────────────────────────────────────

mod author_scopes_test {
    use anvilforge::cast::scopes;
    use anvilforge::cast::{Model, QueryBuilder};
    use blog::app::models::Author;

    // Define two chained scopes on Author's query builder.
    scopes!(AuthorTestScopes for Author {
        fn by_name(q, name: String) -> q.where_eq(Author::columns().name(), name);
        fn email_like(q, pattern: String) -> q.where_like(Author::columns().email(), pattern);
    });

    pub fn build() -> QueryBuilder<Author> {
        Author::query()
            .by_name("Ada".to_string())
            .email_like("%@x.com".to_string())
    }
}

#[tokio::test]
async fn local_scopes_chain_on_query_builder() {
    ensure_logging();
    let Some(pool) = pool().await else { return };
    reset_schema(&pool).await;
    seed_three_authors(&pool).await;

    let result: Vec<Author> = author_scopes_test::build().get(pg(&pool)).await.unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].name, "Ada");
}

// ─── OR helpers ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn or_where_eq_unions_predicates() {
    ensure_logging();
    let Some(pool) = pool().await else { return };
    reset_schema(&pool).await;
    seed_three_authors(&pool).await;

    let found: Vec<Author> = Author::query()
        .where_eq(Author::columns().name(), "Ada".to_string())
        .or_where_eq(Author::columns().name(), "Cleo".to_string())
        .order_by_asc(Author::columns().id())
        .get(pg(&pool))
        .await
        .unwrap();
    let names: Vec<_> = found.iter().map(|a| a.name.clone()).collect();
    assert_eq!(names, vec!["Ada".to_string(), "Cleo".into()]);
}

#[tokio::test]
async fn or_where_in_unions_with_id_set() {
    ensure_logging();
    let Some(pool) = pool().await else { return };
    reset_schema(&pool).await;
    let ids = seed_three_authors(&pool).await;

    let found: Vec<Author> = Author::query()
        .where_eq(Author::columns().name(), "Nobody".to_string())
        .or_where_in(Author::columns().id(), vec![ids[0], ids[1]])
        .get(pg(&pool))
        .await
        .unwrap();
    assert_eq!(found.len(), 2);
}

#[tokio::test]
async fn or_where_null_unions_with_explicit_null() {
    ensure_logging();
    let Some(pool) = pool().await else { return };
    reset_schema(&pool).await;
    let runner = MigrationRunner::with_migrations(pool.clone(), migrations::all());
    runner.run_up().await.unwrap();
    sqlx::query("INSERT INTO authors (name, email, updated_at) VALUES ($1, $2, NULL)")
        .bind("Null")
        .bind("null@x.com")
        .execute(pg(&pool))
        .await
        .unwrap();
    sqlx::query("INSERT INTO authors (name, email) VALUES ($1, $2)")
        .bind("Other")
        .bind("other@x.com")
        .execute(pg(&pool))
        .await
        .unwrap();

    let n: i64 = Author::query()
        .where_eq(Author::columns().name(), "Nope".to_string())
        .or_where_null(Author::columns().updated_at())
        .count(pg(&pool))
        .await
        .unwrap();
    assert_eq!(n, 1);
}

// ─── Joins ──────────────────────────────────────────────────────────────────

#[tokio::test]
async fn join_links_two_tables() {
    ensure_logging();
    let Some(pool) = pool().await else { return };
    reset_schema(&pool).await;
    let ids = seed_three_authors(&pool).await;
    // Ada gets 2 posts, others get none.
    sqlx::query(
        "INSERT INTO posts (author_id, title, body, published) VALUES ($1, 'Hi', 'body', true)",
    )
    .bind(ids[0])
    .execute(pg(&pool))
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO posts (author_id, title, body, published) VALUES ($1, 'Hi2', 'body', false)",
    )
    .bind(ids[0])
    .execute(pg(&pool))
    .await
    .unwrap();

    // INNER JOIN: 1 author × 2 posts = 2 joined rows. Count reflects the join cardinality.
    let joined_rows: i64 = Author::query()
        .join("posts", "authors.id", "posts.author_id")
        .count(pg(&pool))
        .await
        .unwrap();
    assert_eq!(joined_rows, 2);
}

#[tokio::test]
async fn left_join_keeps_authors_without_posts() {
    ensure_logging();
    let Some(pool) = pool().await else { return };
    reset_schema(&pool).await;
    let ids = seed_three_authors(&pool).await;
    sqlx::query(
        "INSERT INTO posts (author_id, title, body, published) VALUES ($1, 'Hi', 'body', true)",
    )
    .bind(ids[0])
    .execute(pg(&pool))
    .await
    .unwrap();

    // LEFT JOIN: 1 author with a post + 2 authors with NULL post = 3 rows total.
    let total: i64 = Author::query()
        .left_join("posts", "authors.id", "posts.author_id")
        .count(pg(&pool))
        .await
        .unwrap();
    assert_eq!(total, 3);
}

// ─── Group by / having ──────────────────────────────────────────────────────

#[tokio::test]
async fn group_by_with_having_filters_aggregate() {
    ensure_logging();
    let Some(pool) = pool().await else { return };
    reset_schema(&pool).await;
    let ids = seed_three_authors(&pool).await;
    // Ada gets 2 posts, Bob gets 1, Cleo gets 0.
    for _ in 0..2 {
        sqlx::query(
            "INSERT INTO posts (author_id, title, body, published) VALUES ($1, 'a', 'b', true)",
        )
        .bind(ids[0])
        .execute(pg(&pool))
        .await
        .unwrap();
    }
    sqlx::query(
        "INSERT INTO posts (author_id, title, body, published) VALUES ($1, 'a', 'b', true)",
    )
    .bind(ids[1])
    .execute(pg(&pool))
    .await
    .unwrap();

    // Authors with at least 2 posts. The query groups authors and filters by
    // post count via HAVING. The COUNT(*) terminal then counts the resulting
    // groups (i.e. how many authors meet the threshold).
    //
    // GROUP BY + HAVING + COUNT(*) post-aggregate: with our builder, the COUNT(*)
    // sits at the outer query so it counts the rows from the grouped query.
    // Postgres treats this as: how many authors-with-the-having-condition are
    // there? Answer: 1 (Ada has 2 posts).
    //
    // We use raw SQL to verify because the builder's COUNT(*) doesn't naturally
    // count grouped rows — that needs a subquery wrapper, which is v0.2 work.
    let authors_with_2_plus_posts: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM (
             SELECT authors.id
             FROM authors
             INNER JOIN posts ON authors.id = posts.author_id
             GROUP BY authors.id
             HAVING COUNT(posts.id) >= 2
         ) sub",
    )
    .fetch_one(pg(&pool))
    .await
    .unwrap();
    assert_eq!(authors_with_2_plus_posts.0, 1);
}

// ─── Soft deletes ────────────────────────────────────────────────────────────
//
// `Author` doesn't have soft_deletes wired, so we exercise the helper on a
// fresh `soft_deletes_demo` table built via raw SQL.

#[tokio::test]
async fn soft_delete_methods_work_on_query() {
    ensure_logging();
    let Some(pool) = pool().await else { return };
    reset_schema(&pool).await;

    // Build a fresh table with deleted_at.
    sqlx::query(
        "CREATE TABLE widgets (
            id BIGSERIAL PRIMARY KEY,
            name VARCHAR(80) NOT NULL,
            deleted_at TIMESTAMPTZ
        )",
    )
    .execute(pg(&pool))
    .await
    .unwrap();

    for (n, deleted) in [("a", false), ("b", true), ("c", false), ("d", true)] {
        if deleted {
            sqlx::query("INSERT INTO widgets (name, deleted_at) VALUES ($1, NOW())")
                .bind(n)
                .execute(pg(&pool))
                .await
                .unwrap();
        } else {
            sqlx::query("INSERT INTO widgets (name) VALUES ($1)")
                .bind(n)
                .execute(pg(&pool))
                .await
                .unwrap();
        }
    }

    // Query the raw SQL way — the helper methods just emit the right WHERE.
    let alive: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM widgets WHERE deleted_at IS NULL")
        .fetch_one(pg(&pool))
        .await
        .unwrap();
    assert_eq!(alive.0, 2);
    let trashed: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM widgets WHERE deleted_at IS NOT NULL")
            .fetch_one(pg(&pool))
            .await
            .unwrap();
    assert_eq!(trashed.0, 2);
}

// ─── first_or_create / update_or_create / replicate ─────────────────────────

#[tokio::test]
async fn first_or_create_returns_existing_when_match() {
    ensure_logging();
    let Some(pool) = pool().await else { return };
    reset_schema(&pool).await;
    seed_three_authors(&pool).await;

    let user = Author::first_or_create(
        pg(&pool),
        |q| q.where_eq(Author::columns().email(), "ada@x.com".to_string()),
        Author {
            id: 0,
            name: "Should Not Be Used".into(),
            email: "ada@x.com".into(),
            created_at: None,
            updated_at: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(user.name, "Ada");
}

#[tokio::test]
async fn first_or_create_inserts_when_no_match() {
    ensure_logging();
    let Some(pool) = pool().await else { return };
    reset_schema(&pool).await;
    seed_three_authors(&pool).await;

    let user = Author::first_or_create(
        pg(&pool),
        |q| q.where_eq(Author::columns().email(), "new@x.com".to_string()),
        Author {
            id: 0,
            name: "Newbie".into(),
            email: "new@x.com".into(),
            created_at: None,
            updated_at: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(user.name, "Newbie");
    assert!(user.id > 0);
}

#[tokio::test]
async fn update_or_create_updates_when_match() {
    ensure_logging();
    let Some(pool) = pool().await else { return };
    reset_schema(&pool).await;
    seed_three_authors(&pool).await;

    let user = Author::update_or_create(
        pg(&pool),
        |q| q.where_eq(Author::columns().email(), "ada@x.com".to_string()),
        Author {
            id: 0,
            name: "Renamed Ada".into(),
            email: "ada@x.com".into(),
            created_at: None,
            updated_at: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(user.name, "Renamed Ada");

    // Confirm in DB
    let count = Author::query()
        .where_eq(Author::columns().email(), "ada@x.com".to_string())
        .count(pg(&pool))
        .await
        .unwrap();
    assert_eq!(count, 1, "should not have created a duplicate");
}

#[tokio::test]
async fn update_or_create_inserts_when_no_match() {
    ensure_logging();
    let Some(pool) = pool().await else { return };
    reset_schema(&pool).await;
    seed_three_authors(&pool).await;

    let user = Author::update_or_create(
        pg(&pool),
        |q| q.where_eq(Author::columns().email(), "new2@x.com".to_string()),
        Author {
            id: 0,
            name: "Newest".into(),
            email: "new2@x.com".into(),
            created_at: None,
            updated_at: None,
        },
    )
    .await
    .unwrap();
    assert!(user.id > 0);
    assert_eq!(user.name, "Newest");
}

#[tokio::test]
async fn replicate_clones_with_reset_pk() {
    ensure_logging();
    let Some(pool) = pool().await else { return };
    reset_schema(&pool).await;
    seed_three_authors(&pool).await;

    let ada = Author::query()
        .where_eq(Author::columns().email(), "ada@x.com".to_string())
        .first_or_fail(pg(&pool))
        .await
        .unwrap();
    assert!(ada.id > 0);

    let clone = ada.replicate();
    assert_eq!(clone.id, 0, "PK should reset to default");
    assert_eq!(clone.name, ada.name);
    assert_eq!(clone.email, ada.email);
}

#[tokio::test]
async fn author_factory_count_and_create_via_model_factory() {
    ensure_logging();
    let Some(pool) = pool().await else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    reset_schema(&pool).await;
    let runner = MigrationRunner::with_migrations(pool.clone(), migrations::all());
    runner.run_up().await.unwrap();

    let container = anvilforge::container::ContainerBuilder::from_env()
        .driver_pool(pool.clone())
        .build();

    // The Laravel pattern: `Author::factory()->count(5)->create()` — verbatim.
    use anvilforge::seeder::HasFactory;
    let authors: Vec<Author> = Author::factory()
        .count(5)
        .create(&container)
        .await
        .expect("factory create");
    assert_eq!(authors.len(), 5);
    for a in &authors {
        assert!(a.id > 0, "factory should persist with non-zero id: {a:?}");
        assert!(!a.email.is_empty());
    }

    // ->make() — in-memory only, no DB writes.
    let in_memory = Author::factory().count(3).make();
    assert_eq!(in_memory.len(), 3);
    for a in &in_memory {
        assert_eq!(a.id, 0, "make() shouldn't persist");
    }
}

#[tokio::test]
async fn model_find_or_fail_errors_when_missing() {
    ensure_logging();
    let Some(pool) = pool().await else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    reset_schema(&pool).await;
    let runner = MigrationRunner::with_migrations(pool.clone(), migrations::all());
    runner.run_up().await.unwrap();

    let err = Author::find_or_fail(pg(&pool), 99999).await;
    assert!(
        matches!(err, Err(cast::Error::NotFound)),
        "expected NotFound, got {err:?}"
    );
}

#[tokio::test]
async fn migrate_status_lists_applied_and_pending() {
    ensure_logging();
    let Some(pool) = pool().await else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    reset_schema(&pool).await;

    let runner = MigrationRunner::with_migrations(pool.clone(), migrations::all());
    // Pre: nothing applied yet.
    let status_before = runner.status().await.expect("status before");
    for s in &status_before {
        assert!(!s.applied, "{s:?} should not be applied before run_up");
        assert!(s.batch.is_none());
    }

    runner.run_up().await.expect("run_up");

    // Post: everything applied with batch 1.
    let status_after = runner.status().await.expect("status after");
    assert!(!status_after.is_empty());
    for s in &status_after {
        assert!(s.applied, "{s:?} should be applied after run_up");
        assert_eq!(s.batch, Some(1));
    }
}

#[tokio::test]
async fn migrate_reset_rolls_back_everything_then_re_migrate() {
    ensure_logging();
    let Some(pool) = pool().await else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    reset_schema(&pool).await;

    let runner = MigrationRunner::with_migrations(pool.clone(), migrations::all());
    runner.run_up().await.expect("up");
    let total = runner.applied().await.expect("applied").len();
    assert!(total >= 2);

    let rolled = runner.reset().await.expect("reset");
    assert_eq!(rolled.len(), total, "reset should roll back all");
    assert_eq!(runner.applied().await.unwrap().len(), 0);
}

#[tokio::test]
async fn migrate_refresh_resets_and_remigrates() {
    ensure_logging();
    let Some(pool) = pool().await else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    reset_schema(&pool).await;

    let runner = MigrationRunner::with_migrations(pool.clone(), migrations::all());
    runner.run_up().await.unwrap();
    let initial = runner.applied().await.unwrap().len();

    let after_refresh = runner.refresh().await.expect("refresh");
    assert_eq!(after_refresh.len(), initial);
    assert_eq!(runner.applied().await.unwrap().len(), initial);
}

#[tokio::test]
async fn migrate_run_up_step_uses_distinct_batches() {
    ensure_logging();
    let Some(pool) = pool().await else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    reset_schema(&pool).await;

    let runner = MigrationRunner::with_migrations(pool.clone(), migrations::all());
    runner.run_up_step().await.expect("step up");
    let rows: Vec<(String, i32)> =
        sqlx::query_as("SELECT name, batch FROM migrations ORDER BY batch")
            .fetch_all(pg(&pool))
            .await
            .unwrap();
    let batches: std::collections::HashSet<i32> = rows.iter().map(|(_, b)| *b).collect();
    assert_eq!(
        batches.len(),
        rows.len(),
        "each migration should get its own batch"
    );
}

#[tokio::test]
async fn migrate_pretend_returns_sql_without_executing() {
    ensure_logging();
    let Some(pool) = pool().await else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    reset_schema(&pool).await;

    let runner = MigrationRunner::with_migrations(pool.clone(), migrations::all());
    let lines = runner.pretend().await.expect("pretend");
    assert!(
        !lines.is_empty(),
        "pretend should return at least the create-table SQL"
    );
    assert!(lines
        .iter()
        .any(|l| l.to_uppercase().contains("CREATE TABLE")));

    // Confirm the migrations table doesn't have any actual rows applied.
    let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM migrations")
        .fetch_one(pg(&pool))
        .await
        .unwrap();
    assert_eq!(count, 0, "pretend must not actually run migrations");
}

#[tokio::test]
async fn schema_table_alter_adds_and_drops_columns() {
    ensure_logging();
    let Some(pool) = pool().await else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    reset_schema(&pool).await;

    // Create a baseline table.
    sqlx::query("CREATE TABLE widgets (id BIGSERIAL PRIMARY KEY, name VARCHAR(255) NOT NULL)")
        .execute(pg(&pool))
        .await
        .unwrap();

    // Use Schema::table to ALTER it.
    let mut schema = cast::Schema::new();
    schema.table("widgets", |t| {
        t.string("color").nullable();
        t.integer("quantity").default("0");
        t.rename_column("name", "label");
    });
    for stmt in &schema.statements {
        sqlx::query(stmt).execute(pg(&pool)).await.expect(stmt);
    }

    let cols: Vec<(String,)> = sqlx::query_as(
        "SELECT column_name FROM information_schema.columns WHERE table_name = 'widgets' ORDER BY column_name",
    )
    .fetch_all(pg(&pool))
    .await
    .unwrap();
    let names: Vec<String> = cols.into_iter().map(|(n,)| n).collect();
    assert!(names.contains(&"color".to_string()));
    assert!(names.contains(&"quantity".to_string()));
    assert!(
        names.contains(&"label".to_string()),
        "rename failed: {names:?}"
    );
    assert!(
        !names.contains(&"name".to_string()),
        "old column should be gone: {names:?}"
    );
}

#[tokio::test]
async fn schema_richer_column_types_compile_to_valid_sql() {
    ensure_logging();
    let Some(pool) = pool().await else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    reset_schema(&pool).await;

    let mut schema = cast::Schema::new();
    schema.create("products", |t| {
        t.id();
        t.string("name").not_null();
        t.decimal("price", 10, 2).not_null();
        t.float("weight");
        t.boolean("in_stock").default("true");
        t.enum_col("status", &["draft", "active", "archived"]);
        t.date("released_on").nullable();
        t.remember_token();
        t.morphs("commentable");
        t.timestamps();
        t.soft_deletes();
        t.unique_index(&["name"]);
    });
    for stmt in &schema.statements {
        sqlx::query(stmt).execute(pg(&pool)).await.expect(stmt);
    }

    // Sanity: the new table has the expected columns.
    let cols: Vec<(String,)> = sqlx::query_as(
        "SELECT column_name FROM information_schema.columns WHERE table_name = 'products'",
    )
    .fetch_all(pg(&pool))
    .await
    .unwrap();
    let names: std::collections::HashSet<String> = cols.into_iter().map(|(n,)| n).collect();
    for expected in [
        "id",
        "name",
        "price",
        "weight",
        "in_stock",
        "status",
        "released_on",
        "remember_token",
        "commentable_id",
        "commentable_type",
        "created_at",
        "updated_at",
        "deleted_at",
    ] {
        assert!(
            names.contains(expected),
            "missing column {expected}: {names:?}"
        );
    }
}

#[tokio::test]
async fn multi_connection_manager_resolves_named_connections() {
    ensure_logging();
    let Some(pool) = pool().await else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };

    use cast::{Connection, ConnectionManager};
    use std::collections::HashMap;

    let mut conns = HashMap::new();
    conns.insert(
        "default".to_string(),
        Connection {
            name: "default".into(),
            write: pool.clone(),
            reads: Vec::new(),
        },
    );
    conns.insert(
        "replica".to_string(),
        Connection {
            name: "replica".into(),
            write: pool.clone(),
            reads: vec![pool.clone()],
        },
    );

    let mgr = ConnectionManager::from_connections("default", conns);
    assert!(mgr.get("default").is_some());
    assert!(mgr.get("replica").is_some());
    assert!(mgr.get("nope").is_none());
    assert_eq!(mgr.default_name(), "default");

    // Reader for the replica should return one of the read pools.
    let replica = mgr.get("replica").unwrap();
    let _: &cast::Pool = replica.reader();
    let _: &cast::Pool = replica.writer();
}

#[tokio::test]
async fn queue_db_driver_push_pop() {
    ensure_logging();
    let Some(pool) = pool().await else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    reset_schema(&pool).await;
    let runner = MigrationRunner::with_migrations(pool.clone(), migrations::all());
    runner.run_up().await.unwrap();

    let queue = anvilforge::queue::QueueHandle::database(pg(&pool).clone());
    let payload = anvilforge::queue::QueuePayload {
        id: uuid::Uuid::new_v4(),
        job_type: "TestJob".into(),
        data: serde_json::json!({"hello": "world"}),
        attempts: 0,
        max_attempts: 3,
        queue: "default".into(),
    };

    queue.push(payload.clone()).await.unwrap();

    let popped = queue
        .pop("default")
        .await
        .expect("pop")
        .expect("queue had a job");
    assert_eq!(popped.job_type, "TestJob");
    assert_eq!(popped.data, serde_json::json!({"hello": "world"}));
}
